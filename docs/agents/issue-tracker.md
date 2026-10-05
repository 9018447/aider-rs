# Issue tracker: Local Markdown

本仓库的 issues、specs 与 tickets 以 markdown 文件落在 `.scratch/`。

## 约定

- 一个 feature 一个目录：`.scratch/<feature-slug>/`
- spec 位于 `.scratch/<feature-slug>/spec.md`
- 实现 tickets 一文件一条，位于 `.scratch/<feature-slug>/issues/<NN>-<slug>.md`，自 `01` 起编号，绝不合并成单个文件
- triage 状态以每文件顶部的 `Status:` 行记录（角色字符串见 `triage-labels.md`）
- 评论与对话历史以 `## Comments` 标题追加到文件末尾

## 当技能说「发布到 issue tracker」

在 `.scratch/<feature-slug>/` 下新建文件（必要时创建目录）。

## 当技能说「获取相关 ticket」

直接读取引用的路径或编号对应的文件。