# 02: 端到端任务闭环

**What to build:** 从 `aider_task` 发一个自然语言编码任务，aider-rs 调用 LLM、以 diff（SEARCH/REPLACE 块）编辑应用到单个文件、自动 git 提交，并返回变更摘要（diff + 提交号 + token 用量）的完整最小闭环。

**Blocked by:** 01 插件骨架与 MCP 宿主

**Status:** ready-for-agent

- [ ] 通过 `aider_task` 提交一个任务能真正修改单文件并落盘
- [ ] 修改成功后自动创建一次 git 提交，返回可识别的提交号
- [ ] 返回负载包含变更 diff 与 token 用量
- [ ] 采用选定的 LLM 接入之一（OpenAI 兼容或 Anthropic 原生）可跑通、密钥走环境变量
- [ ] 端到端径路上均有测试（发任务→改文件→提交→返回）