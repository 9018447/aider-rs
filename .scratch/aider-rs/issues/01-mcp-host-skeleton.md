# 01: 插件骨架与 MCP 宿主

**What to build:** 一个可被 Claude Code 插件机制加载的常驻宿主进程，暴露 `aider_task`/`aider_undo`/`aider_status` 三个工具的探测占位；Claude Code 会话启动时连接它并能在 `/mcp` 中列出这三个工具。

**Blocked by:** None（可立即开始）

**Status:** closed ✅ (2026-10-06)

- [x] Claude Code 通过插件机制连接宿主进程并列出 `aider_task`、`aider_undo`、`aider_status` 三个工具
- [x] 三个工具在未实现逻辑时返回明确的「未实现」应答而不崩溃
- [x] 会话启动成本只支付一次（同一会话多次调用共享同一进程）
- [x] 宿主进程在会话结束时可被正常终止，不留孤儿进程
- [x] 产物为可通过插件机制加载的宿主（含插件清单与可执行件）
---

**Closed:** 2026-10-06 · commits: 39ab46a

**Outcome:** MCP stdio 宿主：initialize/ping/tools/list/tools/call 全实现，三工具注册带 JSON Schema；e2e 验证握手、工具列举（tools_list_exposes_three_tools_with_schemas）、未知方法 -32601、EOF 干净退出；plugin/ 清单（plugin.json + .mcp.json + install.sh）就位。
