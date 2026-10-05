//! The aider-rs core: edit application engine, LLM client, git integration,
//! session state, configuration, and task orchestration.
//!
//! Edit semantics are ported from aider's `editblock_coder.py` (Apache-2.0):
//! SEARCH/REPLACE blocks, filename look-back, first-match replacement,
//! whitespace-flexible matching, `...` elision, append/new-file rules.
//! One deliberate deviation, required by the spec: edits are applied
//! atomically — if any block fails to match, nothing is written.

pub mod config;
pub mod editformat;
pub mod git;
pub mod llm;
pub mod prompts;
pub mod session;
pub mod task;