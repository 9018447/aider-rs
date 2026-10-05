//! Git integration: status, staging, auto-commit, diff, and the undo chain.
//! The aider-rs session tracks every commit it creates; `undo` only ever
//! rewinds commits the session itself made, and only while HEAD still points
//! at them (the spec's "never reset someone else's commits" rule).

use std::path::{Path, PathBuf};
use std::process::Command;

/// A thin wrapper over the `git` CLI, rooted at a working directory.
pub struct Git {
    pub root: PathBuf,
    user_name: String,
    user_email: String,
}

#[derive(Debug)]
pub struct GitError(pub String);

impl std::fmt::Display for GitError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
impl std::error::Error for GitError {}

impl Git {
    /// Open the repository containing `dir` (or fail with a clear error).
    pub fn open(dir: &Path) -> Result<Self, GitError> {
        let out = Self::run(dir, &["rev-parse", "--show-toplevel"])?;
        let root = PathBuf::from(String::from_utf8_lossy(&out.stdout).trim().to_string());
        // Fall back to an explicit identity so commits never fail on machines
        // without git user config.
        let user_name = String::from_utf8_lossy(
            &Self::run(dir, &["config", "user.name"])
                .map(|o| o.stdout)
                .unwrap_or_default(),
        )
        .trim()
        .to_string();
        let user_email = String::from_utf8_lossy(
            &Self::run(dir, &["config", "user.email"])
                .map(|o| o.stdout)
                .unwrap_or_default(),
        )
        .trim()
        .to_string();
        Ok(Self {
            root,
            user_name: if user_name.is_empty() { "aider-rs".into() } else { user_name },
            user_email: if user_email.is_empty() { "aider-rs@localhost".into() } else { user_email },
        })
    }

    fn run(dir: &Path, args: &[&str]) -> Result<std::process::Output, GitError> {
        Command::new("git")
            .args(["-C", &dir.to_string_lossy()])
            .args(args)
            .output()
            .map_err(|e| GitError(format!("failed to run git: {e}")))
    }

    fn git(&self, args: &[&str]) -> Result<String, GitError> {
        let out = Command::new("git")
            .args(["-C", &self.root.to_string_lossy()])
            .arg("-c")
            .arg(format!("user.name={}", self.user_name))
            .arg("-c")
            .arg(format!("user.email={}", self.user_email))
            .args(args)
            .output()
            .map_err(|e| GitError(format!("failed to run git: {e}")))?;
        if !out.status.success() {
            return Err(GitError(format!(
                "git {} failed: {}",
                args.join(" "),
                String::from_utf8_lossy(&out.stderr).trim()
            )));
        }
        Ok(String::from_utf8_lossy(&out.stdout).into_owned())
    }

    /// Is `dir` inside a git work tree?
    pub fn is_repo(dir: &Path) -> bool {
        Self::run(dir, &["rev-parse", "--is-inside-work-tree"])
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    pub fn head_hash(&self) -> Result<String, GitError> {
        Ok(self.git(&["rev-parse", "HEAD"])?.trim().to_string())
    }

    pub fn branch(&self) -> Result<String, GitError> {
        let b = self
            .git(&["rev-parse", "--abbrev-ref", "HEAD"])?
            .trim()
            .to_string();
        Ok(b)
    }

    /// `(staged, unstaged, untracked)` file counts.
    pub fn status_counts(&self) -> Result<(usize, usize, usize), GitError> {
        let s = self.git(&["status", "--porcelain"])?;
        let mut staged = 0;
        let mut unstaged = 0;
        let mut untracked = 0;
        for line in s.lines() {
            let b = line.as_bytes();
            if b.len() < 2 {
                continue;
            }
            match (b[0], b[1]) {
                (b'?', _) => untracked += 1,
                (b' ', b'?') => untracked += 1,
                (x, b' ') if x != b' ' && x != b'?' => staged += 1,
                (b' ', y) if y != b' ' => unstaged += 1,
                (x, y) if x != b' ' && y != b' ' => {
                    staged += 1;
                    unstaged += 1;
                }
                _ => {}
            }
        }
        Ok((staged, unstaged, untracked))
    }

    /// Recent one-line commits, most recent first.
    pub fn log(&self, n: usize) -> Result<Vec<String>, GitError> {
        let s = self.git(&["log", "--oneline", &format!("-{n}"), "--"])?;
        Ok(s.lines().map(|l| l.to_string()).collect())
    }

    /// Stage `paths`, commit with `message`, return the new HEAD short hash.
    pub fn commit_paths(&self, paths: &[&str], message: &str) -> Result<String, GitError> {
        if paths.is_empty() {
            return Err(GitError("nothing to commit: no paths to stage".into()));
        }
        let mut add_args = vec!["add", "--"];
        add_args.extend(paths.iter().copied());
        self.git(&add_args)?;

        let cached = self.git(&["diff", "--cached", "--name-only"])?;
        if cached.trim().is_empty() {
            return Err(GitError("nothing to commit: no staged changes".into()));
        }
        self.git(&["commit", "-m", message, "--"])?;
        self.head_hash()
    }

    /// Diff between two commits (full hashes or refs), unified format.
    pub fn diff_refs(&self, from: &str, to: &str) -> Result<String, GitError> {
        self.git(&["diff", from, to, "--"])
    }

    /// Rewind the last commit with `git reset --hard HEAD~1` after verifying
    /// `expected_head` is the current HEAD (caller checks it's an aider-rs
    /// commit). Returns the new HEAD hash.
    pub fn reset_last_commit(&self, expected_head: &str) -> Result<String, GitError> {
        let head = self.head_hash()?;
        if head != expected_head {
            return Err(GitError(format!(
                "HEAD has moved since that commit ({head} != {expected_head}); refusing to undo"
            )));
        }
        self.git(&["reset", "--hard", "HEAD~1"])?;
        self.head_hash()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_repo(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("aiders-git-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let _ = Command::new("git")
            .args(["-C", &dir.to_string_lossy(), "init", "-q"])
            .output();
        let _ = Command::new("git")
            .args([
                "-C",
                &dir.to_string_lossy(),
                "-c",
                "user.name=t",
                "-c",
                "user.email=t@t",
                "commit",
                "--allow-empty",
                "-m",
                "init",
                "--",
            ])
            .output();
        dir
    }

    #[test]
    fn commit_and_undo_roundtrip() {
        let dir = temp_repo("roundtrip");
        let file = dir.join("f.txt");
        std::fs::write(&file, "v1\n").unwrap();
        let git = Git::open(&dir).unwrap();
        let head1 = git.head_hash().unwrap();
        let head2 = git
            .commit_paths(&["f.txt"], "aider-rs: add f.txt")
            .unwrap();
        assert_ne!(head1, head2);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), "v1\n");

        // Undo restores pre-commit state.
        let head3 = git.reset_last_commit(&head2).unwrap();
        assert_eq!(head3, head1);
        assert!(
            !file.exists(),
            "reset --hard removes the file added by the undone commit"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn undo_refuses_moved_head() {
        let dir = temp_repo("moved");
        let git = Git::open(&dir).unwrap();
        std::fs::write(dir.join("a.txt"), "a\n").unwrap();
        let _h = git.commit_paths(&["a.txt"], "one").unwrap();
        std::fs::write(dir.join("b.txt"), "b\n").unwrap();
        let h2 = git.commit_paths(&["b.txt"], "two").unwrap();
        // Try to undo an older commit while HEAD moved on.
        assert!(git.reset_last_commit("0000000000000000000000000000000000000000").is_err());
        assert!(git.reset_last_commit(&h2).is_ok());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn status_counts_tracks_changes() {
        let dir = temp_repo("status");
        std::fs::write(dir.join("u.txt"), "u\n").unwrap();
        let git = Git::open(&dir).unwrap();
        let (_, _, untracked) = git.status_counts().unwrap();
        assert_eq!(untracked, 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}