# 宿主：内嵌 bundle、开发服务器、选项

> 注：实验工程 `spikes/tsx-host/` 已在迁移完成后删除；结论保留在这里，对应行为现在由 `ui/*.test.tsx` 覆盖。

来源：读 NSH 源码（`nsh-platform-linux/src/{app,dev}.rs`、`nsh-engine/src/lib.rs`）加 `nsh build` 实测。端到端的窗口运行（sway 里起内嵌 bundle 的二进制）要等 Phase 3 的真实宿主，到时补验证。

| 项                            | 结果                     | 证据                                                                                                                                                                                                                                                                                                                   |
| ----------------------------- | ------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| H2 `nsh build App.tsx` 的产物 | 通过                     | `.nsh/out/bundle.js`（源码）、`bundle.nshc`（QuickJS 字节码）、`bundle.js.map`、`app.json`                                                                                                                                                                                                                             |
| H2 内嵌                       | 可行，未跑窗口           | `AppSource::Bundle(Bundle)`；`Bundle::source(String)`（安全）或 `unsafe { Bundle::bytecode(Vec<u8>) }`（要求字节码和运行时版本一致，同一 rev 构建就一致）；也可 `unsafe Bundle::read_trusted(path)` 读 `.nshc`。`.with_app_id(..)`、`.with_capabilities_json(..)`、`.with_source_map(..)`、`.with_policy(..)` 可链式加 |
| H2 `asset_base`               | 不需要，除非用到资源文件 | `AppOptions.asset_base: Option<AssetBase>`，只给 `AppSource::Bundle` 用；Rike 没有打包的资源文件，`None`                                                                                                                                                                                                               |
| H1 开发服务器                 | 可行，未跑窗口           | `AppSource::DevServer("ws://host:port/ws")`，需要 `nsh-platform-linux` 的 `dev` feature（它拉进 `nsh-dev` 和 `nsh-testing`）；每次重建服务器推送 bundle。服务端 `nsh dev --server-only`，已知监听 `0.0.0.0:8790`                                                                                                       |
| H3 `Limits::standalone()`     | 通过（文档）             | `AppOptions.limits` 默认是嵌入宿主用的 `Limits::default()`；开发宿主和 release runner 用 `Limits::standalone()`（无限制）。Rike 是独立应用，用 `standalone()`                                                                                                                                                          |
| H3 其他选项                   | 通过                     | `app_id`、`title`、`size`、`trace`、`fps`、`data_dir`、`appearance` 都在 `AppOptions` 里                                                                                                                                                                                                                               |
| H4 关窗口                     | 通过（沿用）             | Rust 侧 `nsh_core::element::desktop::close_window()` 已在旧 UI 用着（`ctx.rs`）；TSX 侧 `Window.close`。从 async 核心任务里调用 `close_window()` 待 Phase 3 验证                                                                                                                                                       |

## 对 Rike 的影响

- 宿主：release 用 `include_str!("../../ui/.nsh/out/bundle.js")` 加 `Bundle::source`（先不用 unsafe 的字节码；启动时间不够再换）。`ui/.nsh/out` 由 `scripts/build-release.sh` 里的 `nsh build` 生成，不入库。
- 开发：`dev` feature 才开 `AppSource::DevServer`，release 不开（`nsh-testing` 不进 release）。
- 开发服务器监听 `0.0.0.0` 且没有绑定参数：文档里写明，`scripts/dev.sh` 只在需要时起。
