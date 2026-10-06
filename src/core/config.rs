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
    pub task_timeout_secs: Option<u64>,
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
    task_timeout_secs: Option<u64>,
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
            task_timeout_secs,
        } = self;
        // Files are merged home → repo, so a later (higher-precedence) file
        // overwrites whatever an earlier file or the defaults provided.
        if let Some(p) = provider {
            if let Some(parsed) = parse_provider(&p) {
                cfg.provider = Some(parsed);
            }
        }
        if model.is_some() {
            cfg.model = model;
        }
        if api_key.is_some() {
            cfg.api_key = api_key;
        }
        if base_url.is_some() {
            cfg.base_url = base_url;
        }
        if timeout_secs.is_some() {
            cfg.timeout_secs = timeout_secs;
        }
        if max_edit_retries.is_some() {
            cfg.max_edit_retries = max_edit_retries;
        }
        if task_timeout_secs.is_some() {
            cfg.task_timeout_secs = task_timeout_secs;
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
    // Environment is the highest-precedence layer: a present variable
    // overrides whatever the config files provided.
    if let Some(p) = get("AIDER_RS_PROVIDER").as_deref().and_then(parse_provider) {
        cfg.provider = Some(p);
    }
    if let Some(m) = get("AIDER_RS_MODEL") {
        cfg.model = Some(m);
    }
    if let Some(k) = get("ANTHROPIC_API_KEY").or_else(|| get("OPENAI_API_KEY")) {
        cfg.api_key = Some(k);
    }
    if let Some(b) = get("OPENAI_API_BASE").or_else(|| get("ANTHROPIC_BASE_URL")) {
        cfg.base_url = Some(b);
    }
    if let Some(t) = get("AIDER_RS_TIMEOUT_SECS").and_then(|v| v.parse().ok()) {
        cfg.timeout_secs = Some(t);
    }
    if let Some(r) = get("AIDER_RS_MAX_EDIT_RETRIES").and_then(|v| v.parse().ok()) {
        cfg.max_edit_retries = Some(r);
    }
    if let Some(t) = get("AIDER_RS_TASK_TIMEOUT_SECS").and_then(|v| v.parse().ok()) {
        cfg.task_timeout_secs = Some(t);
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
    fn config_precedence_higher_layers_win() {
        // Mirrors load(): defaults → home file → repo file → env.
        let mut cfg = Config::default();
        let home: FileConfig =
            serde_json::from_str(r#"{"model": "from-home", "timeout_secs": 10}"#).unwrap();
        home.merge_into(&mut cfg);
        let repo: FileConfig = serde_json::from_str(
            r#"{"model": "from-repo", "provider": "anthropic", "timeout_secs": 30}"#,
        )
        .unwrap();
        repo.merge_into(&mut cfg);
        assert_eq!(cfg.model.as_deref(), Some("from-repo"), "repo file beats home file");
        assert_eq!(cfg.provider, Some(Provider::Anthropic));
        assert_eq!(cfg.timeout_secs, Some(30), "repo value replaces home value");
        let get = |k: &str| if k == "AIDER_RS_MODEL" { Some("from-env".into()) } else { None };
        apply_env(&mut cfg, &get);
        assert_eq!(cfg.model.as_deref(), Some("from-env"), "env beats files");
        assert_eq!(cfg.timeout_secs, Some(30), "env absent keeps file value");
    }

    #[test]
    fn invalid_file_config_is_rejected_not_panic() {
        let f: Result<FileConfig, _> = serde_json::from_str("{ not json");
        assert!(f.is_err());
    }
}