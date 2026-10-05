//! The aider-rs core library, kept separate from the MCP host binary so the
//! two test seams agreed in the spec can drive it independently:
//!
//! 1. the process-level MCP seam (`tests/`), and
//! 2. the core library seam (`core` module unit tests).

pub mod core;
pub mod mcp;