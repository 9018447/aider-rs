//! SEARCH/REPLACE edit format, ported from aider's `editblock_coder.py`.
//!
//! A block looks like:
//!
//! ```text
//! path/to/file.rs
//! <<<<<<< SEARCH
//! ...exact existing lines...
//! =======
//! ...new lines...
//! >>>>>>> REPLACE
//! ```
//!
//! Matching rules (aligned with aider):
//! 1. exact line-sequence match, first occurrence wins;
//! 2. leading-whitespace-flexible match with uniform re-indent;
//! 3. a spurious leading blank SEARCH line is dropped and 1–2 retried;
//! 4. `...` elision pieces must pair up and match uniquely.
//!
//! Empty SEARCH + existing file appends; empty SEARCH + new file creates it.

use std::collections::HashMap;
use std::path::Path;

const FENCE: &str = "```";
const DOTS: &str = "...";

/// One parsed edit: replace `search` with `replace` in `path`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edit {
    pub path: String,
    pub search: String,
    pub replace: String,
}

/// Shell command the LLM asked to run (parsed but NOT executed by aider-rs;
/// returned to the caller for a decision — safety by default).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellCommand(pub String);

/// Marker-line classifiers, replacing aider's regexes
/// (`^<{5,9} SEARCH>?\s*$`, `^={5,9}\s*$`, `^>{5,9} REPLACE\s*$`).
fn is_head(line: &str) -> bool {
    let s = line.trim_end();
    let n = s.chars().take_while(|&c| c == '<').count();
    if !(5..=9).contains(&n) {
        return false;
    }
    let rest = &s[n..];
    rest == " SEARCH" || rest == " SEARCH>"
}

fn is_divider(line: &str) -> bool {
    let s = line.trim_end();
    let n = s.chars().take_while(|&c| c == '=').count();
    (5..=9).contains(&n) && s[n..].trim().is_empty()
}

fn is_updated(line: &str) -> bool {
    let s = line.trim_end();
    let n = s.chars().take_while(|&c| c == '>').count();
    if !(5..=9).contains(&n) {
        return false;
    }
    s[n..].trim() == "REPLACE"
}

/// Split content into lines, keeping the line terminators (aider's
/// `splitlines(keepends=True)`).
fn keepends(content: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut cur = String::new();
    for ch in content.chars() {
        cur.push(ch);
        if ch == '\n' {
            lines.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        lines.push(cur);
    }
    lines
}

fn ensure_trailing_newline(s: &str) -> String {
    if s.is_empty() || s.ends_with('\n') {
        s.to_string()
    } else {
        format!("{s}\n")
    }
}

/// aider's `strip_filename`: clean a candidate filename line.
fn strip_filename(line: &str) -> Option<String> {
    let filename = line.trim();
    if filename == DOTS {
        return None;
    }
    for prefix in [FENCE, "```python", "```js"] {
        if let Some(candidate) = filename.strip_prefix(prefix) {
            let candidate = candidate.trim();
            if !candidate.is_empty() && (candidate.contains('.') || candidate.contains('/')) {
                return Some(candidate.to_string());
            }
            return None;
        }
    }
    let mut f = filename.trim_end_matches(':').to_string();
    f = f.trim_start_matches('#').to_string();
    f = f.trim().to_string();
    f = f.trim_matches('`').to_string();
    f = f.trim_matches('*').to_string();
    f = f.trim().to_string();
    if f.is_empty() {
        None
    } else {
        Some(f)
    }
}

/// aider's `find_filename`: look back up to 3 preceding lines, only through
/// fence-looking lines; pick the best candidate against known chat files.
fn find_filename(preceding: &[String], valid: &[String]) -> Option<String> {
    let mut filenames = Vec::new();
    for line in preceding.iter().rev().take(3) {
        let t = line.trim();
        if is_head(t) || is_divider(t) || is_updated(t) {
            // Block structure (a previous block's markers): stop looking;
            // these are never filenames.
            break;
        }
        if t.starts_with(FENCE) {
            // Fence line: it may carry a filename ("```python foo.py"),
            // otherwise keep looking back through it.
            if let Some(f) = strip_filename(line) {
                filenames.push(f);
            }
            continue;
        }
        if let Some(f) = strip_filename(line) {
            filenames.push(f);
        }
        break;
    }
    if filenames.is_empty() {
        return None;
    }
    for f in &filenames {
        if valid.contains(f) {
            return Some(f.clone());
        }
    }
    for f in &filenames {
        let base = Path::new(f)
            .file_name()
            .map(|b| b.to_string_lossy().to_string());
        if let Some(base) = base {
            if valid.iter().any(|v| Path::new(v).file_name().map(|b| b == base.as_str()).unwrap_or(false)) {
                for v in valid {
                    if Path::new(v).file_name().map(|b| b == base.as_str()).unwrap_or(false) {
                        return Some(v.clone());
                    }
                }
            }
        }
    }
    for f in &filenames {
        if f.contains('.') {
            return Some(f.clone());
        }
    }
    filenames.first().cloned()
}

/// Parse LLM output into edits (and any shell commands it asked to run).
/// Ported from aider's `find_original_update_blocks`.
pub fn parse_edits(content: &str, chat_files: &[String]) -> (Vec<Edit>, Vec<ShellCommand>) {
    let lines = keepends(content);
    let mut edits = Vec::new();
    let mut shells = Vec::new();
    let mut current_filename: Option<String> = None;
    let mut i = 0usize;

    let shell_starts = [
        "```bash", "```sh", "```shell", "```cmd", "```batch", "```powershell", "```ps1", "```zsh",
        "```fish", "```ksh", "```csh", "```tcsh",
    ];

    while i < lines.len() {
        let line = &lines[i];

        let next_is_editblock = (i + 1 < lines.len() && is_head(lines[i + 1].trim()))
            || (i + 2 < lines.len() && is_head(lines[i + 2].trim()));

        if shell_starts.iter().any(|s| line.trim().starts_with(s)) && !next_is_editblock {
            let mut shell_content = String::new();
            i += 1;
            while i < lines.len() && !lines[i].trim().starts_with("```") {
                shell_content.push_str(&lines[i]);
                i += 1;
            }
            if i < lines.len() {
                i += 1; // closing fence
            }
            if !shell_content.trim().is_empty() {
                shells.push(ShellCommand(shell_content));
            }
            continue;
        }

        if is_head(line.trim()) {
            // New-file blocks (HEAD directly followed by DIVIDER) may use a
            // filename that isn't a chat file yet.
            let new_file_block = i + 1 < lines.len() && is_divider(lines[i + 1].trim());
            let start = i.saturating_sub(3);
            let preceding = &lines[start..i];
            let mut filename = if new_file_block {
                find_filename(preceding, &[])
            } else {
                find_filename(preceding, chat_files)
            }
            .or_else(|| current_filename.clone());

            if filename.is_none() {
                // Skip the malformed block; aider raises here, we record it by
                // skipping (the surrounding failure reporting handles the rest).
                i += 1;
                continue;
            }
            current_filename = filename.clone();

            let mut search = String::new();
            i += 1;
            while i < lines.len() && !is_divider(lines[i].trim()) {
                search.push_str(&lines[i]);
                i += 1;
            }
            if i >= lines.len() {
                break; // unterminated block
            }
            i += 1; // past divider

            let mut replace = String::new();
            while i < lines.len() && !is_updated(lines[i].trim()) && !is_divider(lines[i].trim()) {
                replace.push_str(&lines[i]);
                i += 1;
            }
            if i >= lines.len() {
                break; // unterminated block
            }
            i += 1; // past REPLACE (or divider; rare aider quirk)

            edits.push(Edit {
                path: filename.take().unwrap_or_default(),
                search,
                replace,
            });
            continue;
        }

        i += 1;
    }

    (edits, shells)
}

/// aider's `strip_quoted_wrapping`: drop a filename line and/or code fences
/// wrapping the SEARCH/REPLACE text.
fn strip_quoted_wrapping(res: &str, fname: Option<&str>) -> String {
    if res.is_empty() {
        return res.to_string();
    }
    let mut lines = keepends(res);
    if let Some(fname) = fname {
        if let Some(first) = lines.first() {
            let base = Path::new(fname)
                .file_name()
                .map(|b| b.to_string_lossy().to_string())
                .unwrap_or_default();
            if first.trim().ends_with(&base) {
                lines.remove(0);
            }
        }
    }
    if lines.len() >= 2
        && lines.first().map(|l| l.starts_with(FENCE)).unwrap_or(false)
        && lines.last().map(|l| l.starts_with(FENCE)).unwrap_or(false)
    {
        lines.remove(lines.len() - 1);
        lines.remove(0);
    }
    let joined = lines.join("");
    ensure_trailing_newline(&joined)
}

/// Result of applying one edit's search text against a file's content.
enum ReplaceOutcome {
    Applied(String),
    Failed,
}

/// aider's `perfect_replace`: exact line-sequence match, first occurrence.
fn perfect_replace(whole_lines: &[String], part_lines: &[String], replace_lines: &[String]) -> ReplaceOutcome {
    let n = part_lines.len();
    if n == 0 || whole_lines.len() < n {
        return ReplaceOutcome::Failed;
    }
    for i in 0..=(whole_lines.len() - n) {
        if whole_lines[i..i + n] == *part_lines {
            let mut out = whole_lines[..i].to_vec();
            out.extend_from_slice(replace_lines);
            out.extend_from_slice(&whole_lines[i + n..]);
            return ReplaceOutcome::Applied(out.join(""));
        }
    }
    ReplaceOutcome::Failed
}

/// aider's `replace_part_with_missing_leading_whitespace`: match a window
/// that agrees ignoring leading whitespace, with one uniform indent delta.
fn whitespace_flexible_replace(
    whole_lines: &[String],
    part_lines: &[String],
    replace_lines: &[String],
) -> ReplaceOutcome {
    let n = part_lines.len();
    if n == 0 || whole_lines.len() < n {
        return ReplaceOutcome::Failed;
    }

    // Uniform outdent of part/replace by the minimum leading whitespace.
    let leading: Vec<usize> = part_lines
        .iter()
        .chain(replace_lines.iter())
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.len() - l.trim_start().len())
        .collect();
    let min_leading = leading.iter().copied().min().unwrap_or(0);

    let outdent = |lines: &[String]| -> Vec<String> {
        lines
            .iter()
            .map(|l| {
                if l.trim().is_empty() || min_leading == 0 {
                    l.clone()
                } else {
                    let skip = l.char_indices().nth(min_leading).map(|(i, _)| i).unwrap_or(l.len());
                    l[skip..].to_string()
                }
            })
            .collect()
    };
    let part = outdent(part_lines);
    let replace = outdent(replace_lines);

    for i in 0..=(whole_lines.len() - n) {
        let window = &whole_lines[i..i + n];
        // Non-whitespace content must agree line by line.
        let agrees = window
            .iter()
            .zip(part.iter())
            .all(|(w, p)| w.trim_start() == p.trim_start());
        if !agrees {
            continue;
        }
        // The indent delta must be uniform across non-blank lines.
        let deltas: Vec<&str> = window
            .iter()
            .zip(part.iter())
            .filter(|(w, _)| !w.trim().is_empty())
            .map(|(w, p)| &w[..w.len() - p.len().min(w.len())])
            .collect();
        let uniform = deltas.windows(2).all(|d| d[0] == d[1]) && !deltas.is_empty();
        if !uniform {
            continue;
        }
        let add = deltas[0];
        let mut new_replace = Vec::with_capacity(replace.len());
        for rline in &replace {
            if rline.trim().is_empty() {
                new_replace.push(rline.clone());
            } else {
                new_replace.push(format!("{add}{rline}"));
            }
        }
        let mut out = whole_lines[..i].to_vec();
        out.extend(new_replace);
        out.extend_from_slice(&whole_lines[i + n..]);
        return ReplaceOutcome::Applied(out.join(""));
    }
    ReplaceOutcome::Failed
}

fn perfect_or_whitespace(
    whole_lines: &[String],
    part_lines: &[String],
    replace_lines: &[String],
) -> ReplaceOutcome {
    match perfect_replace(whole_lines, part_lines, replace_lines) {
        ReplaceOutcome::Applied(s) => ReplaceOutcome::Applied(s),
        ReplaceOutcome::Failed => {
            whitespace_flexible_replace(whole_lines, part_lines, replace_lines)
        }
    }
}

/// aider's `try_dotdotdots`: handle `...` elisions in SEARCH/REPLACE.
fn try_dotdotdots(whole: &str, part: &str, replace: &str) -> ReplaceOutcome {
    let split_on_dots = |s: &str| -> Vec<String> {
        let mut pieces = Vec::new();
        let mut cur = String::new();
        for line in keepends(s) {
            if line.trim() == DOTS {
                pieces.push(std::mem::take(&mut cur));
                pieces.push(line);
            } else {
                cur.push_str(&line);
            }
        }
        pieces.push(cur);
        pieces
    };
    let part_pieces = split_on_dots(part);
    let replace_pieces = split_on_dots(replace);
    if part_pieces.len() != replace_pieces.len() {
        return ReplaceOutcome::Failed; // unpaired ...
    }
    if part_pieces.len() == 1 {
        return ReplaceOutcome::Failed; // no dots in this block
    }
    for i in (1..part_pieces.len()).step_by(2) {
        if part_pieces[i] != replace_pieces[i] {
            return ReplaceOutcome::Failed; // unmatched ...
        }
    }
    let mut whole = whole.to_string();
    for (p, r) in part_pieces
        .iter()
        .step_by(2)
        .zip(replace_pieces.iter().step_by(2))
    {
        if p.is_empty() && r.is_empty() {
            continue;
        }
        if p.is_empty() && !r.is_empty() {
            whole = ensure_trailing_newline(&whole);
            whole.push_str(r);
            continue;
        }
        let count = whole.matches(p.as_str()).count();
        if count != 1 {
            return ReplaceOutcome::Failed; // must match uniquely
        }
        whole = whole.replacen(p, r, 1);
    }
    ReplaceOutcome::Applied(whole)
}

/// aider's `replace_most_similar_chunk` (minus the dead fuzzy branch).
fn replace_most_similar_chunk(whole: &str, part: &str, replace: &str) -> Option<String> {
    let whole = ensure_trailing_newline(whole);
    let part = ensure_trailing_newline(part);
    let replace = ensure_trailing_newline(replace);
    let whole_lines = keepends(&whole);
    let part_lines = keepends(&part);
    let replace_lines = keepends(&replace);

    if let ReplaceOutcome::Applied(s) = perfect_or_whitespace(&whole_lines, &part_lines, &replace_lines) {
        return Some(s);
    }

    // GPT sometimes adds a spurious leading blank line (aider issue #25).
    if part_lines.len() > 2 && part_lines[0].trim().is_empty() {
        let skip_blank = &part_lines[1..];
        if let ReplaceOutcome::Applied(s) = perfect_or_whitespace(&whole_lines, skip_blank, &replace_lines) {
            return Some(s);
        }
    }

    if let ReplaceOutcome::Applied(s) = try_dotdotdots(&whole, &part, &replace) {
        return Some(s);
    }
    None
}

/// aider's `do_replace` for one edit against one file's content.
fn do_replace(path: &str, content: Option<&str>, search: &str, replace: &str) -> Option<String> {
    let search = strip_quoted_wrapping(search, Some(path));
    let replace = strip_quoted_wrapping(replace, Some(path));
    let exists = Path::new(path).exists();

    if !exists && search.trim().is_empty() {
        // new file
        return Some(replace);
    }
    let content = content?;
    if search.trim().is_empty() {
        // append to existing file
        return Some(format!("{content}{replace}"));
    }
    replace_most_similar_chunk(content, &search, &replace)
}

/// Apply a batch of edits atomically: all succeed or nothing is written.
/// Returns per-path new contents on success, or the list of failed blocks.
pub fn apply_edits(edits: &[Edit]) -> Result<HashMap<String, String>, Vec<Edit>> {
    // Read current contents of every touched path.
    let mut contents: HashMap<String, Option<String>> = HashMap::new();
    for e in edits {
        contents.entry(e.path.clone()).or_insert_with(|| {
            std::fs::read_to_string(&e.path).ok()
        });
    }

    let mut staged: HashMap<String, String> = HashMap::new();
    let mut failed: Vec<Edit> = Vec::new();

    for e in edits {
        let current: Option<&String> = staged.get(&e.path);
        let current_ref: Option<&str> = match current {
            Some(s) => Some(s.as_str()),
            None => contents.get(&e.path).and_then(|c| c.as_deref()),
        };
        match do_replace(&e.path, current_ref, &e.search, &e.replace) {
            Some(new_content) => {
                staged.insert(e.path.clone(), new_content);
            }
            None => failed.push(e.clone()),
        }
    }

    if failed.is_empty() {
        Ok(staged)
    } else {
        Err(failed)
    }
}

/// Format failed blocks the way aider reports them back to the LLM.
pub fn format_failed_blocks(failed: &[Edit]) -> String {
    let blocks = if failed.len() == 1 { "block" } else { "blocks" };
    let mut res = format!("# {} SEARCH/REPLACE {blocks} failed to match!\n", failed.len());
    for e in failed {
        res.push_str(&format!(
            "\n## SearchReplaceNoExactMatch: this SEARCH block failed to exactly match lines in {}\n<<<<<<< SEARCH\n{}=======\n{}>>>>>>> REPLACE\n",
            e.path, e.search, e.replace
        ));
    }
    res.push_str(
        "\nThe SEARCH section must exactly match an existing block of lines including all white space, comments, indentation, docstrings, etc.\n",
    );
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    fn edit(path: &str, search: &str, replace: &str) -> Edit {
        Edit {
            path: path.to_string(),
            search: search.to_string(),
            replace: replace.to_string(),
        }
    }

    fn parse(content: &str, chat_files: &[&str]) -> Vec<Edit> {
        let files: Vec<String> = chat_files.iter().map(|s| s.to_string()).collect();
        parse_edits(content, &files).0
    }

    #[test]
    fn parses_a_fenced_block_with_filename() {
        let out = "mathweb/flask/app.py\n```python\n<<<<<<< SEARCH\nfrom flask import Flask\n=======\nimport math\nfrom flask import Flask\n>>>>>>> REPLACE\n```\n";
        let edits = parse(out, &["mathweb/flask/app.py"]);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].path, "mathweb/flask/app.py");
        assert_eq!(edits[0].search, "from flask import Flask\n");
        assert_eq!(edits[0].replace, "import math\nfrom flask import Flask\n");
    }

    #[test]
    fn parses_multiple_blocks_and_reuses_filename() {
        let out = "a.py\n<<<<<<< SEARCH\none\n=======\ntwo\n>>>>>>> REPLACE\n<<<<<<< SEARCH\nthree\n=======\nfour\n>>>>>>> REPLACE\n";
        let edits = parse(out, &["a.py"]);
        assert_eq!(edits.len(), 2);
        assert_eq!(edits[1].path, "a.py");
    }

    #[test]
    fn accepts_marker_leniency_aider_style() {
        // 7 <'s with trailing '>' and trailing whitespace, per aider regex.
        let out = "a.py\n<<<<<<< SEARCH \none\n======= \ntwo\n>>>>>>> REPLACE \n";
        let edits = parse(out, &["a.py"]);
        assert_eq!(edits.len(), 1);
    }

    #[test]
    fn new_file_block_has_empty_search() {
        let out = "hello.py\n<<<<<<< SEARCH\n=======\ndef hello():\n    print(\"hello\")\n>>>>>>> REPLACE\n";
        let edits = parse(out, &[]);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].path, "hello.py");
        assert!(edits[0].search.is_empty());
    }

    #[test]
    fn shell_blocks_are_collected_not_edits() {
        let out = "```bash\nls -la\n```\na.py\n<<<<<<< SEARCH\nx\n=======\ny\n>>>>>>> REPLACE\n";
        let files = vec!["a.py".to_string()];
        let (edits, shells) = parse_edits(out, &files);
        assert_eq!(edits.len(), 1);
        assert_eq!(shells.len(), 1);
        assert_eq!(shells[0].0.trim(), "ls -la");
    }

    #[test]
    fn filename_missing_falls_back_to_current() {
        // Second block has no filename line: reuses a.py (aider behavior).
        let out = "a.py\n<<<<<<< SEARCH\nx\n=======\ny\n>>>>>>> REPLACE\n<<<<<<< SEARCH\nz\n=======\nw\n>>>>>>> REPLACE\n";
        let edits = parse(out, &["a.py"]);
        assert_eq!(edits.len(), 2);
        assert_eq!(edits[1].path, "a.py");
    }

    #[test]
    fn deepseek_style_fenced_filename() {
        // ```python\nword_count.py\n``` before the block (aider issue style).
        let out = "```python\nword_count.py\n```\n```python\n<<<<<<< SEARCH\nx\n=======\ny\n>>>>>>> REPLACE\n```\n";
        let edits = parse(out, &["word_count.py"]);
        assert_eq!(edits.len(), 1);
        assert_eq!(edits[0].path, "word_count.py");
    }

    #[test]
    fn replace_exact_first_match_wins() {
        // aider replaces the FIRST exact match, not a unique one.
        let whole = "def f():\n    pass\n\ndef f():\n    pass\n";
        let out = replace_most_similar_chunk(whole, "def f():\n    pass\n", "def f():\n    return 1\n");
        assert_eq!(out.unwrap(), "def f():\n    return 1\n\ndef f():\n    pass\n");
    }

    #[test]
    fn replace_missing_leading_whitespace() {
        let whole = "def f():\n    if x:\n        return 1\n";
        let out = replace_most_similar_chunk(whole, "if x:\n    return 1\n", "if x:\n    return 2\n");
        assert_eq!(out.unwrap(), "def f():\n    if x:\n        return 2\n");
    }

    #[test]
    fn replace_varied_leading_whitespace() {
        // SEARCH omits some (not all) indentation; uniform delta applied.
        let whole = "class A:\n    def m(self):\n        if x:\n            return 1\n";
        let out = replace_most_similar_chunk(
            whole,
            "  def m(self):\n      if x:\n          return 1\n",
            "  def m(self):\n      if x:\n          return 2\n",
        );
        assert_eq!(
            out.unwrap(),
            "class A:\n    def m(self):\n        if x:\n            return 2\n"
        );
    }

    #[test]
    fn replace_skips_spurious_leading_blank_search_line() {
        let whole = "one\ntwo\nthree\n";
        let out = replace_most_similar_chunk(whole, "\none\ntwo\n", "ONE\ntwo\n");
        // A blank first line + ≥3 part lines: aider retries without it.
        assert_eq!(out.unwrap(), "ONE\ntwo\nthree\n");
    }

    #[test]
    fn replace_dotdotdots_elision() {
        let whole = "def f():\n    a = 1\n    b = 2\n    c = 3\n    return a + b + c\n";
        let part = "def f():\n    a = 1\n...\n    return a + b + c\n";
        let replace = "def f():\n    a = 1\n...\n    return a * b * c\n";
        let out = replace_most_similar_chunk(whole, part, replace);
        assert_eq!(
            out.unwrap(),
            "def f():\n    a = 1\n    b = 2\n    c = 3\n    return a * b * c\n"
        );
    }

    #[test]
    fn replace_fails_cleanly_on_no_match() {
        let whole = "one\ntwo\n";
        assert!(replace_most_similar_chunk(whole, "nope\n", "x\n").is_none());
    }

    #[test]
    fn dotdotdots_requires_unique_match() {
        let whole = "x\nx\n";
        let part = "x\n...\nx\n";
        assert!(replace_most_similar_chunk(whole, part, "y\n...\ny\n").is_none());
    }

    #[test]
    fn apply_edits_is_atomic_all_or_nothing() {
        let dir = std::env::temp_dir().join(format!("aiders-test-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let a = dir.join("a.txt");
        std::fs::write(&a, "alpha\nbeta\n").unwrap();

        let good = edit(
            a.to_str().unwrap(),
            "alpha\n",
            "ALPHA\n",
        );
        let bad = edit(
            a.to_str().unwrap(),
            "does-not-exist\n",
            "X\n",
        );
        let result = apply_edits(&[good, bad]);
        assert!(result.is_err());
        // Nothing written: file untouched.
        assert_eq!(std::fs::read_to_string(&a).unwrap(), "alpha\nbeta\n");

        let good2 = edit(a.to_str().unwrap(), "alpha\n", "ALPHA\n");
        let staged = apply_edits(&[good2]).unwrap();
        assert_eq!(staged.get(a.to_str().unwrap()).unwrap(), "ALPHA\nbeta\n");
    }

    #[test]
    fn apply_edits_appends_and_creates() {
        let dir = std::env::temp_dir().join(format!("aiders-append-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let existing = dir.join("existing.txt");
        std::fs::write(&existing, "one\n").unwrap();
        let newf = dir.join("new.txt");

        let e1 = edit(existing.to_str().unwrap(), "", "\ntwo\n");
        let e2 = edit(newf.to_str().unwrap(), "", "brand new\n");
        let staged = apply_edits(&[e1, e2]).unwrap();
        assert_eq!(staged[existing.to_str().unwrap()], "one\n\ntwo\n");
        assert_eq!(staged[newf.to_str().unwrap()], "brand new\n");
    }

    #[test]
    fn multiple_edits_same_file_chain() {
        let dir = std::env::temp_dir().join(format!("aiders-chain-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let a = dir.join("chain.txt");
        std::fs::write(&a, "one\ntwo\nthree\n").unwrap();

        let e1 = edit(a.to_str().unwrap(), "one\n", "1\n");
        let e2 = edit(a.to_str().unwrap(), "three\n", "3\n");
        let staged = apply_edits(&[e1, e2]).unwrap();
        assert_eq!(staged[a.to_str().unwrap()], "1\ntwo\n3\n");
    }

    #[test]
    fn quoted_wrapping_is_stripped() {
        let s = strip_quoted_wrapping("```\ninner\n```\n", None);
        assert_eq!(s, "inner\n");
        let s = strip_quoted_wrapping("a.py\n```\ninner\n```\n", Some("a.py"));
        assert_eq!(s, "inner\n");
        let s = strip_quoted_wrapping("inner\n", None);
        assert_eq!(s, "inner\n");
    }
}