# AGENTS.md — aider-rs

aider-rs 是一个极轻量 Rust 版 AI 结对编程 agent，以 Claude Code plugin 的形式运行。Claude Code 负责编排，直接通过 MCP 工具向 aider-rs 下达编码任务；aider-rs 自带 LLM 完成任务并与 git 自动提交配合。

本文件是承载项目 agent 约定的约定文件（由 setup-matt-pocock-skills 生成）。

## Agent skills

### Issue tracker

Issues、specs 与 tickets 以 markdown 文件形式落在 `.scratch/` 之下（本地 markdown tracker；一个 feature 一个目录，ticket 一文件一条，编号自 `01` 起）。见 `docs/agents/issue-tracker.md`，领域术语见 `GLOSSARY.md`。

### Triage labels

使用默认五标签词汇（`needs-triage`、`needs-info`、`ready-for-agent`、`ready-for-human`、`wontfix`），在本地 ticket 文件中以 `Status:` 行表达。见 `docs/agents/triage-labels.md`。

### Domain docs

单上下文布局：仓库根一份 `GLOSSARY.md`，ADRs 位于 `docs/adr/`。为领域概念命名前先读词汇表，避免自造同义词。见 `docs/agents/domain.md`。