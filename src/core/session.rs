//! Session state for one Claude Code session. Nothing here survives the
//! process; durable state lives in git and on disk only.

use crate::core::llm::ChatMessage;

#[derive(Debug, Default)]
pub struct Session {
    /// Conversation history carried between aider_task calls in this session.
    pub history: Vec<ChatMessage>,
    /// Commits created by this session, oldest first (the undo chain).
    pub commits: Vec<String>,
    /// Task counters for status reporting.
    pub tasks_run: u64,
    pub tasks_failed: u64,
    /// Accumulated token usage.
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    /// Model description for status reporting.
    pub model: String,
}

impl Session {
    pub fn new() -> Self {
        Self::default()
    }

    /// Drop accumulated conversation history (per-task option).
    pub fn reset_context(&mut self) {
        self.history.clear();
    }

    pub fn push_user(&mut self, content: &str) {
        self.history.push(ChatMessage {
            role: "user".into(),
            content: content.to_string(),
        });
    }

    pub fn push_assistant(&mut self, content: &str) {
        self.history.push(ChatMessage {
            role: "assistant".into(),
            content: content.to_string(),
        });
    }
}