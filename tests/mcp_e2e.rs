//! Process-level end-to-end tests at the MCP seam: drive the real binary over
//! stdin/stdout JSON-RPC, exactly as Claude Code does.

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{json, Value};

struct Session {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    next_id: i64,
}

impl Session {
    fn start() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_aider-rs"))
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
        self.read_response()
    }

    fn notify(&mut self, method: &str) {
        let msg = json!({ "jsonrpc": "2.0", "method": method });
        writeln!(self.stdin, "{msg}").unwrap();
        self.stdin.flush().unwrap();
    }

    fn read_response(&mut self) -> Value {
        let mut line = String::new();
        self.stdout
            .read_line(&mut line)
            .expect("read a JSON-RPC line from stdout");
        assert!(!line.is_empty(), "server closed stdout unexpectedly");
        let value: Value = serde_json::from_str(line.trim()).expect("valid JSON-RPC line");
        // The protocol channel must carry exactly one JSON object per line.
        assert!(value.is_object(), "response line must be a JSON object");
        value
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // Never leave orphan processes behind, even if a test panics.
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[test]
fn handshake_lists_exactly_three_tools_and_stubs_calls() {
    let mut s = Session::start();

    let init = s.request(
        "initialize",
        json!({
            "protocolVersion": "2024-11-05",
            "capabilities": {},
            "clientInfo": { "name": "e2e-test", "version": "0" },
        }),
    );
    assert_eq!(init["result"]["serverInfo"]["name"], "aider-rs");
    assert_eq!(init["result"]["protocolVersion"], "2024-11-05");

    s.notify("notifications/initialized");

    let tools = s.request("tools/list", json!({}));
    let names: Vec<&str> = tools["result"]["tools"]
        .as_array()
        .expect("tools array")
        .iter()
        .map(|t| t["name"].as_str().expect("tool name"))
        .collect();
    assert_eq!(names.len(), 3, "exactly three tools are exposed");
    assert!(names.contains(&"aider_task"));
    assert!(names.contains(&"aider_undo"));
    assert!(names.contains(&"aider_status"));

    let call = s.request(
        "tools/call",
        json!({ "name": "aider_task", "arguments": { "task": "add a readme" } }),
    );
    assert_eq!(call["result"]["isError"], true, "skeleton stubs report isError");

    let unknown = s.request("tools/call", json!({ "name": "nope", "arguments": {} }));
    assert_eq!(unknown["error"]["code"], -32602, "unknown tool is a JSON-RPC error");

    let nomethod = s.request("bogus/method", json!({}));
    assert_eq!(nomethod["error"]["code"], -32601, "unknown method is -32601");
}

#[test]
fn stdout_carries_only_jsonrpc_lines() {
    let mut s = Session::start();
    let init = s.request("initialize", json!({ "protocolVersion": "2024-11-05" }));
    assert!(init["result"].get("serverInfo").is_some());
    let tools = s.request("tools/list", json!({}));
    assert!(tools["result"]["tools"].is_array());
}

#[test]
fn process_exits_cleanly_on_stdin_eof() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_aider-rs"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn");
    drop(child.stdin.take().expect("stdin"));
    let status = child.wait().expect("wait for exit");
    assert!(status.success(), "exits cleanly at EOF");
}