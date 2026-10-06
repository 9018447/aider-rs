//! Prompts for the diff (SEARCH/REPLACE) edit format, adapted from aider's
//! `editblock_prompts.py` (Apache-2.0). aider-rs deviations: shell commands
//! are never executed (they are reported back instead), and new files may be
//! created freely (there is no chat-add workflow to ask about).

use std::path::Path;

pub const SYSTEM_PROMPT: &str = r#"Act as an expert software developer.
Always use best practices when coding.
Respect and use existing conventions, libraries, etc that are already present in the code base.
Take requests for changes to the supplied code.
If the request is ambiguous, ask questions.

Once you understand the request you MUST:

1. Think step-by-step and explain the needed changes in a few short sentences.

2. Describe each change with a *SEARCH/REPLACE block* per the examples below.

All changes to files must use this *SEARCH/REPLACE block* format.
ONLY EVER RETURN CODE IN A *SEARCH/REPLACE block*!

For every block, put the file path alone on the line before the opening fence.

You may create new files with an empty SEARCH section, and append to files the same way.

# *SEARCH/REPLACE block* Rules:

1. **Keep each block minimal and focused.** Small, targeted blocks are more likely to match exactly.

2. **The SEARCH section must match the file EXACTLY**: characters, whitespace, indentation, docstrings, comments — everything.

3. **The SEARCH section must be unique enough to match only ONE spot in the file** — include surrounding lines if needed.

4. To move code within a file, use two blocks: one to delete it from the old location, one to add it at the new location.

5. To make a new file, use an empty SEARCH section:

path/to/new_file.py
```
<<<<<<< SEARCH
=======
def hello():
    print("hello")
>>>>>>> REPLACE
```

6. To change code, include both the old and new lines:

mathweb/flask/app.py
```
<<<<<<< SEARCH
from flask import Flask
=======
import math
from flask import Flask
>>>>>>> REPLACE
```

7. Delete code by leaving the REPLACE section empty.

8. Include immediate closing and opening fences on their own lines, with the file path between them.

9. If you need to elide unchanged lines, use a line containing only `...` in BOTH the SEARCH and REPLACE sections, in the same position.

10. Do NOT run shell commands. Reply with edits only.
"#;

/// The effective system prompt: the built-in SEARCH/REPLACE prompt with a
/// non-empty `~/.config/aider-rs/AGENTS.md` appended as user-customizable
/// additional instructions.
pub fn system_prompt() -> String {
    let mut prompt = SYSTEM_PROMPT.to_string();
    if let Some(home) = std::env::var_os("HOME") {
        let p = std::path::PathBuf::from(home)
            .join(".config")
            .join("aider-rs")
            .join("AGENTS.md");
        if let Some(custom) = system_prompt_from(&p) {
            prompt.push_str("\n\n# Additional instructions\n\n");
            prompt.push_str(&custom);
        }
    }
    prompt
}

fn system_prompt_from(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn custom_agents_md_is_appended() {
        let dir = std::env::temp_dir().join(format!("aider-rs-prompts-{}-a", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("AGENTS.md");
        std::fs::write(&p, "  \ncustom prompt for test\n").unwrap();
        let prompt = system_prompt_from(&p).unwrap();
        assert_eq!(prompt, "custom prompt for test");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn missing_or_empty_file_falls_back_to_default() {
        assert_eq!(system_prompt_from(Path::new("/nonexistent/AGENTS.md")), None);
        let dir = std::env::temp_dir().join(format!("aider-rs-prompts-{}-b", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let p = dir.join("AGENTS.md");
        std::fs::write(&p, "   \n").unwrap();
        assert_eq!(system_prompt_from(&p), None);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}

/// Build the user turn for a task, including the requested context files.
pub fn task_user_message(task: &str, files: &[(String, String)]) -> String {
    let mut msg = String::new();
    msg.push_str(task);
    msg.push_str("\n\n");
    if files.is_empty() {
        msg.push_str(
            "(No files were attached to this task. If you need to edit a file that exists in the repository, output its SEARCH/REPLACE block with its path relative to the repository root. If you are unsure of the exact current contents, say so and ask instead of guessing.)",
        );
    } else {
        msg.push_str("Files attached to this task (paths are relative to the repository root):\n\n");
        for (path, content) in files {
            msg.push_str(&format!("{path}\n```\n{content}\n```\n\n"));
        }
    }
    msg
}