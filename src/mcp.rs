//! Model Context Protocol stdio host for aider-rs.
//!
//! The process speaks newline-delimited JSON-RPC 2.0 on stdin/stdout, per the
//! MCP stdio transport. One JSON object per line, responses flushed per
//! message. stdout is protocol-only; all logs go to stderr.

use std::io::{self, BufRead, BufReader, Write};

use serde_json::{json, Value};

pub const SERVER_NAME: &str = "aider-rs";
pub const SERVER_VERSION: &str = env!("CARGO_PKG_VERSION");

/// The tools aider-rs exposes to Claude Code, by name.
pub mod tools {
    pub const TASK: &str = "aider_task";
    pub const UNDO: &str = "aider_undo";
    pub const STATUS: &str = "aider_status";
}

/// Run the stdio server loop until stdin reaches EOF, then exit cleanly.
pub fn serve_stdio() -> io::Result<()> {
    let reader = BufReader::new(io::stdin().lock());
    let mut out = io::stdout().lock();
    let mut server = Server::new();
    for line in reader.lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let msg: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(err) => {
                eprintln!("aider-rs: ignoring malformed JSON-RPC line: {err}");
                continue;
            }
        };
        if let Some(response) = server.handle(msg) {
            writeln!(out, "{response}")?;
            out.flush()?;
        }
    }
    Ok(())
}

/// Stateful MCP server: owns the aider-rs session across one Claude Code
/// session's lifetime (session-resident by design; nothing survives the
/// process).
pub struct Server {
    initialized: bool,
}

impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}

impl Server {
    pub fn new() -> Self {
        Self { initialized: false }
    }

    /// Handle one incoming JSON-RPC message; return the encoded response
    /// line, or `None` for notifications (which are never answered).
    pub fn handle(&mut self, msg: Value) -> Option<String> {
        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(Value::as_str).unwrap_or("");
        let params = msg.get("params").cloned().unwrap_or_else(|| json!({}));

        let result: Result<Value, Value> = match method {
            "initialize" => Ok(self.handle_initialize(&params)),
            "notifications/initialized" => {
                self.initialized = true;
                return None;
            }
            "ping" => Ok(json!({})),
            "tools/list" => Ok(tool_definitions()),
            "tools/call" => self.handle_tool_call(&params),
            _ => Err(json!({
                "code": -32601,
                "message": format!("method not found: {method}"),
            })),
        };

        // Notifications have no id and are never answered, even on error.
        let id = id?;
        Some(
            match result {
                Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
                Err(error) => json!({ "jsonrpc": "2.0", "id": id, "error": error }),
            }
            .to_string(),
        )
    }

    fn handle_initialize(&mut self, params: &Value) -> Value {
        self.initialized = true;
        // Echo the client's protocol version when present (most compatible),
        // defaulting to the version Claude Code ships today.
        let protocol_version = params
            .get("protocolVersion")
            .and_then(Value::as_str)
            .unwrap_or("2024-11-05");
        json!({
            "protocolVersion": protocol_version,
            "capabilities": { "tools": {} },
            "serverInfo": { "name": SERVER_NAME, "version": SERVER_VERSION },
        })
    }

    fn handle_tool_call(&mut self, params: &Value) -> Result<Value, Value> {
        let name = params.get("name").and_then(Value::as_str).unwrap_or("");
        let known = matches!(name, tools::TASK | tools::UNDO | tools::STATUS);
        if !known {
            return Err(json!({
                "code": -32602,
                "message": format!("unknown tool: {name}"),
            }));
        }
        // T01 skeleton: the host and tool registry exist; the tool bodies are
        // implemented by the end-to-end task loop ticket.
        Ok(json!({
            "content": [{
                "type": "text",
                "text": format!(
                    "{SERVER_NAME} {SERVER_VERSION}: `{name}` is not implemented yet (host skeleton)."
                ),
            }],
            "isError": true,
        }))
    }
}

fn tool_definitions() -> Value {
    json!({
        "tools": [
            {
                "name": tools::TASK,
                "description": "Send a natural-language coding task to aider-rs. It runs its own LLM loop, applies edits to files in the current repository, auto-commits each successful edit, and returns the resulting diff, commit hash, and token usage.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "task": {
                            "type": "string",
                            "description": "The coding task, in natural language."
                        },
                        "files": {
                            "type": "array",
                            "items": { "type": "string" },
                            "description": "Optional explicit file paths to include as context / edit targets."
                        },
                        "reset_context": {
                            "type": "boolean",
                            "description": "Start a fresh aider-rs context instead of continuing the session's accumulated context."
                        }
                    },
                    "required": ["task"]
                }
            },
            {
                "name": tools::UNDO,
                "description": "Undo the last aider-rs edit by moving the worktree back along the auto-commit chain. Call repeatedly to step back further.",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": tools::STATUS,
                "description": "Report the current aider-rs session state (context, model, tasks run) and the git worktree state (branch, dirty files, recent commits).",
                "inputSchema": { "type": "object", "properties": {} }
            }
        ]
    })
}