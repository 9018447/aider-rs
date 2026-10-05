# 06: 性能达标

**What to build:** 通过已确认的量化验收指标：release 静态单二进制 <15MB、冷启动 <200ms（到可服务首次工具调用）、常驻内存 <50MB（不含模型上下文文本）；含依赖瘦身与编译/运行参数调优。

**Blocked by:** 03 配置与多端点、04 undo 与 status·超时、05 编辑格式对齐

**Status:** closed ✅ (2026-10-06)

- [x] release 构建为静态单二进制，尺寸 <15MB
- [x] 冷启动时间（进程拉到可服务首次工具调用）<200ms
- [x] 常驻内存 <50MB（不含模型上下文文本）
- [x] 上述三项均有可复现的测量命令与记录
- [x] 达标过程不破坏 02–05 已通过的端到端与语义行为
---

**Closed:** 2026-10-06 · commits: 7480c6f

**Outcome:** release 构建（opt-level=z/lto/strip/panic=abort）实测：二进制 1.8MB <15MB ✅、冷启动 132.7ms <200ms ✅、RSS 2.5MB <50MB ✅、热分发 0.32ms。scripts/bench.py 可复现（exit code 即验收）。
