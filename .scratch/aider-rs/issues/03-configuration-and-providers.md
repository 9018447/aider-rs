# 03: 配置与多端点

**What to build:** 通过 `.env` 与配置文件加载密钥与模型设置；支持 Anthropic 原生与 OpenAI 兼容端点（`base_url`）动态选择；保持单模型模式。

**Blocked by:** 02 端到端任务闭环

**Status:** ready-for-agent

- [ ] 可从 `.env` / 配置文件读取 API 密钥与模型，无硬编码
- [ ] Anthropic 原生与 OpenAI 兼容端点均可配置并被按需选用
- [ ] 缺失或非法配置给出明确错误，不影响已装载部分
- [ ] 配置读取行为有测试覆盖