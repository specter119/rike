# 迁移到 NSH TSX：结论

日期：2026-11。NSH rev `a5036af`，`nsh` 命令行 0.3.0。细节见同目录的 `tsx-api-notes.md`、`tsx-pump.md`、`tsx-types.md`、`tsx-host.md`、`tsx-ui-contract.md`。

## 结论

可以迁，且已经迁完：界面是 `ui/` 里的 TSX，数据和保存在 `crates/rike-native`，旧的 Rust 界面（`ctx.rs`、`ui.rs`、`--legacy-ui`）已删除。

## 实测通过的关卡

| 项目                                                     | 结果                                                                                                                                             |
| -------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------ |
| 原生模块（`#[native_module]`）同步/异步/DTO/回调         | 通过。`"rust"` 键在 `ui/nsh.json` 里必填。                                                                                                       |
| 常驻 `pump()` 任务 + `onEvent` 回调                      | 通过（`tsx-pump.md`）。旧循环靠 epoch 退出。                                                                                                     |
| 无头测试（`nsh test`）                                   | 通过，26 个测试：编辑、拆块缩进、Backspace、TODO、撤销重做、复选框、外部修改、冲突拒绝退出、搜索、反链、后退前进、公式、欢迎页、选库、恢复页面。 |
| 块编辑器 `Input` 在行重建时保持文本                      | 通过。编辑器用草稿模式，`defaultValue` 只在挂载时读一次，Rust 改文本时用新 `epoch` 重建。                                                        |
| 输入法（无头 sway + 独立 fcitx5，`GTK_IM_MODULE=fcitx`） | 通过。块编辑器和搜索框都有预编辑、候选窗和上屏（\`scripts/wayland-ime-smoke.sh block                                                             |
| 公式（`renderMath`）                                     | 通过。显示公式居中，行内公式按基线对齐，坏公式回退成 TeX 源码。                                                                                  |
| `pick_paths`（文件夹对话框）无头行为                     | 不会崩溃：无头里按“打开文件夹…”后欢迎页保持原样。真实对话框只能手测。                                                                            |
| 真实窗口冒烟（无头 sway，嵌入 bundle）                   | 通过：渲染、公式、搜索、块编辑。                                                                                                                 |

## 迁移中踩到的坑

- 宿主的 `nsh-platform-linux` 要开 `js` feature；不开时窗口空白，日志只有 “runs Rust apps only”。
- `Checkbox` 没有外观，没给 `indicatorClass`/`indicator` 时看不见，测试也找不到。
- 持有 `Handle::enter()` 的 guard 跨 `.await`，多个任务交错时在 tokio ≥ 1.54 会 panic（guard 乱序销毁）。改成每次 poll 进入运行时（`session::in_runtime`）。
- 原生状态是引擎线程的线程局部：测试文件里调不到被测应用的 Session。文件内容用 dev 钩子读，不给应用加 fs 权限。
- 在 Journals 视图里 Ctrl+Z 需要知道撤销哪一页：记住最近一次结构操作的页面（`undoTarget()`）。
- `nsh dev --server-only` 监听 `0.0.0.0` 且无绑定选项；宿主只接受 loopback 的 `--dev-server`。

## 没做 / 已知限制

- 渲染性能没有做 300 行以上页面的专门测量；Typst 首次编译公式较慢（字体预热在另一个线程）。
- 真实 GTK 窗口里的鼠标点击路径只在无头测试里验证过（无头 sway 没有指针）。
- ibus 等其他输入法没有验证。
