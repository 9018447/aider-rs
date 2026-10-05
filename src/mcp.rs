//! Model Context Protocol stdio host for aider-rs.
//!
//! The process speaks newline-delimited JSON-RPC 2.0 on stdin/stdout, per the
//! MCP stdio transport. One JSON object per line, responses flushed per
//! message. stdout is protocol-only; all logs go to stderr.
//!
//! The process is session-resident by design: one Claude Code session, one
//! process, one `Session`. Nothing survives the process; durable state lives
//! in git and on disk only.

use std::io::{self, BufRead, BufReader, Write};

use serde_json::{json, Value};

use crate::core::config::{self, Config};
use crate::core::git::short_hash;
use crate::core::session::Session;
use crate::core::task;

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

/// Stateful MCP server: owns the aider-rs session for this Claude Code
/// session's lifetime.
pub struct Server {
    initialized: bool,
    session: Session,
    config: Config,
}

impl Default for Server {
    fn default() -> Self {
        Self::new()
    }
}

impl Server {
    pub fn new() -> Self {
        Self {
            initialized: false,
            session: Session::new(),
            config: config::load(),
        }
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
        let arguments = params.get("arguments").cloned().unwrap_or_else(|| json!({}));
        match name {
            tools::TASK => self.tool_task(&arguments),
            tools::UNDO => self.tool_undo(),
            tools::STATUS => Ok(text_result(task::status(&self.session, &self.config).to_string(), false)),
            _ => Err(json!({
                "code": -32602,
                "message": format!("unknown tool: {name}"),
            })),
        }
    }

    fn tool_task(&mut self, arguments: &Value) -> Result<Value, Value> {
        let task_text = arguments
            .get("task")
            .and_then(Value::as_str)
            .map(str::to_string)
            .ok_or_else(|| json!({
                "code": -32602,
                "message": "aider_task requires a `task` string argument",
            }))?;
        let files: Vec<String> = arguments
            .get("files")
            .and_then(Value::as_array)
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();
        let reset_context = arguments
            .get("reset_context")
            .and_then(Value::as_bool)
            .unwrap_or(false);

        let args = task::TaskArgs {
            task: task_text,
            files,
            reset_context,
        };
        match task::run_task(&mut self.session, &self.config, args) {
            Ok(outcome) => {
                let mut text = String::new();
                if outcome.ok && outcome.commit.is_some() {
                    text.push_str(&format!(
                        "aider_task OK — {} file(s) edited, committed as {}.\n",
                        outcome.files.len(),
                        outcome.commit.as_deref().map(short_hash).unwrap_or_else(|| "?".to_string())
                    ));
                } else {
                    text.push_str("aider_task OK — no edits applied (conversational reply).\n");
                }
                text.push_str(&format!(
                    "tokens: {} in / {} out\n",
                    outcome.usage.input_tokens, outcome.usage.output_tokens
                ));
                if !outcome.shells.is_empty() {
                    text.push_str("\nNOTE: the model asked to run shell commands; NOT executed:\n");
                    for s in &outcome.shells {
                        text.push_str(&format!("  $ {}\n", s.trim()));
                    }
                }
                if !outcome.summary.trim().is_empty() {
                    text.push_str(&format!("\nmodel reply:\n{}\n", outcome.summary.trim()));
                }
                if !outcome.diff.trim().is_empty() {
                    text.push_str(&format!("\ndiff:\n{}\n", outcome.diff.trim_end()));
                }
                Ok(text_result(text, false))
            }
            Err(err) => Ok(text_result(format!("aider_task FAILED: {err}"), true)),
        }
    }

    fn tool_undo(&mut self) -> Result<Value, Value> {
        match task::undo(&mut self.session) {
            Ok(msg) => Ok(text_result(msg, false)),
            Err(err) => Ok(text_result(format!("aider_undo FAILED: {err}"), true)),
        }
    }
}

fn text_result(text: String, is_error: bool) -> Value {
    json!({
        "content": [{ "type": "text", "text": text }],
        "isError": is_error,
    })
}

fn tool_definitions() -> Value {
    json!({
        "tools": [
            {
                "name": tools::TASK,
                "description": "Send a natural-language coding task to aider-rs. It runs its own LLM loop, applies edits to files in the current repository, auto-commits each successful edit round, and returns the resulting diff, commit hash, and token usage.",
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
                "description": "Undo the last aider-rs edit by resetting the worktree back along the auto-commit chain. Call repeatedly to step back further. Only ever rewinds commits aider-rs itself made, and only while HEAD still points at them.",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": tools::STATUS,
                "description": "Report the current aider-rs session state (context, model, tasks run, tokens) and the git worktree state (branch, dirty files, recent commits).",
                "inputSchema": { "type": "object", "properties": {} }
            }
        ]
    })
}