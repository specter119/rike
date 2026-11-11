# AGENTS.md

面向开发者和 coding agent。用户文档见 `README.md`。

## 项目概述

Rike 是兼容 Logseq 文件格式的桌面大纲笔记原型：Rust 管数据和保存，NSH 的 TSX 管界面。Markdown 文件是唯一数据源（local-first）。当前阶段是 M1。

## 工具链与命令

- Rust 1.97 或更新（edition 2024）。cargo 是 huacnlee/cargo（`~/.local/share/cargo/bin`），rustc 来自 rustup（`RUSTUP_HOME=~/.local/share/rustup`，代理在 `~/.local/share/rustup-cargo/bin`，PATH 顺序不能让 rustup 的 cargo 挡住 fork cargo）。没有 `rust-toolchain.toml`。
- Linux 依赖：`gtk4`（4.14 或更新）、`clang`、`nsh` 命令行（`cargo install --locked --git https://github.com/longbridge/nsh nsh-cli`，装最新 main）。不需要 WebKitGTK。
- 命令（cargo 命令前设 `TMPDIR=$HOME/.cache/sx-tmp`）：
  - 检查：`cargo check --workspace`
  - 格式：`cargo fmt --all --check`
  - Lint：`cargo clippy --workspace --all-targets -- -D warnings`
  - Rust 测试：`cargo test --workspace`
  - TSX 类型检查：`(cd ui && nsh check)`（第一次会生成 `ui/.nsh/types`，`ui/.nsh/` 不进版本库）
  - TSX 测试：`scripts/ui-test.sh [测试文件]`（无头，不开窗口）
  - 发布构建：`scripts/build-release.sh`（`nsh build --release` 产出 `ui/.nsh/out/bundle.js`，`touch crates/rike-app/build.rs` 后 `cargo build --release -p rike-app`，`build.rs` 把 bundle 嵌进二进制）
  - 热重载：`scripts/dev.sh [参数]`（`nsh dev --server-only` + `rike --dev-server ws://127.0.0.1:<端口>/ws`，需要 `--features dev`）
  - 运行：`target/release/rike [DIR | --vault DIR]`
- 交付前 fmt、clippy、cargo test、`nsh check`、`scripts/ui-test.sh` 都要通过。
- `Cargo.lock` 纳入版本控制。所有依赖版本放在根 `Cargo.toml` 的 `[workspace.dependencies]`，子 crate 用 `workspace = true`。
- `nsh` 内部构建生成的 crate 时会用 `$CARGO`：fork cargo 的共享目标缓存可能复用旧产物（`describe` 二进制过期、rmeta 混版本），所以 `scripts/dev.sh`、`scripts/ui-test.sh`、`scripts/build-release.sh` 都把 `CARGO` 指向 rustup 的标准 cargo；直接跑 `nsh check` 时也要设。升级 nsh（`cargo update -p nsh-core` 或重装 CLI）后如报 “described by nsh x and this nsh is y”，删 `ui/.nsh/` 和 `~/.cache/rike-ui-target` 再试。
- `nsh dev --server-only` 监听 `0.0.0.0` 且无绑定选项；`rike` 因此只接受 loopback 的 `--dev-server`（`cli::is_loopback_ws`）。别在不可信网络里开着它。
- `rike-app` 的 `nsh-platform-linux` 必须开 `js` feature，否则日志里只有 “this build of nsh runs Rust apps only”，窗口是空白的。

## 架构与 crate 边界

```
crates/rike-core    模型、解析、序列化、行内 tokenizer、任务标记（task.rs）。纯函数，无 IO。
crates/rike-vault   vault actor：OpenDAL fs、watcher、写前检查、备份、冲突。
crates/rike-index   Index trait + MemIndex（M2 路线：tantivy+中文检索 → turso/hawdb 基准对比，见路线图）。
crates/rike-native  给 TSX 用的原生模块 `Rike`：Session（页面状态、gen、编辑、保存、撤销、退出/切库）、api（DTO 和函数）、state（最近的库）、startup（启动决策）、md、math、doc。
crates/rike-app     宿主二进制 rike：命令行、日志、tokio runtime、启动决策、窗口、嵌入 TSX bundle。
ui/                 TSX 界面（NSH）。`nsh.json` 的 "rust" 指向 crates/rike-native。
spikes/typst-math      `rike_native::math` 的命令行（`one`、`scan <dir>`）和公式提取器。不在 default-members。
spikes/tsx-pump        常驻 pump 任务的 TSX 实验（独立工程，不在 workspace）。
scripts/               build-release.sh、dev.sh、ui-test.sh；wayland-ime-smoke.sh（输入法复现）和它用的 gtk4_textview.py 对照程序。
fixtures/vaults/       测试笔记库。
docs/spikes/           探针与实验结论（tsx.md 是迁移到 TSX 的结论）。
```

- 依赖方向：`app` → `native` → `index`、`vault` → `core`。`core` 不能依赖 NSH、OpenDAL、tokio。`session`、`state`、`startup` 不依赖 NSH（只有 `api.rs` 依赖）。
- 新代码放进现有 crate，不要为了小功能新增 crate。

## 界面与原生模块（`ui/`、`crates/rike-native`）

分工：**Rust 管数据和保存，TSX 管界面状态**。判定准则：凡是“丢了会丢数据或写坏文件”的状态只归 Rust；TSX 需要的数据副本只能通过事件失效 + 重取获得，永不双向写——TSX 侧的任何副本都必须能被一条事件整体作废。编辑中的文本例外：正在编辑时，挂载的 `Input` 是最新文本的权威（`registerEditorText`），`flushDraft` 总是优先读它，`noteDraft` 只做定时器的回退值。Rust 的 `leave` 是退出/切库的唯一权威（并发 leave 会拒绝，拒绝时 Rust 和 TSX 各自复位 `quitting`）。

状态归属：

| 状态 | 归属 | 同步机制 |
| --- | --- | --- |
| 页面文档、gen、撤销/重做栈、保存节奏 | Rust `session.rs` | 同步调用 + gen 校验，结构操作返回整页 |
| 编辑登记（busy） | Rust `Edit` | `setEditing`/`clearEditing`；TSX 只在 Rust 调用成功后更新自己的 `editing` |
| 编辑器当前文本 | 挂载的 `Input`（TSX） | `flushDraft` → `setDraft`；`Input` 卸载后回退到 `noteDraft` 捕获值 |
| 页面缓存（`pageMap`） | TSX | `page`/`PageChanged` 事件失效 + 重取 |
| 视图/历史栈、本地 toast（负 id）、`quitting` 的 UI 投影 | TSX `store.ts` | `navigate`；退出结果跟随 `Rike.leave` 返回值 |
| 通知（正 id） | Rust `notices` | `notify` 事件推送 |
| 事件积压合并 | `session.rs::coalesce`（唯一实现） | Session backlog 和 api `BACKLOG` 两层共用同一函数 |
| 最近库/状态文件、启动决策 | Rust `state.rs`/`startup.rs` | 原子写 |

术语：`gen`（文档）、`generation`（DTO 字段）、`r#gen`（Rust 关键字转义）指同一个东西；TSX 的 `epoch` 是编辑器重建的局部序号，与 gen 无关。

```
ui/App.tsx        根：installShortcuts()、init()、加载中/欢迎页/库
ui/src/store.ts   全部界面状态（模块级 signal）和动作：启动、导航与历史、页面缓存、编辑、搜索、通知、退出/切库
ui/src/block.tsx  块行：圆点、任务复选框、渲染视图、编辑器 Input
ui/src/doc.tsx    页面/日记卡片（标题、路径、横幅、行、反链）；page.tsx、journals.tsx、backlinks.tsx、search.tsx、sidebar.tsx、welcome.tsx、notices.tsx、math.tsx、shortcuts.ts、widgets.tsx
ui/*.test.tsx     无头测试；ui/testkit.ts 是测试辅助（start、startWelcome、untilFile）
crates/rike-native/src/api.rs      `Rike` 模块：DTO（驼峰）和函数；线程局部的 SESSION、事件监听
crates/rike-native/src/session.rs  Session：页面、gen、编辑、草稿、结构操作、撤销、保存、pump、leave、open
crates/rike-native/src/state.rs    最近的库文件（`$XDG_STATE_HOME/rike/vaults`）读写，原子写，条目上限 20
crates/rike-native/src/startup.rs  启动决策：命令行目录 > 最近的库 > 欢迎页；拒绝根目录/主目录及其上级
crates/rike-app/src/main.rs        解析命令行，开 tokio runtime，决定打开什么，`session::set_boot`，预热公式渲染，`run_app`，窗口关闭后 flush
```

- **运行时归属**：vault actor、watcher、状态文件 IO 都跑在宿主 `main` 创建的 tokio runtime 上（`Boot.handle`）。`pump()` 和 `try_leave` 在引擎线程上跑，进入 runtime 要用 `session::in_runtime(handle, future)`（每次 poll 进入），**不要**把 `handle.enter()` 的 guard 跨 `.await` 持有：多个任务交错时 guard 乱序销毁会 panic（tokio ≥ 1.54）。
- **Boot 是进程全局**（`session::set_boot` / `boot()`，克隆不取走）。`nsh dev`/`nsh test` 没有 Rust 宿主，`boot_or_dev()` 会用 `RIKE_DEV_VAULT` 在自己的 runtime 上开库；`RIKE_DEV_TODAY=yyyy-mm-dd` 固定“今天”。
- **gen（generation）**：每页一个，只在“磁盘内容替换了内存文档”（外部变更重载、载入磁盘版本、冲突后重载）时递增，随 `PageChanged` 事件下发。草稿和操作都带 gen，旧 gen 一律拒绝。TSX 收到 `PageChanged` 后重取页面；若该页正在编辑则提示不自动重载。正在编辑的块用新 `epoch` 重建编辑器（epoch 是 TSX 局部序号，只在 TSX 侧递增），所以 `Input` 的初始值只在挂载时读一次（草稿模式）；Rust 侧文本变化通过事件驱动重取，不反向写入 TSX 的 `Input`。
- **忙碌（busy）= 脏或正在编辑**：忙碌的页面收到外部变更只发提示（`external` 通知），不自动重载。`setEditing/clearEditing` 登记正在编辑的块；TSX 侧的 `editing` 是调用成功后的投影，Rust 调用失败时 TSX 不更新自己的状态（`endEdit` 先调 `clearEditing` 再清 TSX 状态，失败时编辑器保持打开、可重试）。
- **结构操作**（`split/indent/outdent/moveUp/moveDown/remove/toggleCollapse/toggleTodo/setTaskDone/undo/redo`）是同步调用，返回 `{ focus, page }`；每行带 Rust 算好的 `markdown`（`[[x]]`、`#tag`、`((uuid))` 已改写成 `sx-page:`/`sx-block:` 链接）和 `rev`。TSX 用 `page` 整页替换并按 `focus` 重新挂编辑器。
- **补全**：`Rike.complete(query, kind)`（`kind` 为 `"page"`/`"block"`）出候选；块候选用 `Index::search_blocks`（只搜块文本，页面标题命中不能吃掉面板预算）。接受走 `Rike.replace(rel, gen, id, start, end, expected, insert)`（UTF-16 区间拼接，`expected` 必须与当前文本一致，否则报 `StaleText`；`start > end` 报 `InvalidRange`；`focus.caret` 带重建后的光标）。不依赖 `InputRef.insertText`：headless 测试器的 `insert_text` 只追加不替换选区，真实 GTK 平台才是替换。触发器在 TSX（`[[`/`((`），候选面板的 ↑↓/Enter/Tab 用 `keymap`，Esc 不拦（让它走 endEdit）。
- **事件**：`Rike.onEvent(cb)` 返回的 `Stop` 带注册 id，只撤销自己的注册。没有监听者时事件进积压并在下次注册时合并回放：积压有两层（`session.rs` 的 `backlog` 和 `api.rs` 的 `BACKLOG`），合并规则只有一份——`session.rs::coalesce`，`IndexVer`、`Pages` 各留一个，`PageChanged` 每页留最新，通知全留。加新事件种类时只改这一个函数。
- **任务标记**只在 `rike_core::task` 里解析，`ops::toggle_todo`、`ops::set_task_done`、`md::render` 共用，不要再写第二份关键字列表。复选框是块文本的兄弟元素，点它不会进入编辑。NSH 的 `Checkbox` 没有自己的外观：必须给 `indicatorClass`/`indicator`，否则它不可见、测试也找不到 `id:task-checkbox`。取消勾选固定回 `TODO`。
- **视图历史**（`store.ts`）：后退/前进栈上限 100；所有页面切换走 `navigate`，先结束编辑、关搜索，视图不同才入栈并清空前进栈；重载、冲突处理不写历史。`BackHandler.on("press")`（来自 `nsh/mobile`；Linux 上 Alt+←、Back 键、鼠标第 8 键）要在 `installShortcuts` 的 effect 外面注册。`Alt+→` 走 keymap。
- **撤销**只记录结构操作的整页快照（上限 100）；编辑中的文本撤销交给 `Input`；编辑中不响应 `ctrl+z`。在 Journals 视图里 Ctrl+Z 作用于最近一次结构操作所在的页面（`undoTarget()`）。
- **退出/切库**：全部走 `Session::leave(verb, path)`（`"quit"` 关窗口，`"switch"` 重启进入另一个库；内部是 `try_leave`/`try_leave_inner` 两层）。进行中的 `leave` 会拒绝第二次调用（“正在退出”），拒绝时 Rust 复位 `quitting`，TSX 跟随返回值复位 `quittingS`。TSX 先置 `quitting`（禁用界面，同时关搜索），等 100 ms 让最后的 `onChange` 到达，`flushDraft`、`clearEditing`，再调 `Rike.leave`。**quitting 期间导航族（`navigate`/`back`/`forward`）和 `acceptDisk`/`keepMine` 直接拒绝**：leave 保存/复查期间不许改 stale 状态或“最后视图”。Rust 侧：取消防抖 → `vault.flush()` → 有页面 stale（未处理的冲突/外部修改）就拒绝并返回提示 → 对每个有改动的页面保存（失败即拒绝）→ 再 flush → 复查；通过后调 `Hooks`（关窗口 / relaunch）。拒绝时 `quitting` 复位，可以再试。X 和 Ctrl+Q 不经过这里，是文档化的限制。
- **Session 借用**：`Session` 内部是 `RefCell`，同步方法借用后不能跨 `.await` 持有；异步路径先取出需要的数据再 `.await`。
- **`set_ui_sets` 对整个进程生效**：以后做插件系统时，插件之间无法靠它隔离，要另想办法。
- **启动**：`startup::decide`——命令行目录（`--vault` 可创建，位置参数必须存在）优先，其次最近的库第一项，否则欢迎页。非 Logseq 的非空文件夹在欢迎页/库菜单里要确认（`Rike.inspect` 返回 `kind`），空文件夹和 Logseq 库直接开。欢迎页打开是原地进入（`Rike.open` + `Rike.start`）；已经开着库时切换要重启进程（`relaunch` 重新执行当前可执行文件，带 `--trace/--fps/--dev-server` 和 `--vault`）。最后的页面记在最近的库文件里（`journals` 或 `page:<rel>`），启动时页面不存在就退回 Journals。
- **日志**：`logging::init`，默认 `warn`；`--trace`/`--fps` 打开 `nsh::perf=info`；`RUST_LOG` 最后生效。
- **测试钩子**（只在 `RIKE_DEV_VAULT` 设置、宿主没开库时有效，真实运行碰不到）：`Rike.devReset(seed, files, view)` 在 `RIKE_DEV_VAULT` 下新建目录并开库（`seed` 是 `RIKE_DEV_SEED` 下的夹具目录名），`devWelcome(recent, notice)` 以无库启动，`devFolder(name, files)`，`devRead/devWrite(rel)` 读写当前测试库的文件。测试文件放在 `ui/` 根目录（`open("./App.tsx")` 相对测试文件）。`scripts/ui-test.sh` 设好这些环境变量。
- `RIKE_DEBUG_EDIT_FIRST`（仅 debug 构建）：启动后直接编辑第一个块，给无指针的无头 sway 输入法冒烟脚本用。

## 选型与锁定版本

| 组件          | 选择                                                                                                 | 理由                                                                 |
| ------------- | ---------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------- |
| UI            | NSH（Linux GTK 后端），界面用 TSX（`ui/`），git 依赖跟随上游 main（不锁 rev，具体版本见 `Cargo.lock`；CLI 装最新 main） | 用户指定初版 rev，后升级到 main 拿 `InputRef.insertText`，并改为不锁。TSX 为以后的插件系统留路，也能热重载。RichEditor 的 `suggestion` 补全插件已可用。 |
| 存储          | `opendal =0.59.5`，仅 `services-fs` 和 `executors-tokio`                                             | 二期同步时本地和远端接口一致。0.x 版本常有破坏性改动，所以精确锁定。 |
| 监听          | `notify-debouncer-full` 0.7，使用它重新导出的 `notify`                                               | 避免两个版本的 notify。                                              |
| 哈希          | `blake3`                                                                                             | 判断文件内容是否变化。                                               |
| Markdown 渲染 | NSH `TextView`（markdown-rs）                                                                        | 只渲染单个块的正文。                                                 |
| 索引          | `MemIndex`，放在 `Index` trait 后面                                                                  | 可重建的派生缓存；后端候选评估见 M2 路线（tantivy 先行，turso/hawdb 后续基准）。 |

## 数据格式不变量

这些规则不能破坏。

1. **磁盘格式**：`journals/yyyy_MM_dd.md`、`pages/<名称>.md`。子块用 tab 加 `- `，续行缩进为父缩进加两个空格。读取兼容空格缩进，写出统一用 tab。
1. **属性**：第一个 bullet 之前的 `key:: value` 是页面属性；块首行之后紧跟的 `key:: value` 是块属性。key 小写，`_` 换成 `-`。
1. **无损兜底**：
   - 不认识的内容（YAML front matter、bullet 之前非属性的段落、没有 bullet 的 Markdown、代码围栏内部）一律原样保留为 raw 段。
   - 加载时自检：`serialize(parse(x))` 除了缩进空白和行尾，必须保留所有字符。不通过则页面只读并提示。
   - 规范格式文件往返必须逐字节一致。
1. **块 ID**：每个块内存里都有 UUID。只有被引用时才写 `id::`。出现重复 `id::` 时取第一个并告警。
1. **页面键**：页面名 NFC 规范化后转小写。`title::` 优先于文件名。多个文件映射到同一个页面键时告警，不合并。
1. **文件名编码**：`/` 写成 `___`；其他保留字符做百分号编码。
1. **每日页面（界面暂称 Journals）**：文件名日期和页面标题双向映射；从 `logseq/config.edn` 只提取 `:journal/page-title-format`（默认 `MMM do, yyyy`）、`:journal/file-name-format`（默认 `yyyy_MM_dd`）、`:file/name-format`，不引入完整 EDN 解析器。`:file/name-format` 不是 `:triple-lowbar` 时告警。空日志不落盘，第一次输入才创建文件。
1. **写入范围**：只重写被编辑的页面。忽略 `logseq/`、`.git/`、`.rike/` 和非 `.md` 文件。
1. **只读页**：自检失败的页面，所有写入（包括写 `id::` 和撤销写回）都必须被 vault actor 拒绝。
1. **偏移量**：`core` 内部只用字节偏移。UTF-16 与字节偏移的换算只放在 `rike-native` 的 session 层（`utf16_to_byte`），并要有测试覆盖中文和 emoji。

## 保存与并发规则

- **磁盘 IO 只经过 vault actor**（一个 tokio task，跑在 `main` 里创建的 tokio runtime 上）。引擎线程不做同步 IO（最近的库文件也经 `Boot.handle` 的 `spawn_blocking` 写）。OpenDAL 用异步 `Operator`，不用 blocking API。`VaultHandle` 的 future 只等 channel，所以原生模块的 `async fn` 可以直接 `.await` 它。
- actor 按页面串行写入。每页记录 `base_hash`（blake3）。
- 写前：读取当前文件内容，比较哈希与 `base_hash`。`(mtime, len)` 只用来快速跳过没有变化的文件。
- 原子写：`atomic_write_dir` 设为 `<vault>/.rike/tmp`，必须与 vault 在同一个文件系统。
- 自写识别：看读到的内容哈希是否等于登记的自写哈希，不用时间窗口。
- 备份：每页每次加载后第一次覆盖时备份，之后最多每 10 分钟一次，每页保留 20 份，放在 `.rike/backups/<path>/<ts>.md`。
- 冲突：保存时发现磁盘被外部改动，actor 先把本地版本写进 `.rike/conflicts/` 并确认成功，再返回 `Conflict`；界面在页面顶部提示，用户选“载入磁盘版本”或“覆盖磁盘”（actor 此时已把 `base_hash` 对齐到磁盘，再保存即覆盖）。页面有未保存修改时收到外部变更事件，则只提示（`External`），不自动重载。两种提示都会把 `Doc.stale` 置位，`save_now` 在 stale 时不写盘，直到用户选边（`reload` 或 `keep_mine`）；否则 actor 已对齐的 `base_hash` 会让下一次自动保存静默覆盖外部版本。“载入磁盘版本”会重新 `vault.load`，不使用提示里缓存的内容。
- 页面从磁盘重新加载或进入冲突流程时，清空该页的撤销栈和重做栈。
- 保存节奏：输入 200 ms 后把草稿提交进模型，再 300 ms 去抖落盘；切页、退出编辑、结构操作立即提交草稿，结构操作同时立即落盘（只有打字走 300 ms 去抖）。输入框每次挂载都会推进 `editEpoch`，旧输入框迟到的 `onChange`/`onBlur` 带着旧 epoch 被忽略。`run_app` 没有退出钩子，所以直接关窗口（X、Ctrl+Q）时最后约 0.5 s 的输入可能丢失，这是已知限制，`main` 只在窗口关闭后 flush 已排队的写入。安全退出走“退出”按钮 / `Ctrl+W`，见“界面与原生模块”里的退出/切库。
- 任何结构操作（Tab、移动、拆块、TODO 切换、切页、判断页面是否脏）之前，TSX 先 `flushDraft()` 把 Input 的最新文本写回 Rust 模型。

## 输入法

**现象**：sway + fcitx5、`GTK_IM_MODULE` 未设置时，Rike（以及 NSH 的任何输入框）打不出中文，键盘字母直接上屏。

**根因**（已用 `scripts/wayland-ime-smoke.sh` 复现并验证）：此时 GTK4 走 Wayland 的 `zwp_text_input_v3`。合成器（sway 1.12）只在窗口获得键盘焦点时给 text-input 对象发 `enter`；NSH 在窗口已聚焦之后才创建输入框的 GTK 控件，也就是之后才创建 text-input 对象，所以 `enter`/`enable` 永远不来，fcitx5 没有激活的输入上下文。普通 GTK4 `TextView` 延迟创建也一样（`gtk-late` 目标）。不是 NSH 的 `Input`、我们的 keymap 或保存逻辑的问题。

**处理**：`im::select_gtk_module` 在 `main` 最开头（单线程时）检测到 `XMODIFIERS` 含 `@im=fcitx` 或 `QT_IM_MODULE` 以 `fcitx` 开头、`GTK_IM_MODULE` 未设置、且装有 `libim-fcitx*` 的 GTK4 模块时，设置 `GTK_IM_MODULE=fcitx`（走 DBus，不依赖 `enter`）。用户自己设置的值优先，所以 `GTK_IM_MODULE=wayland` 可以关掉这个处理。验证过：fcitx 模块下 TSX 版 app 的块编辑器、搜索框都有预编辑、候选窗和上屏（用 `block`、`search` 目标测过；最早的纯 Rust 探针已删除，结论在 `docs/spikes/nsh-probe.md`）。ibus 等其他输入法没有处理，也没有验证。

**复现脚本** `scripts/wayland-ime-smoke.sh {gtk|gtk-late|search|block|math|quit} [outdir]`（`search`/`block`/`math` 跑 `target/debug/rike`，它必须嵌了 TSX bundle：先 `(cd ui && nsh build)`，再 `touch crates/rike-app/build.rs`，再 `cargo build -p rike-app`）：

- 在独立的 `dbus-run-session` 里起无头 sway（`WLR_BACKENDS=headless`，不读 `/etc/sway/config.d`，不设 `DISPLAY`，开始前检查所有输出都是 `HEADLESS-*`）和独立的 fcitx5（只复制你的 `profile`，配置和词库都在临时目录），用 `wtype` 打拼音、`grim` 截图，不碰真实桌面。
- 无头 sway 没有指针，所以全部用键盘驱动：块编辑器用仅 debug 构建生效的 `RIKE_DEBUG_EDIT_FIRST=1`，搜索框用 `Ctrl+K`。
- `gtk` 是对照组：它失败说明环境有问题，不能怪应用。`gtk-late` 复现根因。想看修复前的行为，用 `GTK_IM_MODULE=wayland` 运行。
- 产物在 `target/ime-smoke/`：三张截图、`fcitx5.log`、`*-text-input.txt`（`WAYLAND_DEBUG` 里的 text-input 事件，看有没有 `enter`/`enable`）、`fcitx5-remote.txt`（`state: 2` 表示输入法已激活）。

## NSH 已知坑

来自早期探针（`docs/spikes/nsh-probe.md`，工程已删除）、迁移到 TSX 的实测（`docs/spikes/tsx.md`、`tsx-*.md`）和源码阅读。

**通用**

- **keymap 命中即消费**：窗口级快捷键是 capture 阶段的全局快捷键，命中就消费按键，handler 事后才执行，无法放行。TSX 里 `keymap.bind(键, 处理, { when })`：默认在聚焦的输入框用到这个键时不触发；`backspace`、`tab`、`escape`、方向键这类要 `when: "always"`；撤销/重做用 `"notEditing"`。
- **Wayland 下输入框的 text-input 对象晚于窗口焦点创建**：见“输入法”，不改环境变量就无法使用输入法。
- **输入法组合期间要卸载绑定**：Linux 的 keymap 没有组合输入保护。`composing()` 信号为真时，快捷键 effect 不绑定任何键（`cleanup` 里解绑），编辑器里的绑定同理；补全面板在组合中不更新、不接受（preedit 不是块文本）；Editor 的 cleanup 只在自己是当前 epoch 时复位全局 `composing`（重建时旗子归新编辑器）。`BackHandler.on` 要在 effect 外面注册，否则组合期间监听器消失。
- **vim 模态编辑（M2，设计已定）**：设置项 `vim`（默认关）开启后页面获得两个模式：**normal**（渲染态 + 块选择）和 **insert**（现有块编辑，即 editing()）。Logseq 原生只有块内 Esc，无块间导航；键位参考 Obsidian vimrc 插件 + org-mode 的块导航，保留 Logseq 块概念：
  - 移动：`j`/`k` 上下选块（可见行，跳过折叠）、`h`/`l` 父块/首子块、`za` 折叠/展开、`gg`/`G` 首末块。
  - 进入编辑：`i`/`enter` 编辑当前块（caret 在末尾/行首）、`o`/`O` 下/上新建同级块并进入 insert；`esc` 从 insert 回 normal（现有 endEdit）。
  - 结构操作全部映射现有 `Rike.*` 同步操作：`dd` 删块、`>>`/`<<` 缩进/反缩进、`shift+j`/`shift+k` 块上/下移、`space` 切 TODO、`u`/`ctrl+r` 撤销/重做、`/` 打开搜索。
  - 实现接缝：normal 模式 = 块选择 signal（rel+id）+ 无编辑器挂载，keymap 绑定在页面级；操作后用返回的 `focus` 更新选择。**NSH keymap 无 chord（多键序列）**：`dd`/`>>`/`<<`/`za`/`gg` 需应用层 pending-key 状态机（一个 pending 变量 + 超时清零），spike 先验证。
- **Enter 走 `onSubmit`**：`submitBehavior="submit"`，先经过输入法，Shift+Enter 换行。
- **Linux 上 `onChange` 会延迟**：到主循环空闲、submit 或失焦才送达。所以结构操作前要先 `flushDraft`，退出前先等 100 ms。
- **Backspace 只在输入为空时上报**。编辑器只在文本为空时绑定 Backspace，回调里再检查一次。
- **没有同步读 Input 文本/光标的 API**：只有 `onChange` / `onSelectionChange` 镜像（UTF-16 偏移）。拆块走 `onSubmit`（它晚于 change/selection）。
- **窗口自带的关闭没有钩子**：X、GTK 的 Ctrl+Q 不能“退出时保存”，所以仍要编辑即去抖落盘。`ctrl+q`、`alt+F4`、`F11` 是保留键。我们的退出调 `Window.close()`（`nsh/desktop`），先保存再关。
- **Input 没有 preventDefault 的按键钩子**。
- **TextView 自定义行内节点不在文字流里**：把 `[[x]]` 改写成 `[x](sx-page:...)` 链接，用 `onLinkPress` 处理点击；公式用 `renderMath` 回调（`TextViewMath{tex,display}`）。
- **扁平渲染**：所有可见块渲染成一个以块 uuid 为 key 的列表，层级换算成左边距。不要在嵌套的 `Each` 里重排，会卸载 Input，丢失焦点和未送达的文本。
- **程序触发的 `blur()`** 会触发 `onBlur`，别因此退出编辑态。
- **`Checkbox` 没有外观**：要给 `indicatorClass`（尺寸、边框）和 `indicator`（对勾）。
- 窗口在 Wayland 无头 sway 里没有指针，输入法冒烟只能用键盘（见“输入法”）。

**TSX / 原生模块**

- `ui/nsh.json` 必须有 `"rust": "../crates/rike-native"`，否则 `nsh build`/`check` 不认原生模块。`#[native_module("rike")]` 生成 crate 根的 `registry()`，宿主用 `set_ui_sets(vec![nsh_base::registry(), rike_native::registry()])` 注册。
- 同步函数直接返回，`async fn` 返回 Promise，`Result<_, String>` 的 `Err` 会 reject，panic 会被捕获。`#[derive(JsObject)]` 的 DTO 字段在 TS 里是驼峰。原生模块缺文档注释只会在 `nsh test` 时给一条无害的 warn。
- 原生状态（SESSION、监听器）是**引擎线程的线程局部**。测试文件和被测应用不在同一个线程，所以测试里不能直接 `Rike.page(...)`（报 “no vault is open”），要通过界面或 dev 钩子（见“测试钩子”）。
- `Callback` 的 `Stop` 只撤销自己的注册；常驻 `pump()` 用 epoch 让旧循环退出。
- 无头测试：窗口很窄，被裁剪的文字不会出现在 `app.texts()` 里，长提示查按钮等短文本；没有输入法组合；`app.keyboard.press("ctrl+k")` 走 keymap；`app.type` 往输入框末尾打字；`app.value(sel)` 读输入框；`app.back()` 触发 `BackHandler`；`app.wait(ms)` 用真实时钟。headless 不上报 `onFocus`/`onBlur`，所以靠 `Escape` 结束编辑。输入法和真实焦点只能手测（`scripts/wayland-ime-smoke.sh`）。
- 文件内容的断言用 `Rike.devRead(rel)`（`ui/testkit.ts` 的 `untilFile`/`file`）。不要给应用加 `capabilities.fs`。

## 许可证红线

- 不参考、不复制 AGPL 项目的代码：Logseq、tine、lsdoc、mldoc。
- docu.md（markdown-viewer-extension）是 GPL-3.0，只作设计参考。
- MiaoYan 为 MIT，可参考设计。
- 本仓库是 MIT OR Apache-2.0。新增依赖不得是 GPL/AGPL（`cargo metadata` 查 `license`）；NSH 是 Apache-2.0，不传染。发布二进制要附 `typst-assets` 内嵌字体的署名和 NOTICE。

## 版本控制

- 使用 git。不要擅自 commit 或 push。
- 不要覆盖、删除用户的笔记库或 `fixtures/` 之外的数据。

## 里程碑边界

- **M1**：大纲、日志、双向链接、搜索、本地文件监听、冲突安全。
- **M1 不做**：远端同步、HawDB、`{{query}}`、属性 schema、未链接引用、页面重命名、最近页面、多行块的 Up/Down、非空块行首 Backspace 合并、复制块引用、图谱视图、白板、插件、多窗口、图片附件、导入导出。
- **公式渲染**（`rike-native/src/math.rs`，实验记录在 `docs/spikes/typst-math.md`）：TextView 解析 `$…$`、`$$…$$`、`\(…\)`、`\[…\]` 并区分行内和展示，`ui/src/math.tsx` 是它的 `renderMath` 回调（调 `Rike.renderMath`）。
  - 流程：TeX → `mitex::convert_math` → 加 `PRELUDE`（mitex 为旧版 Typst 写的名字，新 Typst 里要自己定义）和 `RENAMED` 替换 → `typst::compile` → `typst_render` → PNG → `data:` URI → `<Image src>`。字体用 `typst-assets` 内嵌的，不读系统字体。
  - 行内公式的基线：页面里在公式前放一个零宽、透明、7 pt 的 `H` 标记，读它的 y 当基线（分式、纯文字公式在 Typst 里拿不到可靠基线）；`style.translate: [0, height - baseline]` 把图片放到文字基线上。`#set text` 要带 `top-edge/bottom-edge: "bounds"`，否则行内公式高度被钳在文字行高里，分式被裁。
  - 渲染同步执行（毫秒级），线程内缓存键为 `(tex, display, dark)`，失败也缓存，有字节上限。宿主 `main` 在 `logging::init` 之后调 `math::prewarm()`，在另一个线程预热字体和 Library。编译包在 `catch_unwind` 里，panic 当作失败，回退为等宽 TeX（`testID="math-fallback"`，测试里写 `id:math-fallback`）。
  - 加新的 mitex 兼容定义：在 `math.rs` 的 `PRELUDE` / `RENAMED` 里加，用 `cargo run -p typst-math -- scan <vault>` 回归。
  - 依赖版本全部精确锁定（`mitex`、`typst*` =0.15.x）：Typst 的 API 每个小版本都会变。不要用 `tex2typst-rs`（GPL-3.0）。
  - `rike_core::inline` 的 tokenizer 会跳过 `$$…$$`、`\(…\)`、`\[…\]`（可跨行，遇到空行放弃），公式里的 `[[x]]`、`#x` 不算引用。
  - 根 `Cargo.toml` 里 `[profile.dev.package."*"] opt-level = 2`：dev 构建里的 Typst 太慢；代价是首次构建约 7 分钟。
- **退出与切库**：见“界面与原生模块”（`Session::leave`，侧栏按钮 `id:quit` 和 `Ctrl+W`）。
- **M2**：属性类型与 Class 模板（借鉴 Atomic Data 的概念，不嵌入 `atomic_lib`）、未链接引用、页面重命名、简单查询、vim 模态编辑（见下）、中文全文检索。
- **M2 索引后端路线**（2026-11 评估 HawDB/DuckDB/Turso/seekdb 后定）：① 增量失效已落地：`Index::upsert_page/remove_page` 返回 `PageDelta`，session 累积成 `IndexChanges`（changed/removed rels + 被引用页键）随 `Event::IndexVer` 发给 TS，coalesce 按集合并集合并，TS 侧 backlinks 用 keys 脏检查、搜索用 ver；② 中文全文检索用 **tantivy + jieba** 直接接在 MemIndex 侧（spike 已验证：建 10k 块 93ms、单块增量 commit 53ms、查询全在亚毫秒级，结论见 `docs/spikes/tantivy-fts.md`）；③ 再用同一 `Index` 接缝 spike turso（纯 Rust、活跃，但 FTS 实验性）和 hawdb（同步 facade 默认可用、图+FTS 一体，但 v0.30、API 常变、单公司自用库）各一个最小实现，用同一基准（万页 upsert + 中文查询）定默认后端。DuckDB（OLAP 不匹配、CJK 弱）和 seekdb（无 Rust SDK、向量定位）排除。所有后端都只是可重建的派生缓存，Markdown 是唯一数据源。
- **M3**：OpenDAL 远端同步（webdav/s3、三方合并、tombstone）；RichEditor 实验（工程已删除，结论在 `docs/spikes/rich-editor.md`）是：：`[[x]]` 往返会被转义，要先给 schema 加 `pageRef`/`tag`/`blockRef` 节点才能考虑换编辑器。
