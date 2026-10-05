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

/// Serve canned assistant contents on an ephemeral port, one per HTTP
/// request, in order; returns the base URL (`http://127.0.0.1:PORT/v1`).
fn start_mock_llm(responses: Vec<String>) -> String {
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

struct McpClient {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: i64,
}

impl McpClient {
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

impl Drop for McpClient {
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
    let base_url = start_mock_llm(vec![reply.to_string()]);

    let mut s = McpClient::start_in(&repo, &base_url);
    s.handshake();

    let resp = s.call_tool(
        "aider_task",
        json!({ "task": "change alpha to ALPHA", "files": ["a.txt"] }),
    );
    let text = McpClient::tool_text(&resp);
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
    let st = McpClient::tool_text(&status);
    assert!(st.contains("\"tasks_run\":1"), "status reports tasks_run: {st}");
    assert!(st.contains("\"commits_this_session\":1"), "status reports commits: {st}");

    // Undo rewinds the edit.
    let undo = s.call_tool("aider_undo", json!({}));
    let ut = McpClient::tool_text(&undo);
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
    let base_url = start_mock_llm(vec![bad.to_string(), bad.to_string()]);

    let mut s = McpClient::start_in(&repo, &base_url);
    s.handshake();

    let resp = s.call_tool(
        "aider_task",
        json!({ "task": "change something", "files": ["a.txt"] }),
    );
    let text = McpClient::tool_text(&resp);
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
    let base_url = start_mock_llm(vec![bad.to_string(), good.to_string()]);

    let mut s = McpClient::start_in(&repo, &base_url);
    s.handshake();

    let resp = s.call_tool("aider_task", json!({ "task": "uppercase alpha", "files": ["a.txt"] }));
    let text = McpClient::tool_text(&resp);
    assert_eq!(resp["result"]["isError"], false, "retry recovers: {text}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "ALPHA\n");

    let _ = std::fs::remove_dir_all(&repo);
}

#[test]
fn new_file_creation_via_empty_search_block() {
    let repo = temp_repo("newfile");
    let reply = "Creating a new module.\n\nsrc/hello.py\n<<<<<<< SEARCH\n=======\ndef hello():\n    print(\"hello\")\n>>>>>>> REPLACE\n";
    let base_url = start_mock_llm(vec![reply.to_string()]);

    let mut s = McpClient::start_in(&repo, &base_url);
    s.handshake();

    let resp = s.call_tool("aider_task", json!({ "task": "create src/hello.py with a hello function" }));
    let text = McpClient::tool_text(&resp);
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
    let base_url = start_mock_llm(vec![reply.to_string()]);

    let mut s = McpClient::start_in(&repo, &base_url);
    s.handshake();

    let resp = s.call_tool("aider_task", json!({ "task": "do it", "files": ["a.txt"] }));
    let text = McpClient::tool_text(&resp);
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

    let base_url = start_mock_llm(vec!["hi".to_string()]);
    let mut s = McpClient::start_in(&dir, &base_url);
    s.handshake();

    let resp = s.call_tool("aider_task", json!({ "task": "anything" }));
    let text = McpClient::tool_text(&resp);
    assert_eq!(resp["result"]["isError"], true);
    assert!(text.contains("git repository"), "clear error: {text}");

    let _ = std::fs::remove_dir_all(&dir);
}
#[test]
fn tools_list_exposes_three_tools_with_schemas() {
    let repo = temp_repo("listtools");
    let base_url = start_mock_llm(vec![]);
    let mut s = McpClient::start_in(&repo, &base_url);
    s.handshake();

    let resp = s.request("tools/list", json!({}));
    let tools = resp["result"]["tools"].as_array().expect("tools array");
    let names: Vec<&str> = tools.iter().filter_map(|t| t["name"].as_str()).collect();
    assert_eq!(names, vec!["aider_task", "aider_undo", "aider_status"]);
    let task = tools.iter().find(|t| t["name"] == "aider_task").unwrap();
    assert_eq!(task["inputSchema"]["required"], json!(["task"]));
    assert_eq!(task["inputSchema"]["type"], "object");

    let _ = std::fs::remove_dir_all(&repo);
}

#[test]
fn multi_undo_rewinds_commits_in_order() {
    let repo = temp_repo("multiundo");
    let file = repo.join("a.txt");
    std::fs::write(&file, "v0\n").unwrap();
    run_git(&repo, &["add", "a.txt"]);
    run_git(&repo, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-m", "base", "--"]);
    let baseline = run_git(&repo, &["rev-parse", "HEAD"]);

    let r1 = "a.txt\n<<<<<<< SEARCH\nv0\n=======\nv1\n>>>>>>> REPLACE\n";
    let r2 = "a.txt\n<<<<<<< SEARCH\nv1\n=======\nv2\n>>>>>>> REPLACE\n";
    let base_url = start_mock_llm(vec![r1.to_string(), r2.to_string()]);

    let mut s = McpClient::start_in(&repo, &base_url);
    s.handshake();
    let _ = s.call_tool("aider_task", json!({ "task": "make v1", "files": ["a.txt"] }));
    let _ = s.call_tool("aider_task", json!({ "task": "make v2", "files": ["a.txt"] }));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "v2\n");

    // Undo twice: v2 -> v1 -> v0, HEAD back to baseline.
    let u1 = s.call_tool("aider_undo", json!({}));
    assert_eq!(u1["result"]["isError"], false, "{}", McpClient::tool_text(&u1));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "v1\n");
    let u2 = s.call_tool("aider_undo", json!({}));
    assert_eq!(u2["result"]["isError"], false, "{}", McpClient::tool_text(&u2));
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "v0\n");
    assert_eq!(run_git(&repo, &["rev-parse", "HEAD"]), baseline);

    let _ = std::fs::remove_dir_all(&repo);
}

#[test]
fn undo_keeps_unrelated_dirty_files_but_refuses_conflicts() {
    let repo = temp_repo("undokeep");
    let a = repo.join("a.txt");
    let b = repo.join("b.txt");
    std::fs::write(&a, "alpha\n").unwrap();
    std::fs::write(&b, "beta\n").unwrap();
    run_git(&repo, &["add", "a.txt", "b.txt"]);
    run_git(&repo, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-m", "base", "--"]);

    let r = "a.txt\n<<<<<<< SEARCH\nalpha\n=======\nALPHA\n>>>>>>> REPLACE\n";
    let base_url = start_mock_llm(vec![r.to_string()]);
    let mut s = McpClient::start_in(&repo, &base_url);
    s.handshake();
    let _ = s.call_tool("aider_task", json!({ "task": "uppercase alpha", "files": ["a.txt"] }));

    // Someone (e.g. Claude Code) leaves an uncommitted change in an
    // unrelated file; undo must preserve it.
    std::fs::write(&b, "beta-EDITED\n").unwrap();
    let u = s.call_tool("aider_undo", json!({}));
    assert_eq!(u["result"]["isError"], false, "{}", McpClient::tool_text(&u));
    assert_eq!(std::fs::read_to_string(&a).unwrap(), "alpha\n", "edit rewound");
    assert_eq!(std::fs::read_to_string(&b).unwrap(), "beta-EDITED\n", "unrelated dirty change preserved");

    let _ = std::fs::remove_dir_all(&repo);
}

#[test]
fn whole_file_fallback_creates_and_commits() {
    let repo = temp_repo("whole");
    let reply = "Writing the whole module.\n\nsrc/greet.py\n```python\ndef greet():\n    return \"hi\"\n```\n";
    let base_url = start_mock_llm(vec![reply.to_string()]);

    let mut s = McpClient::start_in(&repo, &base_url);
    s.handshake();
    let resp = s.call_tool("aider_task", json!({ "task": "create src/greet.py" }));
    assert_eq!(resp["result"]["isError"], false, "{}", McpClient::tool_text(&resp));
    assert_eq!(
        std::fs::read_to_string(repo.join("src").join("greet.py")).unwrap(),
        "def greet():\n    return \"hi\"\n"
    );
    let tracked = run_git(&repo, &["ls-files"]);
    assert!(tracked.contains("src/greet.py"));

    let _ = std::fs::remove_dir_all(&repo);
}

#[test]
fn anthropic_native_provider_end_to_end() {
    let repo = temp_repo("anthropic");
    std::fs::write(repo.join("a.txt"), "alpha\n").unwrap();
    run_git(&repo, &["add", "a.txt"]);
    run_git(&repo, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-m", "base", "--"]);

    // Mock Anthropic /v1/messages server on an ephemeral port.
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let content = "a.txt\n<<<<<<< SEARCH\nalpha\n=======\nALPHA\n>>>>>>> REPLACE\n".to_string();
    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            // read headers
            let mut buf = Vec::new();
            let mut byte = [0u8; 1];
            while let Ok(1) = stream.read(&mut byte) {
                buf.push(byte[0]);
                if buf.ends_with(b"\r\n\r\n") {
                    break;
                }
            }
            let headers = String::from_utf8_lossy(&buf).to_ascii_lowercase();
            let len: usize = headers
                .lines()
                .find_map(|l| l.strip_prefix("content-length:"))
                .and_then(|v| v.trim().parse().ok())
                .unwrap_or(0);
            let mut body = vec![0u8; len];
            if len > 0 {
                let _ = stream.read_exact(&mut body);
            }
            assert!(headers.contains("x-api-key"), "anthropic key header present");
            let payload = json!({
                "id": "msg_mock",
                "type": "message",
                "role": "assistant",
                "content": [{ "type": "text", "text": content }],
                "model": "claude-mock",
                "usage": { "input_tokens": 50, "output_tokens": 7 },
            }).to_string();
            let resp = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                payload.len(), payload
            );
            let _ = stream.write_all(resp.as_bytes());
        }
    });

    let mut child = Command::new(env!("CARGO_BIN_EXE_aider-rs"))
        .current_dir(&repo)
        .env("ANTHROPIC_API_KEY", "test-key")
        .env("ANTHROPIC_BASE_URL", format!("http://{addr}"))
        .env("AIDER_RS_PROVIDER", "anthropic")
        .env("AIDER_RS_MODEL", "claude-mock")
        .env_remove("OPENAI_API_KEY")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn");
    let stdin = child.stdin.take().unwrap();
    let stdout = BufReader::new(child.stdout.take().unwrap());
    let mut s = McpClient {
        child,
        stdin,
        stdout,
        next_id: 0,
    };
    s.handshake();
    let resp = s.call_tool("aider_task", json!({ "task": "uppercase alpha", "files": ["a.txt"] }));
    let text = McpClient::tool_text(&resp);
    assert_eq!(resp["result"]["isError"], false, "{text}");
    assert!(text.contains("tokens: 50 in / 7 out"), "anthropic usage reported: {text}");
    assert_eq!(std::fs::read_to_string(repo.join("a.txt")).unwrap(), "ALPHA\n");

    let _ = std::fs::remove_dir_all(&repo);
}

#[test]
fn config_file_precedence_model_visible_in_status() {
    let repo = temp_repo("cfgfile");
    std::fs::write(
        repo.join(".aider-rs.json"),
        serde_json::json!({ "model": "file-model", "provider": "openai" }).to_string(),
    )
    .unwrap();
    let base_url = start_mock_llm(vec![]);

    // No AIDER_RS_MODEL env: the repo config file must win.
    let mut child = Command::new(env!("CARGO_BIN_EXE_aider-rs"))
        .current_dir(&repo)
        .env("OPENAI_API_KEY", "k")
        .env("OPENAI_API_BASE", &base_url)
        .env_remove("AIDER_RS_MODEL")
        .env_remove("ANTHROPIC_API_KEY")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn");
    let stdin = child.stdin.take().unwrap();
    let stdout = BufReader::new(child.stdout.take().unwrap());
    let mut s = McpClient { child, stdin, stdout, next_id: 0 };
    s.handshake();
    let status = s.call_tool("aider_status", json!({}));
    let st = McpClient::tool_text(&status);
    assert!(st.contains("file-model"), "config-file model wins over defaults: {st}");

    let _ = std::fs::remove_dir_all(&repo);
}

#[test]
fn reset_context_and_shared_context_across_tasks() {
    let repo = temp_repo("ctx");
    let file = repo.join("a.txt");
    std::fs::write(&file, "v0\n").unwrap();
    run_git(&repo, &["add", "a.txt"]);
    run_git(&repo, &["-c", "user.name=t", "-c", "user.email=t@t", "commit", "-m", "base", "--"]);

    let r1 = "a.txt\n<<<<<<< SEARCH\nv0\n=======\nv1\n>>>>>>> REPLACE\n";
    let r2 = "a.txt\n<<<<<<< SEARCH\nv1\n=======\nv2\n>>>>>>> REPLACE\n";
    let r3 = "a.txt\n<<<<<<< SEARCH\nv2\n=======\nv3\n>>>>>>> REPLACE\n";
    let base_url = start_mock_llm(vec![r1.to_string(), r2.to_string(), r3.to_string()]);

    let mut s = McpClient::start_in(&repo, &base_url);
    s.handshake();

    // Task 1: context starts with the user message (+assistant after reply).
    let _ = s.call_tool("aider_task", json!({ "task": "v1", "files": ["a.txt"] }));
    let st = McpClient::tool_text(&s.call_tool("aider_status", json!({})));
    assert!(st.contains("\"context_messages\":2"), "after task 1: {st}");

    // Task 2 (shared): context accumulates.
    let _ = s.call_tool("aider_task", json!({ "task": "v2", "files": ["a.txt"] }));
    let st = McpClient::tool_text(&s.call_tool("aider_status", json!({})));
    assert!(st.contains("\"context_messages\":4"), "after task 2: {st}");

    // Task 3 with reset_context: fresh conversation.
    let _ = s.call_tool(
        "aider_task",
        json!({ "task": "v3", "files": ["a.txt"], "reset_context": true }),
    );
    let st = McpClient::tool_text(&s.call_tool("aider_status", json!({})));
    assert!(st.contains("\"context_messages\":2"), "after reset: {st}");
    assert_eq!(std::fs::read_to_string(&file).unwrap(), "v3\n");

    let _ = std::fs::remove_dir_all(&repo);
}
