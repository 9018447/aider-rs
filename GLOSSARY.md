# GLOSSARY — aider-rs

项目领域词汇表。为领域概念命名前先读本表。

- **aider-rs**：本项目的极轻量 Rust 版 AI 结对编程 agent，以 Claude Code plugin 形式运行。
- **Claude Code**：编排 agent。负责直接向 aider-rs 下达编码命令并消费其返回结果；主人不再作为中间传话者。
- **MCP 工具**：Claude Code 通过模型上下文协议调用的能力入口。aider-rs 对外暴露三个：`aider_task`、`aider_undo`、`aider_status`。
- **编辑格式（edit format）**：aider-rs 让 LLM 以特定格式汇报修改、再由它精确落盘的协议。v1 支持两种：`diff`（SEARCH/REPLACE 块）与 `whole`（整文件输出，用于新建）。
- **SEARCH/REPLACE 块（diff）**：以唯一匹配原文块进行文本替换的编辑格式，匹配不到即失败回退。
- **自动提交（auto-commit）**：每次编辑成功落到磁盘后自动创建一次 git commit 的行为，是 undo 链的地基。
- **undo 链**：基于自动提交历史、可逐次回退编辑的机制（`aider_undo`）。
- **repo map**：aider 原版用 tree-sitter 生成并喂给 LLM 的仓库大纲。**本项目明确不做**；代码上下文由文件树、grep/文件检索与显式指定文件提供。
- **seam（测试边界）**：测试行为的外部边界。本项目两层：进程 MCP 接口（黑盒端到端）+ 核心库 API（单元）。
- **spec / ticket / issue tracker**：本仓库用本地 markdown tracker 记录规范（`.scratch/<feature>/spec.md`）与任务（`.scratch/<feature>/issues/NN-slug.md`）。
- **会话级常驻**：aider-rs 作为 plugin 的 stdio 宿主进程，在一次 Claude Code 会话内常驻、共享状态；会话结束进程销毁，无跨会话状态（状态只落 git 与文件系统）。
- **冷启动**：从进程拉起(d到)可服务首次 MCP 工具调用所需时间；验收目标 <200ms。