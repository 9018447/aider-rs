# aider-rs 规范（SPEC）

> 由 to-spec 依据 grilling 2026-10-06 共识综合产出 · 本地 markdown tracker 使用

## Problem Statement

主人在 Claude Code 与 aider（Python 版结对编程 agent）之间手动转述任务与结果，形成传话链条、易失真且低效；同时 aider 每次冷启动（进程拉起 + 解释器 + 依赖加载）耗时明显，打断了对话式工作流。需要一个极轻量、以会话级常驻进程形态运行的 aider Rust 版，让 Claude Code 直接向其下达编码命令，它自带 LLM 完成任务并与 git 自动提交配合，从而把主人移出通讯链路、消除每次冷启动等待。

## Solution

aider-rs：用 Rust 编写的轻量 AI 结对编程 agent，打包为 Claude Code plugin。在一次 Claude Code 会话内，aider-rs 作为插件提供的 MCP 宿主进程常驻（启动成本每会话只付一次）。Claude Code 通过 `aider_task` 直接发送自然语言编码任务；aider-rs 自己调用 LLM 完成代码编辑并自动 git 提交，返回变更摘要与提交号。提供 `aider_undo` 回退与 `aider_status` 查询，支撑编排与恢复。状态不跨会话持久化，只落 git 与文件系统。

## User Stories

1. 作为 Claude Code 用户，我希望 Claude Code 能直接向 aider-rs 下发编码任务而无需我复制粘贴转述，以便退出两个 agent 之间的传话角色。
2. 作为用户，我希望 aider-rs 作为 Claude Code plugin 的常驻进程运行，以便在同一会话的多次任务间共享进程状态、避免重复冷启动。
3. 作为用户，我希望每次编码任务完成后看到变更 diff、git 提交号与 token 用量，以便确认它实际做了什么。
4. 作为用户，我希望 aider-rs 能在既有 git 仓库中工作并自动提交每次编辑，以便形成可回退、可审查的历史。
5. 作为用户，我希望在出错时能通过 aider_undo 回退到编辑前状态，以便不污染代码库。
6. 作为用户，我希望几乎立即获得首次工具调用响应（冷启动 <200ms 起），以便对话不被冷启动打断。
7. 作为用户，我希望 aider-rs 调用我配置的模型（Anthropic 原生或 OpenAI 兼容端点），以便复用我的 provider 与密钥。
8. 作为用户，我希望 aider-rs 仅占用小量的磁盘与内存（单二进制 <15MB、常驻 <50MB），以便轻量地长期挂载。
9. 作为用户，我希望随时通过 aider_status 查询 aider-rs 的会话与 git 状态，以便了解其上下文。
10. 作为用户，我希望 aider-rs 在出现错误或超时后仍能把工作区保持在一致状态，以便可恢复继续。
11. 作为用户，我希望安装为本插件的 aider-rs 一次配置即可被 Claude Code 自动发现并连接，以便开箱即用。
12. 作为用户，我希望 aider-rs 的每次编辑遵循「先匹配再替换」的精确语义、匹配不到即失败并回滚，以便不产生破坏性误写。
13. 作为用户，我希望一份既有 aider 编辑样例能被用于回归校验本实现，以便对齐 aider 的编辑语义。

## Implementation Decisions

- **语言与交付物**：Rust，产出一个静态单二进制；Apache 2.0 授权（沿袭 aider），保留版权声明。
- **进程模型**：会话级常驻。aider-rs 作为 Claude Code plugin 的 stdio MCP 宿主进程；会话启动连接、结束销毁；不做跨会话 daemon。状态只落 git 与文件系统。
- **MCP 工具面**：`aider_task`（发任务→返回 diff + 提交号 + token 用量，支持可选上下文重置）、`aider_undo`（沿 commit 链回退）、`aider_status`（会话与 git 状态）。长任务需超时控制与进度/部分结果返回。
- **LLM 接入**：Anthropic 原生 API + OpenAI 兼容端点（`base_url` 可配，覆盖 OpenAI/DeepSeek/OpenRouter/Ollama/vLLM 等）。密钥取自环境变量 + `.env`。单模型模式（不做 architect/editor 双模型）。
- **编辑格式**：v1 支持 `diff`（SEARCH/REPLACE 块）+ `whole`（整文件，用于新建）；udiff/diff-fenced 不做，接口留扩展位。
- **git 集成**：每次编辑成功自动 commit，形成 undo 链；与编排侧约定「不 reset aider-rs 的 commit」。
- **repo map**：明确不做（见 ADR-0001）；上下文 = 文件树 + grep + 显式指定。
- **会话历史**：同一 Claude Code 会话内多次 `aider_task` 共享进程内上下文；跨会话清空；任务可带重置上下文开关。
- **核心分层**：核心库（LLM 循环、编辑应用引擎、git 封装）与 MCP 宿主进程分离，支撑两层 seam 测试。

## Testing Decisions

- **好测试的标准**：只测外部行为与可观察结果（落盘后的文件内容、git 历史、MCP 应答），不测内部实现细节。
- **两层 seam**：
  1. 进程 MCP 接口（端到端）：黑盒从 stdin/stdout 的 JSON-RPC 驱动 `aider_task`/`aider_undo`/`aider_status`，验证「发任务→真实改文件→自动提交→返回结果」完整链路。
  2. 核心库 API（单元）：测编辑应用引擎（diff/whole 的精确 apply 与失败回滚）、LLM 循环协议解析、git 操作行为。
- **对齐基准**：以 aider 既有编辑样例作为解析/应用回归用例，保证与 aider 编辑语义一致。
- **prior art**：全新仓库无既有测试；采用 Rust 标准测试框架 + 进程级集成测试。

## Out of Scope

- repo map（彻底不做，见 ADR-0001）
- voice、浏览器/Web UI、watch mode
- litellm 级 provider 广度、architect/editor 双模型、udiff/diff-fenced
- benchmark 完整移植（仅取编辑样例做对齐测试）
- 跨会话状态的系统级 daemon、多客户端并发一致性（v1 单一会话）

## Further Notes

- 已确认的量化验收指标：单二进制 <15MB、冷启动 <200ms、常驻内存 <50MB（不含模型上下文文本）。
- 参考实现：`/Coze/Drive/编程专家/aider`（shallow clone，Apache 2.0）。
- 完整共识与里程碑：`/Coze/Drive/编程专家/aider-rs-plan.md`。