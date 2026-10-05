//! Runtime configuration, loaded from environment variables (`.env` support
//! and the config file arrive with the configuration ticket).

/// Provider kinds aider-rs can talk to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Provider {
    Anthropic,
    OpenAiCompatible,
}

/// Minimal environment-derived configuration (extended in the configuration
/// ticket with `.env` and config-file loading).
#[derive(Debug, Clone, Default)]
pub struct Config {
    pub provider: Option<Provider>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
}

impl Config {
    /// Read the basic provider settings from environment variables.
    pub fn from_env() -> Self {
        let api_key = std::env::var("ANTHROPIC_API_KEY")
            .ok()
            .filter(|k| !k.is_empty())
            .map(|k| (Provider::Anthropic, k))
            .or_else(|| {
                std::env::var("OPENAI_API_KEY")
                    .ok()
                    .filter(|k| !k.is_empty())
                    .map(|k| (Provider::OpenAiCompatible, k))
            });
        let (provider, api_key) = match api_key {
            Some((p, k)) => (Some(p), Some(k)),
            None => (None, None),
        };
        Self {
            provider,
            model: std::env::var("AIDER_RS_MODEL").ok().filter(|m| !m.is_empty()),
            api_key,
            base_url: std::env::var("OPENAI_API_BASE")
                .ok()
                .filter(|u| !u.is_empty()),
        }
    }
}