# 03: 配置与多端点

**What to build:** 通过 `.env` 与配置文件加载密钥与模型设置；支持 Anthropic 原生与 OpenAI 兼容端点（`base_url`）动态选择；保持单模型模式。

**Blocked by:** 02 端到端任务闭环

**Status:** closed ✅ (2026-10-06)

- [x] 可从 `.env` / 配置文件读取 API 密钥与模型，无硬编码
- [x] Anthropic 原生与 OpenAI 兼容端点均可配置并被按需选用
- [x] 缺失或非法配置给出明确错误，不影响已装载部分
- [x] 配置读取行为有测试覆盖
---

**Closed:** 2026-10-06 · commits: b82e222, 7480c6f

**Outcome:** env > .env > .aider-rs.json > ~/.config 四级优先级（dotenv 解析器 + 单测 + e2e config_file_precedence_model_visible_in_status）；Anthropic 原生与 OpenAI 兼容双客户端，anthropic_native_provider_end_to_end 全链路验证。
