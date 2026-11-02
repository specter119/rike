# RichEditor 小实验

> 这个实验工程（`spikes/rich-editor/`）已在 0.1.0 发布前删除；结论保留在这里，下面的命令已不能运行。

代码曾在 `spikes/rich-editor/`。
NSH rev `a5036af`，只用 `nsh_base::rich_editor`（ProseMirror 的 Rust 移植）。

## 问题

M1 的块编辑是“源码模式”：点一下进入原始 Markdown 编辑，失焦后渲染。
RichEditor 能不能换成“所见即所得”的块编辑？关键是 **Markdown 往返是否保真**，因为文件格式是 Logseq 风格的 Markdown 子集，
不能被编辑器改写。

## 结果

**结论：M1 不采用。** 编辑器本身能挂载、能输入、`**bold**` 快捷键能变成加粗；但往返会改写 Logseq 的语法。

往返保真度（Markdown → 编辑器文档 → Markdown，`tests/roundtrip.rs`）：

| 输入                                 | 输出                                 | 是否一致 |
| ------------------------------------ | ------------------------------------ | -------- |
| 粗体、斜体、行内代码               | 相同                                   | 一致     |
| `[text](https://example.com)`        | 相同                                 | 一致     |
| `TODO write it`、`DONE ~~old~~`      | 相同                                 | 一致     |
| `((uuid))`                           | 相同                                 | 一致     |
| `key:: value`                        | 相同                                 | 一致     |
| `[[Page Name]]`                      | `\[\[Page Name\]\]`                  | **改写** |
| `#[[long tag]]`                      | `#\[\[long tag\]\]`                  | **改写** |
| `snake_case_name and 2 * 3 * 4`      | `snake\_case\_name and 2 \* 3 \* 4`  | **改写** |
| `- nested\n  - list`                 | `- nested\n\n  - list`（多一个空行） | **改写** |

输入测试（`tests/typing.rs`）：在编辑器里键入 `hello [[Page]] **bold**`，输出是 `hello \[\[Page\]\] **bold**`。

## 原因

- 编辑器的文档模型里没有“页面引用”这种节点或标记，`[[x]]` 只是普通文本。序列化器按 CommonMark 规则转义了 `[`、`_`、`*`。
- 要保真，需要在 schema 里加 `pageRef`、`tag`、`blockRef` 三种 inline 节点，并在 `MarkdownNames` / 输入规则 / 粘贴规则里接上，
  再补上 `[[` 自动补全（NSH 有 `suggestion` 扩展）。这是一个独立的工作量，超出 M1。

## 对后续的建议

1. M1 保持源码模式；这个结论不阻塞任何 M1 功能。
1. 以后若要做所见即所得，走“自定义 inline 节点”路线：schema 加 `pageRef` / `tag` / `blockRef`，序列化时写回 Logseq 语法，
   用往返测试（本实验的 `SAMPLES`）作为回归。
1. 每个块一个 `RichEditor` 的成本要单独评估：一个页面几百个块时，每块一个编辑器不现实；更可能是“只有正在编辑的块是 RichEditor，其余仍用 `TextView`”，
   和 M1 的“点击 → 编辑”结构一致。
