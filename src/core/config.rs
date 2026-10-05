//! Runtime configuration.
//!
//! Precedence (highest first):
//! 1. process environment variables
//! 2. `.env` in the working directory
//! 3. `.aider-rs.json` in the repository root
//! 4. `~/.config/aider-rs/config.json`
//!
//! Recognized environment keys: `ANTHROPIC_API_KEY`, `OPENAI_API_KEY`,
//! `OPENAI_API_BASE` (also `ANTHROPIC_BASE_URL`), `AIDER_RS_MODEL`,
//! `AIDER_RS_PROVIDER` (`anthropic` | `openai`), `AIDER_RS_TIMEOUT_SECS`,
//! `AIDER_RS_MAX_EDIT_RETRIES`.

use std::path::PathBuf;

use serde::Deserialize;

/// Provider kinds aider-rs can talk to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Provider {
    Anthropic,
    OpenAiCompatible,
}

/// Fully-resolved runtime configuration.
#[derive(Debug, Clone, Default)]
pub struct Config {
    pub provider: Option<Provider>,
    pub model: Option<String>,
    pub api_key: Option<String>,
    pub base_url: Option<String>,
    pub timeout_secs: Option<u64>,
    pub max_edit_retries: Option<u32>,
}

/// File-backed portion of the config (JSON, snake_case).
#[derive(Debug, Default, Deserialize)]
struct FileConfig {
    provider: Option<String>,
    model: Option<String>,
    api_key: Option<String>,
    base_url: Option<String>,
    timeout_secs: Option<u64>,
    max_edit_retries: Option<u32>,
}

impl FileConfig {
    fn load(path: &PathBuf) -> Option<Self> {
        let text = std::fs::read_to_string(path).ok()?;
        match serde_json::from_str(&text) {
            Ok(cfg) => Some(cfg),
            Err(e) => {
                eprintln!("aider-rs: ignoring invalid config file {}: {e}", path.display());
                None
            }
        }
    }

    fn merge_into(self, cfg: &mut Config) {
        let Self {
            provider,
            model,
            api_key,
            base_url,
            timeout_secs,
            max_edit_retries,
        } = self;
        if cfg.provider.is_none() {
            cfg.provider = provider.as_deref().and_then(parse_provider);
        }
        if cfg.model.is_none() {
            cfg.model = model;
        }
        if cfg.api_key.is_none() {
            cfg.api_key = api_key;
        }
        if cfg.base_url.is_none() {
            cfg.base_url = base_url;
        }
        if cfg.timeout_secs.is_none() {
            cfg.timeout_secs = timeout_secs;
        }
        if cfg.max_edit_retries.is_none() {
            cfg.max_edit_retries = max_edit_retries;
        }
    }
}

fn parse_provider(s: &str) -> Option<Provider> {
    match s.trim().to_ascii_lowercase().as_str() {
        "anthropic" => Some(Provider::Anthropic),
        "openai" | "openai-compatible" | "openai_compatible" => Some(Provider::OpenAiCompatible),
        _ => None,
    }
}

/// Load the effective configuration for the current working directory.
pub fn load() -> Config {
    let mut cfg = Config::default();

    // 4. home config (lowest precedence)
    if let Some(home) = std::env::var_os("HOME") {
        let p = PathBuf::from(&home)
            .join(".config")
            .join("aider-rs")
            .join("config.json");
        if let Some(f) = FileConfig::load(&p) {
            f.merge_into(&mut cfg);
        }
    }

    // 3. repo config
    if let Ok(cwd) = std::env::current_dir() {
        let p = cwd.join(".aider-rs.json");
        if let Some(f) = FileConfig::load(&p) {
            f.merge_into(&mut cfg);
        }
    }

    // 2. .env in the working directory
    if let Ok(env_map) = load_dotenv(cwd_dotenv_path().as_deref()) {
        let get = |k: &str| -> Option<String> {
            std::env::var(k).ok().or_else(|| env_map.get(k).cloned()).filter(|v| !v.is_empty())
        };
        apply_env(&mut cfg, &get);
    } else {
        let get = |k: &str| -> Option<String> {
            std::env::var(k).ok().filter(|v| !v.is_empty())
        };
        apply_env(&mut cfg, &get);
    }

    cfg
}

fn cwd_dotenv_path() -> Option<PathBuf> {
    std::env::current_dir().ok().map(|d| d.join(".env"))
}

/// Parse a `.env` file: KEY=VALUE lines, `#` comments, optional quotes.
/// Malformed lines are ignored (never fail the whole load).
pub fn parse_dotenv(text: &str) -> std::collections::HashMap<String, String> {
    let mut map = std::collections::HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim().to_string();
        if key.is_empty() {
            continue;
        }
        let mut value = value.trim().to_string();
        if (value.starts_with('"') && value.ends_with('"') && value.len() >= 2)
            || (value.starts_with('\'') && value.ends_with('\'') && value.len() >= 2)
        {
            value = value[1..value.len() - 1].to_string();
        }
        map.insert(key, value);
    }
    map
}

fn load_dotenv(path: Option<&std::path::Path>) -> Result<std::collections::HashMap<String, String>, ()> {
    let path = path.ok_or(())?;
    let text = std::fs::read_to_string(path).map_err(|_| ())?;
    Ok(parse_dotenv(&text))
}

fn apply_env(cfg: &mut Config, get: &impl Fn(&str) -> Option<String>) {
    if cfg.provider.is_none() {
        if let Some(p) = get("AIDER_RS_PROVIDER").as_deref().and_then(parse_provider) {
            cfg.provider = Some(p);
        }
    }
    if cfg.model.is_none() {
        cfg.model = get("AIDER_RS_MODEL");
    }
    if cfg.api_key.is_none() {
        cfg.api_key = get("ANTHROPIC_API_KEY").or_else(|| get("OPENAI_API_KEY"));
    }
    if cfg.base_url.is_none() {
        cfg.base_url = get("OPENAI_API_BASE").or_else(|| get("ANTHROPIC_BASE_URL"));
    }
    if cfg.timeout_secs.is_none() {
        if let Some(t) = get("AIDER_RS_TIMEOUT_SECS").and_then(|v| v.parse().ok()) {
            cfg.timeout_secs = Some(t);
        }
    }
    if cfg.max_edit_retries.is_none() {
        if let Some(r) = get("AIDER_RS_MAX_EDIT_RETRIES").and_then(|v| v.parse().ok()) {
            cfg.max_edit_retries = Some(r);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dotenv_parses_comments_quotes_and_blanks() {
        let text = "# comment\n\nANTHROPIC_API_KEY=\"sk-quoted\"\nOPENAI_API_BASE=http://127.0.0.1:9/v1\nBADLINE\nEMPTY=\n";
        let map = parse_dotenv(text);
        assert_eq!(map.get("ANTHROPIC_API_KEY").unwrap(), "sk-quoted");
        assert_eq!(map.get("OPENAI_API_BASE").unwrap(), "http://127.0.0.1:9/v1");
        assert!(!map.contains_key("BADLINE"));
        assert_eq!(map.get("EMPTY").map(String::as_str), Some(""), "EMPTY= sets an empty value");
    }

    #[test]
    fn provider_strings_parse() {
        assert_eq!(parse_provider("anthropic"), Some(Provider::Anthropic));
        assert_eq!(parse_provider(" OpenAI "), Some(Provider::OpenAiCompatible));
        assert_eq!(parse_provider("nope"), None);
    }

    #[test]
    fn file_config_merges_without_overwriting() {
        let mut cfg = Config {
            model: Some("from-env".into()),
            ..Default::default()
        };
        let f: FileConfig = serde_json::from_str(
            r#"{"model": "from-file", "provider": "anthropic", "timeout_secs": 30}"#,
        )
        .unwrap();
        f.merge_into(&mut cfg);
        assert_eq!(cfg.model.as_deref(), Some("from-env"), "already-set fields win");
        assert_eq!(cfg.provider, Some(Provider::Anthropic));
        assert_eq!(cfg.timeout_secs, Some(30));
    }

    #[test]
    fn invalid_file_config_is_rejected_not_panic() {
        let f: Result<FileConfig, _> = serde_json::from_str("{ not json");
        assert!(f.is_err());
    }
}