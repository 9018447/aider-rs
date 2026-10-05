# 05: 编辑格式对齐

**What to build:** 完善编辑应用引擎：`whole`（整文件，用于新建文件）兜底；`diff`（SEARCH/REPLACE）匹配不到即失败回滚的精确语义；并用 aider 既有编辑样例做回归对齐，保证编辑语义与 aider 一致。

**Blocked by:** 02 端到端任务闭环

**Status:** closed ✅ (2026-10-06)

- [x] `whole` 可新建/整体替换文件
- [x] SEARCH/REPLACE 唯一性匹配失败时不做部分写入、整体回滚
- [x] 用 aider 既有编辑样例跑回归，diff 语义行为与 aider 对齐
- [x] 编辑应用引擎在核心库 seam 有单元测试覆盖正向、重复匹配、匹配失败等情形
---

**Closed:** 2026-10-06 · commits: b82e222, 7480c6f

**Outcome:** SEARCH/REPLACE 完整移植（围栏剥壳、文件名回溯、perfect/whitespace 弹性匹配、... 省略、空 SEARCH 新建/追加）；whole 兜底（无 S/R 块时文件名+围栏=整文件内容，EditKind::Whole）；aider 真实样例回归 9 例（test_editblock.py 移植：whitespace 变体、首匹配、issue #25 空行、无尾换行多块）。
