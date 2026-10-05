# 05: 编辑格式对齐

**What to build:** 完善编辑应用引擎：`whole`（整文件，用于新建文件）兜底；`diff`（SEARCH/REPLACE）匹配不到即失败回滚的精确语义；并用 aider 既有编辑样例做回归对齐，保证编辑语义与 aider 一致。

**Blocked by:** 02 端到端任务闭环

**Status:** ready-for-agent

- [ ] `whole` 可新建/整体替换文件
- [ ] SEARCH/REPLACE 唯一性匹配失败时不做部分写入、整体回滚
- [ ] 用 aider 既有编辑样例跑回归，diff 语义行为与 aider 对齐
- [ ] 编辑应用引擎在核心库 seam 有单元测试覆盖正向、重复匹配、匹配失败等情形