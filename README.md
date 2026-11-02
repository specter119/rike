# Rike（日课）

一个用 Rust 写的、兼容 Logseq 文件格式的大纲笔记原型。

> 状态：M1 原型，开发中。数据格式和界面都可能变化，请先在笔记库的副本上试用。

## 它是什么

Rike 保留 Logseq 最受欢迎的几件事：

- **大纲编辑**：每个块都是一个条目，可以缩进、折叠、移动。
- **每日页面**（界面上暂称 Journals，名字还没定）：每天一个 Markdown 文件，打开就是今天。
- **双向链接**：用 `[[页面]]`、`#标签`、`((块引用))` 互相连接，页面底部显示谁引用了它。

笔记就是你磁盘上的 Markdown 文件，没有私有数据库。删掉 Rike，文件还在。

### 和 Logseq、tine 的关系

- Logseq 的文件版（现称 Logseq OG）已进入维护状态，主线转向数据库版。Rike 面向文件版的目录和语法。
- [tine](https://github.com/martinkoutecky/tine) 是另一个兼容 Logseq 文件版的替代品。Rike 是独立实现，没有参考它的代码。
- Rike 只兼容 Logseq 格式的一个**子集**，不保证与 Logseq 逐字节一致。无法识别的内容会原样保留。
- 界面使用 [NSH](https://github.com/longbridge/nsh)：界面代码是 TSX（`ui/`），由 Rust 宿主在 Linux 上用 GTK 4 窗口运行；数据和保存逻辑在 Rust 里（`crates/rike-native`），TSX 通过 `Rike` 原生模块调用。

## M1 功能

- **选库**：不带参数启动时自动打开上次的库；没有上次的库就显示欢迎页，列出最近打开的库，也可以“打开文件夹…”。已有的 Logseq 文件夹可以直接打开；不像笔记库的非空文件夹会先问你。欢迎页里打开是原地进入；已经打开库时，侧栏顶部的库菜单可以切换到另一个库（保存后重启进程）。
- **恢复上次的页面**：下次打开同一个库时回到离开时的页面（Journals 或某个页面），页面不存在了就回到 Journals。
- 打开一个笔记库，Journals 先显示今天（没有文件也能直接写），再往前显示已有的每日页面，每次 7 天，可“加载更多”。每天是一张独立的卡片，标题下面写着对应的文件路径；点日期标题打开那一天的单页。
- 任务块（`TODO`、`DOING`、`DONE` 等）前面有可点的复选框：点一下完成，再点一下回到 `TODO`。点复选框不会进入编辑。`DOING`、`NOW`、`LATER`、`WAIT` 会显示成小标签，`CANCELED` 只显示删除线。
- 后退 / 前进：侧栏的 ← → 按钮、`Alt+←` / `Alt+→`、鼠标侧键（后退）。
- 点击块编辑 Markdown 源码，离开后渲染。
- 双向链接和页面底部的反向引用面板。
- 侧栏（Journals、所有页面）和 `Ctrl+K` 搜索。
- 外部修改（例如同步工具、其他编辑器）会实时刷新；如果你正好有未保存的编辑，则在页面顶部提示，由你选择。
- **数学公式**：`$x^2$`、`\(x^2\)` 是行内公式，`$$…$$`、`\[ … \]` 是独立成行的展示公式（`\[` 要在块的行首，`\]` 在行尾）。渲染走 Typst（先把 LaTeX 转成 Typst 数学再排版），颜色跟随深浅主题。点公式会进入这个块的源码编辑。转换不了的公式显示原来的 TeX 源码，不会丢内容。
- 侧栏底部的“退出”按钮（或 `Ctrl+W`）：先把所有修改写进文件再关窗口。
- 跟随系统深色或浅色。
- 中文等输入法：在 Wayland 下检测到 fcitx（`XMODIFIERS` 或 `QT_IM_MODULE` 含 fcitx）且装了 fcitx 的 GTK 模块时，Rike 会自动使用 `GTK_IM_MODULE=fcitx`，原因见 `AGENTS.md`。如果你自己设置了 `GTK_IM_MODULE`，以你的为准。

### 快捷键

| 按键                          | 作用                                                   |
| ----------------------------- | ------------------------------------------------------ |
| `Enter`                       | 在光标处拆分块                                         |
| `Shift+Enter`                 | 块内换行                                               |
| `Tab` / `Shift+Tab`           | 缩进 / 反缩进                                          |
| `Alt+Shift+↑` / `Alt+Shift+↓` | 上移 / 下移块                                          |
| `Ctrl+Enter`                  | 无标记 → TODO → DONE → 无标记（`DOING` 等直接变 DONE） |
| `Esc`                         | 退出编辑                                               |
| `Alt+←` / `Alt+→`             | 后退 / 前进                                            |
| `Ctrl+K`                      | 搜索                                                   |
| `Ctrl+W`                      | 保存并退出                                             |
| `Ctrl+Z` / `Ctrl+Shift+Z`     | 撤销 / 重做块结构操作（非编辑状态）                    |

### 暂不支持

`[[` 自动补全、复制块引用、远端同步、`{{query}}` 查询、属性类型、未链接引用、页面重命名、图谱视图、白板、插件、多窗口、图片附件。路线图见下文。

## 安装与运行

目前只支持 Linux。

依赖：Rust 1.97 或更新版本、GTK 4.14 或更新版本、clang、`nsh` 命令行（构建界面用）。

```sh
# Arch Linux
sudo pacman -S gtk4 clang
cargo install --locked --git https://github.com/longbridge/nsh --rev a5036af48ddc193b5023ece9b9196c76b8f8d3f5 nsh-cli

scripts/build-release.sh          # 先编译 TSX 界面并嵌入，再编译 rike
target/release/rike               # 自动打开上次的库，没有就显示欢迎页
target/release/rike ~/notes       # 打开已有目录
target/release/rike --vault ~/new # 目录不存在就创建
```

`rike --help` 列出所有参数。最近打开的库记在 `$XDG_STATE_HOME/rike/vaults`（默认 `~/.local/state/rike/vaults`）。不能把文件系统根目录、主目录或主目录的上级目录当笔记库。

没有用 `scripts/build-release.sh` 构建（也就是没有嵌入界面）时，`rike` 会报错退出；开发时用下面的热重载。

### 开发

```sh
scripts/dev.sh [rike 参数]     # nsh dev 服务 + rike --dev-server，改 TSX 立即热重载
scripts/ui-test.sh [文件]      # 无头跑 ui/*.test.tsx（每个测试各用一个新库）
(cd ui && nsh check)           # TSX 类型检查
```

`nsh dev --server-only` 监听 `0.0.0.0` 且没有绑定地址选项，运行期间同一网络里的人都能连上，只在可信网络里用。`rike --dev-server` 只接受本机的 `ws://` 地址，且需要 `--features dev` 的构建。

`RUST_LOG` 控制日志（默认 `warn`）；`--trace`、`--fps` 打开界面性能日志。

## 笔记库

```
<vault>/
  journals/2026_10_31.md     每日页面，每天一个文件
  pages/项目___日课.md         页面“项目/日课”，命名空间的 / 写成 ___
  assets/                    附件目录（M1 不处理）
  .rike/                  Rike 自己的数据：备份、冲突副本、临时文件
```

### 文件格式

```markdown
tags:: 项目, 笔记

- TODO 写 [[README]] #文档
  id:: 6356e902-3b7b-4cb2-8c3e-6a904c813c40
	- 子块用 tab 缩进，可以引用 ((6356e902-3b7b-4cb2-8c3e-6a904c813c40))
```

- 第一个 `- ` 之前的 `key:: value` 是页面属性，块首行之后紧跟的 `key:: value` 是块属性。
- 只有被引用的块才会写入 `id::`。
- 读取时也接受空格缩进，保存时统一写成 tab。
- 如果库里有 `logseq/config.edn`，Rike 会读取其中的每日页面标题与文件名格式，让 `[[Oct 31st, 2026]]` 这类链接仍然指向对应的每日页面。

## 数据安全

- **不要同时用 Rike 和 Logseq 打开同一个库。** 两者都会写文件，可能互相覆盖。
- 每次覆盖文件前会先检查它是否被外部修改过。
- 覆盖前会在 `.rike/backups/` 保留旧版本，每页保留最近 20 份。
- 如果你在 Rike 里有修改，同时文件又被外部改动，Rike 不会覆盖磁盘上的版本：你的版本会先存到 `.rike/conflicts/`，页面顶部提示，由你选择“载入磁盘版本”或“覆盖磁盘”。
- 无法安全往返的页面会以**只读**方式打开，标题旁标注“只读”并给出提示。这是为了避免把你已有的内容写坏。
- 快速打字后立刻按 `Alt+←` 后退，最后几个字可能没有提交。
- 编辑后约 0.5 秒内自动保存。点“退出”按钮或按 `Ctrl+W` 退出时会先保存；如果有页面正等你在“载入磁盘版本”和“覆盖磁盘”之间选择，会提示并拒绝退出，免得丢内容。切换库同理：先保存再重启。
- 直接点窗口的关闭按钮或按 `Ctrl+Q` 时，Linux 上没有退出钩子，刚输入完的最后半秒内容可能丢失。想稳妥就用“退出”。
- 尽管如此，原型仍可能有 bug。请先备份。

## 路线图

- **M1**：大纲、每日页面、双向链接、搜索、本地文件监听、选库与切库。
- **M2**：[HawDB](https://github.com/nowledge-co/hawdb) 索引后端（中文全文检索、图查询）、属性类型与页面模板、未链接引用、页面重命名、简单查询。界面是 TSX，为以后的插件系统留了路。
- **M3**：通过 [OpenDAL](https://github.com/apache/opendal) 同步到 WebDAV 或 S3，所见即所得编辑器评估。

## 许可证

以下任选其一：

- [MIT](LICENSE-MIT)
- [Apache-2.0](LICENSE-APACHE)

依赖全部是宽松许可证（没有 GPL/AGPL）。界面库 [NSH](https://github.com/longbridge/nsh) 是 Apache-2.0。公式渲染用 `typst-assets` 内嵌了 DejaVu、Libertinus、New Computer Modern 等字体；以后发布二进制时，要附上这些字体的署名和 NOTICE。
