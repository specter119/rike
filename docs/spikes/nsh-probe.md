# NSH 交互门禁探针（M1 gate）

> 这个探针工程（`spikes/nsh-probe/`，纯 Rust UI）已在 0.1.0 发布前删除，UI 改用 TSX；结论保留在这里，下面的命令已不能运行。

探针代码曾在 `spikes/nsh-probe/`。
NSH rev `a5036af48ddc193b5023ece9b9196c76b8f8d3f5`；Linux GTK4 4.24 + Wayland；Rust 1.99.1。

结果字段：PASS / FAIL / PARTIAL / UNVERIFIED-needs-human。
`源码` 表示结论来自 `~/.cargo/git/checkouts/nsh-9218b667fc8c8325/a5036af/` 的代码阅读，`headless` 表示由 `cargo test` 验证，
`人工` 表示必须在真实窗口里由人观察。

## 结论一览

| 问题                                                         | 结果                            | 证据  | 变通                                                           |
| ------------------------------------------------------------ | ------------------------------- | ----- | -------------------------------------------------------------- |
| (a) keymap 回调里同步读 Input 当前文本/光标                  | FAIL（源码）                    | 见 §a | 结构操作前用“先取实时值”路径：见 §a 的 workaround              |
| (b) `on_submit` 时 `on_change`/selection 是否已送达          | PASS（headless）                | 见下  | 在 `on_submit` 里读镜像拆块；GTK 下 `on_change` 延迟见 §a      |
| (c) IME 组合期间卸载 Enter/Backspace/Esc/Tab/方向键          | 源码：管道存在；触发顺序 待人工 | 见 §c | 用 `on_composition_change` 挂/卸 keymap                        |
| (d) 窗口关闭 / 退出钩子                                      | FAIL（源码，`run_app` 路径）    | 见 §d | 每次编辑即落盘；或用 `AppView` 自建 GTK 窗口接 `close_request` |
| (e) 剪贴板写入 API                                           | PASS（源码）                    | 见 §e | `set_clipboard` / `clipboard_string`                           |
| (f) 扁平 keyed `each` 重排/改层级后 Input 是否保焦点与文本   | PASS（headless）                | 见下  | 用稳定 `BlockId` 作 key；扁平列表 + 缩进 padding               |
| (g) 200 个 TextView 的挂载与滚动性能                         | PASS（headless，debug 构建）    | 见下  | 日志流用 `virtual_list`；真实滚动手感需人工                    |
| (h) 程序化 `blur()`/`focus()` 触发 `on_blur`；新建块立刻聚焦 | PARTIAL（headless）；GTK 须人工 | 见下  | 新块在下一 tick（`set_timeout(ZERO)`）取 ref 再 `focus()`；用 `self_blur` 标记忽略自己触发的 blur |
| (i) `when(signal, ..)` 条件挂载 keymap 绑定                  | PASS（headless）                | 见下  | `when(..)` 必须作为元素返回，不能丢弃（见下）                  |

## GATE VERDICT

**PASS 的前提是接受以下约束，M1 按此开工（有条件通过）。**

- 结构操作（Tab / Shift+Tab / Enter 拆块 / 方向键）可用：keymap 绑定在 `when(not_composing, ..)` 里；
  回调只读 `on_change` / `on_selection_change` 镜像（headless 下镜像是当前的；GTK 下 `on_change` 延迟到空闲，
  所以 **拆块走 `on_submit`**，它在 change/selection 之后到达）。
- 退出前无法拦截，必须“编辑即去抖落盘”；`Ctrl+Q` 直接由平台退出，所以应用层不保证 flush（见 §d）。
- 光标 / 文本不可同步读：“Backspace 合并”这类需要光标位置的功能，M1 只做“空块 Backspace 删除”。
- 需要人工确认的项（不阻塞开工，但在 M1 验收时必须过一遍）：
  1. fcitx5 中文输入下 Enter / Esc / Tab 的先后（§c）；
  2. GTK 真实窗口里程序化 `blur()` 是否上报 `on_blur`、`focus()` 是否生效（headless 不上报）；
  3. 200 行以上日志流的真实滚动手感。

## §a 同步读取 Input 的当前文本与光标

**结论：FAIL。没有同步 getter。**

源码证据：

- `crates/nsh-core/src/element/element_ref.rs` 的 `impl ElementRef` 全部公开方法是
  `node`、`focus`、`blur`、`set_accessibility_focus`、`animate_layout`、`set_selection(start, end)`、
  `scroll_to` / `scroll_to_offset` / `scroll_to_end` / `scroll_to_index`、`zoom_to`、`reload`、`go_back`、
  `go_forward`、`stop_loading`、`inject_java_script`、`post_message`、`seek`、`command`、`measure`、`transition_time`。
  只有 **写**（`set_selection`）和 `measure`，**没有** `text()` / `selection()` / `value()` 读取方法。
- `nsh-core` 内部确实保存了实时文本：`core/mod.rs` 的 `struct InputState { value: String, .. }`，由
  `Core::input_changed(id, text, count)`（`core/mod.rs:1669`）在**平台事件到达引擎时**更新，
  注释写明 “keeps the text for layout and for stale-value detection”。
  但 `InputState` 是私有结构、字段私有，`Core::input_text(..)` 也是私有方法，**Rust 侧没有公开访问器**。
- 引擎把 `ChangeText` 事件在派发给应用前就写进 core（`nsh-engine/src/lib.rs:2536` 附近），
  所以“core 知道”但不对外暴露。

`set_selection` 能写选区但读不回来，因此也无法用“设置后再观察变化”反推光标位置。

workaround（M1 需要接受）：

1. **文本**：单一真相仍是 `on_change` 镜像到 signal。GTK 后端 `on_change` 延迟到主循环空闲
   （`nsh-platform-linux/src/text_input.rs`：`changed()` 里 `glib::idle_add_local_once(flush_change)`），
   所以 keymap 回调可能看到**上一次**的文本。
1. **光标**：只能靠 `on_selection_change` 镜像（UTF-16 偏移）。
1. 结构操作（Tab/拆块/TODO）前**不能**信任镜像。可行做法：
   - 键位改为在 **Input 自身的按键钩子**里触发，而不是窗口 keymap —— 但 `on_key_press` 是**只观察**
     （`EventKind::KeyPress` 由平台在文本变化前上报，无 preventDefault），按 Tab 仍会被 GTK 输入法/焦点系统吃掉。
   - 或者沿用 “blur-then-next-tick” 模式：先 `ref.blur()`（会触发 `on_blur`，`on_blur` 前平台会 `flush_change()`，
     见 `text_input.rs::focus_changed` → `self.flush_change()`），拿到最终文本，再用**程序化写回标记**避免把页面踢出编辑态。
   - 结论：**结构操作需要“先写回再执行”的两步，不能同步取实时值**。

## §c IME 组合期间的按键处理（需人工验证触发顺序）

源码证据（`crates/nsh-platform-linux/src/text_input.rs`）：

- `connect_preedit_changed` → `preedit_changed(text)`：非空即 `send_composition(true, text)`；
  空串走 `composition_ended()`，用一个 `glib::idle_add_local_once` 在空闲时 `flush_change()` 后
  `send_composition(false, "")`。也就是说**组合结束事件在提交文本的 `onChangeText` 之后**（源码注释明确写了这一点）。
- 回车：multiline 且 submits 时，先 `view.im_context_filter_keypress(&event)`；**输入法消费了就不 `submitted()`**
  （注释：“a return that commits a composition (CJK input methods) submits nothing”）。
  所以 Enter 被输入法吃掉的场景，NSH 层面根本不会触发 `on_submit`。
- Escape：`key_press("Escape")` 直接上报，**没有**经过 `im_context_filter_keypress`。组合中按 Esc 会既取消组合又上报 Escape。
- Backspace：仅当编辑器为空时才上报 `key_press("Backspace")`（`input.editor.borrow().is_empty()`），
  非空时不上报 —— 与 AGENTS.md 现有“Backspace 在位置 0 不上报”的描述需要修正为“仅在文本为空时上报”。
- Tab：`new_editor` 里 `view.set_accepts_tab(false)`，Tab 不被输入法/编辑器接受，会冒泡到窗口 keymap。

⇒ 卸载 Enter/Backspace/Esc/Tab/方向键绑定在**机制上可行**（`on_composition_change` + `when`），
但**组合事件与按键事件在同一主循环内的先后**只能人工在 fcitx5 下观察。

## §d 窗口关闭 / 退出钩子

**结论：FAIL（`run_app` 路径下没有回调）。**

源码证据：

- `crates/nsh-core/src/desktop.rs` 的 `DesktopCommand` 只有**命令**（`Close`、`SetTitle`、`ToggleMaximize` …），
  没有任何 “窗口即将关闭” 的**事件**；`EventKind` 全表（`crates/nsh-core/src/ops.rs:163-222`）里也没有 close/quit 事件。
- `nsh-platform-linux/src/app.rs`：
  - `install_shortcuts`：`app.quit` 绑 `<Control>q`，激活时**关闭全部窗口**；`<Control>w` 用 `NamedAction("window.close")`
    挂在窗口的 **bubble** 阶段（注释：给应用自己绑 Ctrl+W 的机会）；`F11` 全屏。
  - `unsafe_keep`：`window.connect_destroy(...)` → `view.stop()`。**这是平台内部行为，没有暴露给应用。**
- `Keystroke::reserved(Os::Linux)`（`nsh-core/src/keymap.rs:429`）：`ctrl+q`（“it quits the app”）、
  `alt+F4`（“it closes the window”）、`F11` 全屏 —— 这三个**不能** `on_key`（会 panic）。
  `ctrl+w` **可以**绑定。

⇒ 退出时“先写完所有待保存内容”不能靠退出钩子。M1 必须：

1. 编辑就去抖 + 立刻经 vault actor 落盘（不允许“只在退出时保存”）；
1. 若要拦截关闭，需要自己用 `nsh_platform_linux::AppView` 嵌进自建 `gtk::ApplicationWindow`，再
   `connect_close_request`（探针未实现，属 M3 embedding 路径）。

## §e 剪贴板

**结论：PASS。**

- `nsh-core/src/element/ambient.rs:447` `pub fn set_clipboard(text: impl Into<String>)`（写文本）。
- 同文件 `clipboard_string() -> Answer<String>`、`clipboard_has_string() -> Answer<bool>`（读，需在 task 里 await）。
- 均在 `nsh_core::prelude` 中重导出（`prelude.rs:225` 列出 `clipboard_has_string, clipboard_image, clipboard_string, ... set_clipboard`）。
- 平台实现：`Command::SetClipboard(String)`（`ops.rs`）→ GTK 侧落到 `crate::system`。

## 详细记录（运行时）

headless（`cargo test -p nsh-probe --test probe -- --test-threads=1`，7 个测试全部通过）：

- (b) `submit` 日志里 `text="abcd"`、`mirror_live="abcd"`、`mirror_sel=4-4`，且 change 行在 submit 行之前。
- (f) 重排、缩进后，聚焦的 Input 仍挂载、文本不丢；key 用稳定 id。
- (g) 200 行 `virtual_list` 挂载 0.2 ms（计数器），整个按键到出现约 100 ms（debug 构建）。
- (h) `btn-blur` 发出 `Command::Blur`，headless 平台**不会**上报 `on_blur` / `on_focus`；新块在下一 tick
  `ref mounted=true`。headless 命令日志只保留发出它的那一帧，所以无法事后断言 Focus 命令。
- (i) 条件绑定：输入非空时 backspace 不触发，清空后触发，再输入后又消失，再清空又回来。
  **坑**：`when(..)` 返回的是 `If` 元素，若作为语句丢弃则永远不会挂载（探针第一版就是这样，绑定一次也没触发）。
  必须 `view().child(when(..))` 或作为闭包返回值。
- headless 的 `press_key` 只走 keymap，不改文本；`clear` 会触发 `on_change`；`run_for` 需要手动时钟，真实时钟用 `wait`。
