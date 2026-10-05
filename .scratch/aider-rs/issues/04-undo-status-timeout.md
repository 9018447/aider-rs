# 04: undo 与 status、超时

**What to build:** `aider_undo` 沿 commit 链逐次回退上次编辑；`aider_status` 返回当前会话与 git 状态；长任务具备超时与部分结果/进度返回，出错后工作区保持一致、可恢复。

**Blocked by:** 02 端到端任务闭环

**Status:** closed ✅ (2026-10-06)

- [x] `aider_undo` 能把工作区回退到上次编辑前，多次回退依次生效
- [x] `aider_status` 报告会话上下文状态与 git 工作区状态
- [x] 超过超时的任务给出部分结果/明确的中止说明，不残留回滚或半写状态
- [x] 任务失败或超时后工作区回到一致状态，可继续后续任务
- [x] undo 与 status 在进程 MCP seam 有端到端测试
---

**Closed:** 2026-10-06 · commits: b82e222, 7480c6f

**Outcome:** undo 链（multi_undo_rewinds_commits_in_order 多级回退验证）+ moved-HEAD 拒绝；status 报会话+git 双状态；任务级超时 AIDER_RS_TASK_TIMEOUT_SECS（默认 600s，worker 线程 + recv_timeout，超时零写入）；undo 改用 reset --keep 保护无关脏文件（undo_keeps_unrelated_dirty_files_but_refuses_conflicts）。
