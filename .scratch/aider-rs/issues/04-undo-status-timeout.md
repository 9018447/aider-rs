# 04: undo 与 status、超时

**What to build:** `aider_undo` 沿 commit 链逐次回退上次编辑；`aider_status` 返回当前会话与 git 状态；长任务具备超时与部分结果/进度返回，出错后工作区保持一致、可恢复。

**Blocked by:** 02 端到端任务闭环

**Status:** ready-for-agent

- [ ] `aider_undo` 能把工作区回退到上次编辑前，多次回退依次生效
- [ ] `aider_status` 报告会话上下文状态与 git 工作区状态
- [ ] 超过超时的任务给出部分结果/明确的中止说明，不残留回滚或半写状态
- [ ] 任务失败或超时后工作区回到一致状态，可继续后续任务
- [ ] undo 与 status 在进程 MCP seam 有端到端测试