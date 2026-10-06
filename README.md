# aider-rs


An extremely lightweight Rust port of [aider](https://aider.chat), packaged as
a **Claude Code plugin** (MCP server).

Claude Code orchestrates. aider-rs codes: it runs its own LLM loop, applies
edits with aider's SEARCH/REPLACE format, and auto-commits every successful
edit round — so Claude Code talks to it directly instead of you relaying
messages between two agents.

```
Claude Code  --(MCP stdio: aider_task / aider_undo / aider_status)-->  aider-rs
                                                                         |
                                                                    your git repo
```

## Why

- **No cold-start tax.** aider-rs is session-resident: Claude Code spawns it
  once per session and every tool call talks to the same warm process
  (sub-millisecond dispatch, ~130ms cold start, 1.8MB binary, ~2.4MB RSS).
- **Direct communication.** Claude Code sends tasks and gets back diffs,
  commit hashes, and token usage. No copy-pasting between terminals.
- **Safe by construction.** aider-rs never executes shell commands the model
  suggests; edits apply atomically (all-or-nothing) and only ever in a git
  repository.

## Install

```bash
./plugin/install.sh   # builds (local-disk target) and stages plugin/bin/aider-rs
```

Then add the plugin to Claude Code — either via a plugin marketplace that
points at the `plugin/` directory, or by referencing the server in your
`.mcp.json`:

```json
{
  "mcpServers": {
    "aider-rs": { "command": "/absolute/path/to/aider-rs/plugin/bin/aider-rs" }
  }
}
```

aider-rs runs with your working directory as the repository root, so start
Claude Code in the repo you want edited.

## Configure

Configuration resolves in this order (highest first):

1. environment variables
2. `.env` in the working directory
3. `.aider-rs.json` in the repository root
4. `~/.config/aider-rs/config.json`

| Key | Env var | Meaning |
|---|---|---|
| provider | `AIDER_RS_PROVIDER` | `anthropic` or `openai` (OpenAI-compatible) |
| model | `AIDER_RS_MODEL` | model name |
| api_key | `ANTHROPIC_API_KEY` / `OPENAI_API_KEY` | provider key |
| base_url | `OPENAI_API_BASE` / `ANTHROPIC_BASE_URL` | custom endpoint (OpenRouter, DeepSeek, Ollama, vLLM…) |
| timeout_secs | `AIDER_RS_TIMEOUT_SECS` | per-request LLM timeout (default 120) |
| task_timeout_secs | `AIDER_RS_TASK_TIMEOUT_SECS` | wall-clock budget per aider_task, including retry rounds (default 600); on timeout nothing is written |
| max_edit_retries | `AIDER_RS_MAX_EDIT_RETRIES` | retry rounds when SEARCH blocks fail to match (default 1) |

System prompt: a non-empty `~/.config/aider-rs/AGENTS.md` is appended to the
built-in SEARCH/REPLACE prompt as additional instructions for every task;
keep the file empty (or absent) to use the default alone.

JSON keys in the config files are the left column, e.g.
`{"provider": "openai", "model": "deepseek-chat", "base_url": "https://api.deepseek.com/v1"}`.

## Tools

- **`aider_task`** — send a natural-language coding task. aider-rs runs its
  LLM loop, applies SEARCH/REPLACE edits, auto-commits, and returns the diff,
  commit hash, and token usage. Optional `files` (explicit context targets)
  and `reset_context` (fresh conversation).
- **`aider_undo`** — rewind the last aider-rs edit round. Only ever resets
  commits aider-rs itself made, and only while HEAD still points at them.
  Uses `git reset --keep`, so uncommitted changes to files the commit did
  not touch survive the rewind; conflicting dirty state aborts the undo
  with a clear error instead of destroying work.
- **`aider_status`** — session state (model, tasks, tokens) and git state
  (branch, dirty counts, recent commits).

## Differences from aider (by design)

- No repo map: context comes from explicit file lists, not tree-sitter outlines.
- Edit formats: `diff` (SEARCH/REPLACE) primary; `whole` (filename + fenced
  full content) as a fallback when no SEARCH/REPLACE blocks are present.
- Failed SEARCH matches are fed back to the model for one retry round
  (`AIDER_RS_MAX_EDIT_RETRIES`, default 1) before the task fails.
- Shell commands the model proposes are reported back to Claude Code, never executed.
- No chat-mode commands (`/add`, `/drop`, …); Claude Code orchestrates instead.

## Performance

Acceptance targets (ticket 06): binary < 15MB, cold start < 200ms,
resident memory < 50MB. Reproduce with:

```bash
./plugin/install.sh
python3 scripts/bench.py
```

Recorded on the dev machine (rustc 1.99.0, x86_64 Linux, 2026-10-06):
binary 1.8MB, cold start ~131ms, RSS ~2.4MB, warm dispatch ~0.2ms.

## Development

```bash
export CARGO_TARGET_DIR=/root/.cache/aider-rs-target  # or any local-disk dir
cargo test
```

The test suite has two seams: unit tests on the core library (edit-format
parsing/applying, git semantics, config precedence) and process-level e2e
tests that drive the real binary over MCP stdio against a mock LLM server.

## License

Apache-2.0. The edit-format engine and prompts are ported from
[aider](https://github.com/Aider-AI/aider) by Paul Gauthier (Apache-2.0).
See `LICENSE` and `NOTICE`.
