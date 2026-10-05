//! End-to-end tests at the process MCP seam with a mock OpenAI-compatible
//! LLM server: drive the real binary over stdin/stdout JSON-RPC exactly as
//! Claude Code does, with a real temp git repo as the worktree.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::thread;

use serde_json::{json, Value};

// ---------------------------------------------------------------------------
// Mock OpenAI-compatible server
// ---------------------------------------------------------------------------

struct MockLlm {
    /// Canned assistant contents, served in order, one per HTTP request.
    responses: Vec<String>,
}

impl MockLlm {
    /// Serve on an ephemeral port; returns the base URL (`http://127.0.0.1:PORT/v1`).
    fn start(responses: Vec<String>) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind mock llm");
        let addr = listener.local_addr().expect("local addr");
        thread::spawn(move || {
            for body in responses {
                if let Ok((stream, _)) = listener.accept() {
                    serve_one_completion(stream, &body);
                }
            }
        });
        format!("http://{addr}/v1")
    }
}

fn serve_one_completion(mut stream: TcpStream, assistant_content: &str) {
    // Read headers, then Content-Length body (we don't need to parse it).
    let mut buf = Vec::new();
    let mut byte = [0u8; 1];
    loop {
        match stream.read(&mut byte) {
            Ok(0) | Err(_) => return,
            Ok(_) => {
                buf.push(byte[0]);
                if buf.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
        }
    }
    let headers = String::from_utf8_lossy(&buf).to_ascii_lowercase();
    let content_length: usize = headers
        .lines()
        .find_map(|l| l.strip_prefix("content-length:"))
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0);
    let mut body = vec![0u8; content_length];
    if content_length > 0 {
        let _ = stream.read_exact(&mut body);
    }

    let payload = json!({
        "id": "chatcmpl-mock",
        "object": "chat.completion",
        "model": "mock-model",
        "choices": [{
            "index": 0,
            "message": { "role": "assistant", "content": assistant_content },
            "finish_reason": "stop",
        }],
        "usage": { "prompt_tokens": 111, "completion_tokens": 42 },
    })
    .to_string();
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        payload.len(),
        payload
    );
    let _ = stream.write_all(response.as_bytes());
    let _ = stream.flush();
}

// ---------------------------------------------------------------------------
// MCP client harness
// ---------------------------------------------------------------------------

struct Session {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: i64,
}

impl Session {
    fn start_in(dir: &std::path::Path, base_url: &str) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_aider-rs"))
            .current_dir(dir)
            .env("OPENAI_API_KEY", "test-key")
            .env("OPENAI_API_BASE", base_url)
            .env("AIDER_RS_MODEL", "mock-model")
            .env_remove("ANTHROPIC_API_KEY")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn aider-rs binary");
        let stdin = child.stdin.take().expect("child stdin");
        let stdout = child.stdout.take().expect("child stdout");
        Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
            next_id: 0,
        }
    }

    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let msg = json!({
            "jsonrpc": "2.0",
            "id": self.next_id,
            "method": method,
            "params": params,
        });
        writeln!(self.stdin, "{msg}").unwrap();
        self.stdin.flush().unwrap();
        let mut line = String::new();
        self.stdout
            .read_line(&mut line)
            .expect("read a JSON-RPC line from stdout");
        assert!(!line.is_empty(), "server closed stdout unexpectedly");
        serde_json::from_str(line.trim()).expect("valid JSON-RPC line")
    }

    fn notify(&mut self, method: &str) {
        let msg = json!({ "jsonrpc": "2.0", "method": method });
        writeln!(self.stdin, "{msg}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn handshake(&mut self) {
        let init = self.request(
            "initialize",
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": { "name": "e2e-test", "version": "0" },
            }),
        );
        assert_eq!(init["result"]["serverInfo"]["name"], "aider-rs");
        self.notify("notifications/initialized");
    }

    fn call_tool(&mut self, name: &str, arguments: Value) -> Value {
        self.request("tools/call", json!({ "name": name, "arguments": arguments }))
    }

    fn tool_text(resp: &Value) -> String {
        resp["result"]["content"][0]["text"]
            .as_str()
            .expect("tool text")
            .to_string()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

// ---------------------------------------------------------------------------
// Temp git repo fixture
// ---------------------------------------------------------------------------

fn temp_repo(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("aiders-e2e-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    run_git(&dir, &["init", "-q"]);
    run_git(&dir, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "--allow-empty", "-m", "init", "--"]);
    dir
}

fn run_git(dir: &std::path::Path, args: &[&str]) -> String {
    let out = Command::new("git")
        .args(["-C", &dir.to_string_lossy()])
        .args(args)
        .output()
        .expect("run git");
    assert!(
        out.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn task_edits_commits_and_undo_restores() {
    let repo = temp_repo("task-undo");
    let file = repo.join("a.txt");
    std::fs::write(&file, "alpha\nbeta\n").unwrap();
    run_git(&repo, &["add", "a.txt"]);
    run_git(&repo, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-m", "base", "--"]);
    let baseline = run_git(&repo, &["rev-parse", "HEAD"]);

    let reply = "Renaming alpha to ALPHA.\n\na.txt\n<<<<<<< SEARCH\nalpha\n=======\nALPHA\n>>>>>>> REPLACE\n";
    let base_url = MockLlm::start(vec![reply.to_string()]);

    let mut s = Session::start_in(&repo, &base_url);
    s.handshake();

    let resp = s.call_tool(
        "aider_task",
        json!({ "task": "change alpha to ALPHA", "files": ["a.txt"] }),
    );
    let text = Session::tool_text(&resp);
    assert_eq!(resp["result"]["isError"], false, "task should succeed: {text}");
    assert!(text.contains("committed as"), "reports the commit: {text}");
    assert!(text.contains("tokens: 111 in / 42 out"), "reports usage: {text}");

    // File really edited.
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "ALPHA\nbeta\n");
    // Commit really created, on top of baseline.
    let head = run_git(&repo, &["rev-parse", "HEAD"]);
    assert_ne!(head, baseline);
    let msg = run_git(&repo, &["log", "-1", "--pretty=%s"]);
    assert!(msg.starts_with("aider-rs:"), "commit message is aider-rs prefixed: {msg}");

    // Status reflects the session.
    let status = s.call_tool("aider_status", json!({}));
    let st = Session::tool_text(&status);
    assert!(st.contains("\"tasks_run\":1"), "status reports tasks_run: {st}");
    assert!(st.contains("\"commits_this_session\":1"), "status reports commits: {st}");

    // Undo rewinds the edit.
    let undo = s.call_tool("aider_undo", json!({}));
    let ut = Session::tool_text(&undo);
    assert_eq!(undo["result"]["isError"], false, "undo should succeed: {ut}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "alpha\nbeta\n", "undo restored content");
    assert_eq!(run_git(&repo, &["rev-parse", "HEAD"]), baseline, "undo rewound HEAD");

    // Undo with nothing left to undo is a clean failure.
    let undo2 = s.call_tool("aider_undo", json!({}));
    assert_eq!(undo2["result"]["isError"], true, "second undo fails cleanly");

    let _ = std::fs::remove_dir_all(&repo);
}

#[test]
fn failed_match_retries_once_then_reports_without_writing() {
    let repo = temp_repo("fail");
    let file = repo.join("a.txt");
    std::fs::write(&file, "alpha\n").unwrap();
    run_git(&repo, &["add", "a.txt"]);
    run_git(&repo, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-m", "base", "--"]);
    let baseline = run_git(&repo, &["rev-parse", "HEAD"]);

    // Both attempts return a block that cannot match.
    let bad = "a.txt\n<<<<<<< SEARCH\ndoes-not-exist\n=======\nX\n>>>>>>> REPLACE\n";
    let base_url = MockLlm::start(vec![bad.to_string(), bad.to_string()]);

    let mut s = Session::start_in(&repo, &base_url);
    s.handshake();

    let resp = s.call_tool(
        "aider_task",
        json!({ "task": "change something", "files": ["a.txt"] }),
    );
    let text = Session::tool_text(&resp);
    assert_eq!(resp["result"]["isError"], true, "unmatchable edits fail: {text}");
    assert!(text.contains("failed to match"), "failure explains itself: {text}");
    assert!(text.contains("nothing was written"), "atomicity is stated: {text}");

    // Nothing was written, no commit was made.
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "alpha\n");
    assert_eq!(run_git(&repo, &["rev-parse", "HEAD"]), baseline);

    let _ = std::fs::remove_dir_all(&repo);
}

#[test]
fn retry_round_can_recover_after_initial_mismatch() {
    let repo = temp_repo("retry");
    let file = repo.join("a.txt");
    std::fs::write(&file, "alpha\n").unwrap();
    run_git(&repo, &["add", "a.txt"]);
    run_git(&repo, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-m", "base", "--"]);

    // First attempt misses; second attempt (after failure feedback) matches.
    let bad = "a.txt\n<<<<<<< SEARCH\nnope\n=======\nX\n>>>>>>> REPLACE\n";
    let good = "a.txt\n<<<<<<< SEARCH\nalpha\n=======\nALPHA\n>>>>>>> REPLACE\n";
    let base_url = MockLlm::start(vec![bad.to_string(), good.to_string()]);

    let mut s = Session::start_in(&repo, &base_url);
    s.handshake();

    let resp = s.call_tool("aider_task", json!({ "task": "uppercase alpha", "files": ["a.txt"] }));
    let text = Session::tool_text(&resp);
    assert_eq!(resp["result"]["isError"], false, "retry recovers: {text}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "ALPHA\n");

    let _ = std::fs::remove_dir_all(&repo);
}

#[test]
fn new_file_creation_via_empty_search_block() {
    let repo = temp_repo("newfile");
    let reply = "Creating a new module.\n\nsrc/hello.py\n<<<<<<< SEARCH\n=======\ndef hello():\n    print(\"hello\")\n>>>>>>> REPLACE\n";
    let base_url = MockLlm::start(vec![reply.to_string()]);

    let mut s = Session::start_in(&repo, &base_url);
    s.handshake();

    let resp = s.call_tool("aider_task", json!({ "task": "create src/hello.py with a hello function" }));
    let text = Session::tool_text(&resp);
    assert_eq!(resp["result"]["isError"], false, "new file creation works: {text}");
    let created = repo.join("src").join("hello.py");
    assert_eq!(
        std::fs::read_to_string(&created).unwrap(),
        "def hello():\n    print(\"hello\")\n"
    );
    let tracked = run_git(&repo, &["ls-files"]);
    assert!(tracked.contains("src/hello.py"), "new file is committed: {tracked}");

    let _ = std::fs::remove_dir_all(&repo);
}

#[test]
fn shell_commands_are_reported_not_executed() {
    let repo = temp_repo("shell");
    std::fs::write(repo.join("a.txt"), "alpha\n").unwrap();
    run_git(&repo, &["add", "a.txt"]);
    run_git(&repo, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-m", "base", "--"]);

    let reply = "Running a command and editing.\n\n```bash\nrm -rf /\n```\n\na.txt\n<<<<<<< SEARCH\nalpha\n=======\nALPHA\n>>>>>>> REPLACE\n";
    let base_url = MockLlm::start(vec![reply.to_string()]);

    let mut s = Session::start_in(&repo, &base_url);
    s.handshake();

    let resp = s.call_tool("aider_task", json!({ "task": "do it", "files": ["a.txt"] }));
    let text = Session::tool_text(&resp);
    assert!(text.contains("NOT executed"), "shell commands reported: {text}");
    assert!(text.contains("rm -rf /"), "the command text is surfaced: {text}");
    assert_eq!(std::fs::read_to_string(repo.join("a.txt")).unwrap(), "ALPHA\n", "edit still applied");

    let _ = std::fs::remove_dir_all(&repo);
}

#[test]
fn task_outside_git_repo_fails_with_clear_error() {
    let dir = std::env::temp_dir().join(format!("aiders-e2e-nogit-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();

    let base_url = MockLlm::start(vec!["hi".to_string()]);
    let mut s = Session::start_in(&dir, &base_url);
    s.handshake();

    let resp = s.call_tool("aider_task", json!({ "task": "anything" }));
    let text = Session::tool_text(&resp);
    assert_eq!(resp["result"]["isError"], true);
    assert!(text.contains("git repository"), "clear error: {text}");

    let _ = std::fs::remove_dir_all(&dir);
}