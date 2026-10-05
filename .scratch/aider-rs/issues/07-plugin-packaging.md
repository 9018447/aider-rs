# 07: 插件打包发布

**What to build:** 把 aider-rs 打包为可发布的 Claude Code plugin：插件清单、README、版本管理；在 Claude Code 中安装并验证自动发现连接；Apache 2.0 版权声明合规（沿袭 aider）。

**Blocked by:** 06 性能达标

**Status:** closed ✅ (2026-10-06)

- [x] 产出一份完整 Claude Code plugin 清单（含 MC 服务声明与版本）
- [x] README 说明安装、配置（密钥/端点）与三个工具的用法
- [x] 在 Claude Code 中安装后能被自动发现并连接、三个工具可用
- [x] 保留/标注 Apache 2.0 版权声明
- [x] 记录安装与验证步骤，可供他人复现
---

**Closed:** 2026-10-06 · commits: 7480c6f

**Outcome:** plugin/ 完整清单 + install.sh（本地盘 target 防云盘损坏构建产物）；README（安装/配置/工具/差异/性能）；Apache-2.0 LICENSE + NOTICE（aider 署名 Paul Gauthier）；.gitignore。
