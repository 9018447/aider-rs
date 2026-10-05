//! LLM clients: Anthropic native and OpenAI-compatible chat endpoints.
//!
//! Sync HTTP via ureq on purpose: the MCP host serves one tool call at a time
//! (Claude Code's tool calls are synchronous from aider-rs's point of view),
//! and a sync client keeps the binary small and the failure modes simple.

use std::time::Duration;

use serde_json::{json, Value};

use crate::core::config::{Config, Provider};

#[derive(Debug, Clone)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub role: String, // "user" | "assistant"
    pub content: String,
}

#[derive(Debug, Clone)]
pub struct ChatRequest {
    pub system: String,
    pub messages: Vec<ChatMessage>,
}

#[derive(Debug, Clone)]
pub struct ChatResponse {
    pub content: String,
    pub usage: Usage,
}

#[derive(Debug)]
pub enum LlmError {
    Http(String),
    Protocol(String),
    Timeout(String),
}

impl std::fmt::Display for LlmError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LlmError::Http(s) => write!(f, "llm http error: {s}"),
            LlmError::Protocol(s) => write!(f, "llm protocol error: {s}"),
            LlmError::Timeout(s) => write!(f, "llm timeout: {s}"),
        }
    }
}
impl std::error::Error for LlmError {}

/// Which concrete client to use, derived from the config.
pub fn client_from_config(config: &Config) -> Result<Box<dyn LlmClient>, LlmError> {
    let provider = config.provider.clone().unwrap_or(Provider::OpenAiCompatible);
    let api_key = config
        .api_key
        .clone()
        .ok_or_else(|| LlmError::Protocol("no API key configured (set ANTHROPIC_API_KEY or OPENAI_API_KEY, or api_key in the config file)".into()))?;
    let model = config
        .model
        .clone()
        .unwrap_or_else(|| match provider {
            Provider::Anthropic => "claude-sonnet-4-5".to_string(),
            Provider::OpenAiCompatible => "gpt-5.2".to_string(),
        });
    let timeout = Duration::from_secs(config.timeout_secs.unwrap_or(120));
    Ok(match provider {
        Provider::Anthropic => Box::new(AnthropicClient {
            api_key,
            model,
            base_url: config
                .base_url
                .clone()
                .unwrap_or_else(|| "https://api.anthropic.com".into()),
            timeout,
        }),
        Provider::OpenAiCompatible => Box::new(OpenAiClient {
            api_key,
            model,
            base_url: config
                .base_url
                .clone()
                .unwrap_or_else(|| "https://api.openai.com/v1".into()),
            timeout,
        }),
    })
}

pub trait LlmClient {
    fn complete(&self, req: &ChatRequest) -> Result<ChatResponse, LlmError>;
    fn model(&self) -> &str;
}

/// OpenAI-compatible `/chat/completions` (OpenAI, DeepSeek, OpenRouter,
/// Ollama, vLLM, and anything else speaking the same JSON).
pub struct OpenAiClient {
    pub api_key: String,
    pub model: String,
    pub base_url: String,
    pub timeout: Duration,
}

impl LlmClient for OpenAiClient {
    fn model(&self) -> &str {
        &self.model
    }

    fn complete(&self, req: &ChatRequest) -> Result<ChatResponse, LlmError> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let mut messages = vec![json!({ "role": "system", "content": req.system })];
        for m in &req.messages {
            messages.push(json!({ "role": m.role, "content": m.content }));
        }
        let body = json!({
            "model": self.model,
            "messages": messages,
            "stream": false,
        });
        let agent = ureq::AgentBuilder::new()
            .timeout(self.timeout)
            .build();
        let resp = agent
            .post(&url)
            .set("Authorization", &format!("Bearer {}", self.api_key))
            .send_json(body)
            .map_err(map_ureq)?;
        let value: Value = resp.into_json().map_err(|e| LlmError::Protocol(e.to_string()))?;
        let content = value["choices"][0]["message"]["content"]
            .as_str()
            .ok_or_else(|| LlmError::Protocol("missing choices[0].message.content".into()))?
            .to_string();
        let usage = Usage {
            input_tokens: value["usage"]["prompt_tokens"].as_u64().unwrap_or(0),
            output_tokens: value["usage"]["completion_tokens"].as_u64().unwrap_or(0),
        };
        Ok(ChatResponse { content, usage })
    }
}

/// Anthropic native `/v1/messages`.
pub struct AnthropicClient {
    pub api_key: String,
    pub model: String,
    pub base_url: String,
    pub timeout: Duration,
}

impl LlmClient for AnthropicClient {
    fn model(&self) -> &str {
        &self.model
    }

    fn complete(&self, req: &ChatRequest) -> Result<ChatResponse, LlmError> {
        let url = format!("{}/v1/messages", self.base_url.trim_end_matches('/'));
        let body = json!({
            "model": self.model,
            "max_tokens": 8192,
            "system": req.system,
            "messages": req.messages.iter().map(|m| json!({
                "role": m.role,
                "content": m.content,
            })).collect::<Vec<_>>(),
        });
        let agent = ureq::AgentBuilder::new()
            .timeout(self.timeout)
            .build();
        let resp = agent
            .post(&url)
            .set("x-api-key", &self.api_key)
            .set("anthropic-version", "2023-06-01")
            .send_json(body)
            .map_err(map_ureq)?;
        let value: Value = resp.into_json().map_err(|e| LlmError::Protocol(e.to_string()))?;
        // Concatenate all text blocks (Anthropic returns a list).
        let mut content = String::new();
        if let Some(blocks) = value["content"].as_array() {
            for b in blocks {
                if b["type"] == "text" {
                    if let Some(t) = b["text"].as_str() {
                        content.push_str(t);
                    }
                }
            }
        }
        if content.is_empty() {
            return Err(LlmError::Protocol("empty Anthropic content".into()));
        }
        let usage = Usage {
            input_tokens: value["usage"]["input_tokens"].as_u64().unwrap_or(0),
            output_tokens: value["usage"]["output_tokens"].as_u64().unwrap_or(0),
        };
        Ok(ChatResponse { content, usage })
    }
}

fn map_ureq(e: ureq::Error) -> LlmError {
    match e {
        ureq::Error::Status(code, resp) => {
            let body = resp.into_string().unwrap_or_default();
            LlmError::Http(format!("HTTP {code}: {}", truncate(&body, 400)))
        }
        other => {
            let s = other.to_string();
            if s.contains("timed out") || s.contains("deadline") {
                LlmError::Timeout(s)
            } else {
                LlmError::Http(s)
            }
        }
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.len() <= n {
        s.to_string()
    } else {
        format!("{}…", &s[..n])
    }
}
