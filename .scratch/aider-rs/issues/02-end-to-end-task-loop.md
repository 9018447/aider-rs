# 02: 端到端任务闭环

**What to build:** 从 `aider_task` 发一个自然语言编码任务，aider-rs 调用 LLM、以 diff（SEARCH/REPLACE 块）编辑应用到单个文件、自动 git 提交，并返回变更摘要（diff + 提交号 + token 用量）的完整最小闭环。

**Blocked by:** 01 插件骨架与 MCP 宿主

**Status:** closed ✅ (2026-10-06)

- [x] 通过 `aider_task` 提交一个任务能真正修改单文件并落盘
- [x] 修改成功后自动创建一次 git 提交，返回可识别的提交号
- [x] 返回负载包含变更 diff 与 token 用量
- [x] 采用选定的 LLM 接入之一（OpenAI 兼容或 Anthropic 原生）可跑通、密钥走环境变量
- [x] 端到端径路上均有测试（发任务→改文件→提交→返回）
---

**Closed:** 2026-10-06 · commits: b82e222, 7480c6f

**Outcome:** 完整闭环：任务→LLM→SEARCH/REPLACE 应用→自动 commit→返回 diff+commit+token。e2e task_edits_commits_and_undo_restores / retry_round_can_recover 验证（mock LLM + 真实临时 git 仓库）。
