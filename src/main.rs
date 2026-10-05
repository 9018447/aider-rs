//! aider-rs: extremely lightweight Rust port of aider, as a Claude Code plugin
//! MCP host.
//!
//! Claude Code orchestrates and calls the MCP tools; aider-rs runs its own LLM
//! loop, edits files, and auto-commits to git. All diagnostics go to stderr:
//! stdout is the JSON-RPC protocol channel and must stay clean.

use aiders::mcp;

fn main() {
    if let Err(err) = mcp::serve_stdio() {
        eprintln!("aider-rs: fatal: {err}");
        std::process::exit(1);
    }
}