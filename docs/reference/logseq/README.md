# Logseq 文档快照（Markdown 方言与渲染）

这里是 [logseq/docs](https://github.com/logseq/docs) 的**只读快照**，只收录定义
Markdown 方言、属性、引用和渲染的那几十页。用途是给 rike 一个可以随时对照的规范来源，
而不是靠记忆或猜测。

- 上游：`https://github.com/logseq/docs`（MIT，见 `LICENSE.md`）
- 快照提交：见 `MANIFEST.md` 的 `commit`
- 刷新：`scripts/sync-logseq-docs.sh`
- 检查上游是否移动：`scripts/sync-logseq-docs.sh --check`（有漂移时 exit 1）

**不要手改 `pages/` 下的文件**，它们由脚本覆盖。要改收录范围，改脚本里的 `pages` 列表。

## 为什么 vendored 而不是链接

- 上游文档会改，链接会失效；快照 + 脚本让"相对实时"变成一条命令
- 离线可读：改解析器时不用联网
- 可 diff：上游改了哪一页，`git diff` 里直接看见

## 范围

收录的是**文件格式和渲染**相关页面：Markdown 语法、属性、文件名格式、命名空间、
页面/块引用、标签、别名、任务、代码块、公式、表格、Hiccup、宏、嵌入媒体、查询、
模板、config.edn、导出，以及 `term___*` 那批术语页。`Org Mode.org` 也在里面，
作为 org 格式的对照。

**不在范围内**：产品功能页（白板、插件、同步、发布、移动端）、更新日志、教程。
需要时把它们加进 `scripts/sync-logseq-docs.sh` 的列表。

## 和 rike 的关系

- rike 只支持 `:triple-lowbar` 文件名格式（见 `pages/Filename format.md`）。`:legacy`
  库会收到一条告警，因为它的页面名依赖 `title::` 才能还原。
- `pages/Built-in Properties.md` 和 `pages/Properties.md` 是属性语义的权威来源；
  rike 的兼容缺口清单在 `../logseq-properties.md` §10。
- `pages/Markdown.md` 是扩展语法清单（`[[x]]`、`((uuid))`、`{{embed}}`、优先级、
  Hiccup 等），rike 实现了其中一部分。
