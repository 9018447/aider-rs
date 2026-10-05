//! Task orchestration: run one aider_task end to end — build the prompt,
//! call the LLM, parse edits, retry once on failed matches, apply atomically,
//! auto-commit, and report the outcome.

use serde_json::json;

use crate::core::config::Config;
use crate::core::editformat::{self, Edit};
use crate::core::git::Git;
use crate::core::llm::{self, ChatRequest};
use crate::core::prompts;
use crate::core::session::Session;

/// What one aider_task call produced, for the MCP tool response.
#[derive(Debug, Clone)]
pub struct TaskOutcome {
    pub ok: bool,
    pub files: Vec<String>,
    pub commit: Option<String>,
    pub diff: String,
    pub summary: String,
    pub usage: (u64, u64),
    pub shells: Vec<String>,
}

#[derive(Debug)]
pub enum TaskError {
    NotInGitRepo,
    Llm(llm::LlmError),
    AllEditsFailed(String),
    Git(String),
}

impl std::fmt::Display for TaskError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            TaskError::NotInGitRepo => write!(f, "aider-rs must run inside a git repository (auto-commit and undo depend on it)"),
            TaskError::Llm(e) => write!(f, "{e}"),
            TaskError::AllEditsFailed(d) => write!(f, "edits failed to apply; nothing was written:\n{d}"),
            TaskError::Git(g) => write!(f, "git error: {g}"),
        }
    }
}

/// Arguments to aider_task.
pub struct TaskArgs {
    pub task: String,
    pub files: Vec<String>,
    pub reset_context: bool,
}

/// Run one coding task. `session` carries this Claude Code session's context.
pub fn run_task(
    session: &mut Session,
    config: &Config,
    args: TaskArgs,
) -> Result<TaskOutcome, TaskError> {
    let cwd = std::env::current_dir().map_err(|e| TaskError::Git(e.to_string()))?;
    if !Git::is_repo(&cwd) {
        return Err(TaskError::NotInGitRepo);
    }
    let git = Git::open(&cwd).map_err(|e| TaskError::Git(e.to_string()))?;

    if args.reset_context {
        session.reset_context();
    }

    let client = llm::client_from_config(config).map_err(TaskError::Llm)?;
    session.model = client.model().to_string();

    // Read context files (bounded) relative to the repo root.
    let mut context_files: Vec<(String, String)> = Vec::new();
    for f in &args.files {
        let path = git.root.join(f);
        match std::fs::read_to_string(&path) {
            Ok(content) => context_files.push((f.clone(), content)),
            Err(e) => {
                return Err(TaskError::Llm(llm::LlmError::Protocol(format!(
                    "cannot read context file {f}: {e}"
                ))))
            }
        }
    }

    let user_msg = prompts::task_user_message(&args.task, &context_files);
    session.push_user(&user_msg);

    // First attempt, plus one retry round when blocks fail to match.
    let max_attempts = config.max_edit_retries.unwrap_or(1) + 1;
    let mut last_usage = (0u64, 0u64);
    let mut shells: Vec<String> = Vec::new();
    let mut edits: Vec<Edit> = Vec::new();
    let mut assistant_reply = String::new();

    for attempt in 0..max_attempts {
        let req = ChatRequest {
            system: prompts::SYSTEM_PROMPT.to_string(),
            messages: session.history.clone(),
        };
        let resp = client.complete(&req).map_err(TaskError::Llm)?;
        last_usage = (resp.usage.input_tokens, resp.usage.output_tokens);
        session.total_input_tokens += resp.usage.input_tokens;
        session.total_output_tokens += resp.usage.output_tokens;
        assistant_reply = resp.content;

        let chat_files: Vec<String> = context_files.iter().map(|(p, _)| p.clone()).collect();
        let (parsed, parsed_shells) = editformat::parse_edits(&assistant_reply, &chat_files);
        shells = parsed_shells.iter().map(|s| s.0.clone()).collect();

        // Dry-run apply against current file contents.
        match editformat::apply_edits(&parsed) {
            Ok(_) => {
                edits = parsed;
                break;
            }
            Err(failed) => {
                if attempt + 1 >= max_attempts {
                    session.tasks_run += 1;
                    session.tasks_failed += 1;
                    session.push_assistant(&assistant_reply);
                    return Err(TaskError::AllEditsFailed(
                        editformat::format_failed_blocks(&failed),
                    ));
                }
                // Feed the failure back to the LLM for one retry round.
                session.push_assistant(&assistant_reply);
                session.push_user(&editformat::format_failed_blocks(&failed));
            }
        }
    }

    if edits.is_empty() {
        session.tasks_run += 1;
        session.push_assistant(&assistant_reply);
        // No edits: treat as a conversational reply (question or refusal).
        let summary = first_meaningful_lines(&assistant_reply, 20);
        return Ok(TaskOutcome {
            ok: true,
            files: vec![],
            commit: None,
            diff: String::new(),
            summary,
            usage: last_usage,
            shells,
        });
    }

    // Apply for real (guaranteed to succeed: same inputs just succeeded dry).
    let staged = editformat::apply_edits(&edits).expect("dry run already validated these edits");
    let mut written: Vec<String> = Vec::new();
    for (path, content) in &staged {
        // Ensure parent directories exist for new files.
        if let Some(parent) = std::path::Path::new(path).parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(path, content).map_err(|e| TaskError::Git(e.to_string()))?;
        written.push(path.clone());
    }

    // Auto-commit: one commit per task round, aider-style message prefix.
    let prev_head = git.head_hash().map_err(|e| TaskError::Git(e.to_string()))?;
    let commit_msg = commit_message(&args.task, &written);
    let path_refs: Vec<&str> = written.iter().map(|s| s.as_str()).collect();
    let new_head = git
        .commit_paths(&path_refs, &commit_msg)
        .map_err(|e| TaskError::Git(e.to_string()))?;
    session.commits.push(new_head.clone());
    session.tasks_run += 1;

    let diff = git
        .diff_refs(&prev_head, &new_head)
        .unwrap_or_default();

    session.push_assistant(&format!(
        "Applied {} edit(s) to {} file(s), committed as {}.\n\n{}",
        edits.len(),
        written.len(),
        &new_head[..7.min(new_head.len())],
        assistant_reply
    ));


    Ok(TaskOutcome {
        ok: true,
        files: written,
        commit: Some(new_head),
        diff,
        summary: first_meaningful_lines(&assistant_reply, 20),
        usage: last_usage,
        shells,
    })
}

/// Undo the session's last commit. Refuses if HEAD moved on.
pub fn undo(session: &mut Session) -> Result<String, String> {
    let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
    let git = Git::open(&cwd).map_err(|e| e.to_string())?;
    let last = session
        .commits
        .last()
        .ok_or_else(|| "nothing to undo: this session has not committed anything yet".to_string())?
        .clone();
    let undone = git.reset_last_commit(&last).map_err(|e| e.to_string())?;
    session.commits.pop();
    Ok(format!(
        "Undone commit {} (aider-rs edit round rewound); HEAD is now {}.",
        &last[..7.min(last.len())],
        &undone[..7.min(undone.len())]
    ))
}

/// Session + git status report for aider_status.
pub fn status(session: &Session, config: &Config) -> serde_json::Value {
    let cwd = std::env::current_dir().unwrap_or_default();
    let repo = if Git::is_repo(&cwd) {
        Git::open(&cwd).ok()
    } else {
        None
    };
    let mut git_info = json!({ "in_repo": repo.is_some() });
    if let Some(git) = repo {
        let (staged, unstaged, untracked) = git.status_counts().unwrap_or((0, 0, 0));
        git_info = json!({
            "in_repo": true,
            "branch": git.branch().unwrap_or_default(),
            "head": git.head_hash().ok().map(|h| h[..12.min(h.len())].to_string()),
            "staged_files": staged,
            "unstaged_files": unstaged,
            "untracked_files": untracked,
            "recent_commits": git.log(5).unwrap_or_default(),
        });
    }
    json!({
        "session": {
            "model": if session.model.is_empty() {
                config.model.clone().unwrap_or_else(|| "not configured".into())
            } else {
                session.model.clone()
            },
            "tasks_run": session.tasks_run,
            "tasks_failed": session.tasks_failed,
            "context_messages": session.history.len(),
            "commits_this_session": session.commits.len(),
            "total_input_tokens": session.total_input_tokens,
            "total_output_tokens": session.total_output_tokens,
        },
        "git": git_info,
    })
}

fn commit_message(task: &str, files: &[String]) -> String {
    let first = task.lines().find(|l| !l.trim().is_empty()).unwrap_or("task");
    let short: String = first.chars().take(72).collect();
    format!("aider-rs: {short} ({})", files.join(", "))
}

fn first_meaningful_lines(s: &str, n: usize) -> String {
    let lines: Vec<&str> = s
        .lines()
        .skip_while(|l| l.trim().is_empty())
        .take(n)
        .collect();
    lines.join("\n")
}