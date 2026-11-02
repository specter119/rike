# 原生模块的导入、类型与打包

结论：**`nsh.json` 必须写 `"rust"`，指向 `rike-native` crate。** 计划里原本设想"不写 `"rust"`，手写 golden `.d.ts`"，实测走不通，改为用 `"rust"`，类型由 CLI 自动生成。没有致命问题。

| 项                                                    | 结果           | 证据                                                                                                                                                                         |
| ----------------------------------------------------- | -------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| T1 没写 `"rust"`，TS 里 `import { Rike } from "rike"` | 失败           | `nsh build`：`could not load module 'rike' (only registered native modules and bundle chunks can be imported at run time)`；`nsh check`：`TS2307: Cannot find module 'rike'` |
| T1 `"rust": "<工作区外的 crate 绝对或相对路径>"`      | 通过           | 用 `spikes/tsx-pump` 试：`nsh check` 通过，`nsh build App.tsx` 产出 `.nsh/out`（js 386 B + bytecode 904 B）                                                                  |
| T2 类型声明                                           | 通过，自动生成 | `.nsh/types/probe/native.d.ts`（由 `nsh check` / `nsh build` 描述 crate 后写出）。不需要手写 golden 文件，也不需要 cargo test 去生成                                         |
| 构建位置                                              | 通过           | 生成的工作区在 `<app>/.nsh/rust`；产物目录是 `CARGO_TARGET_DIR`，没设就是 app 自己的 `target/`                                                                               |
| 缓存后的耗时                                          | 通过           | `nsh check` 273 ms，`nsh build` 655 ms（crate 已编译过）                                                                                                                     |

## 对 Rike 的影响

- `ui/nsh.json` 写 `"rust": "../crates/rike-native"`。`nsh check`、`nsh build`、`nsh test`、`nsh dev` 都会先描述并编译这个 crate，所以 **rike-native 里的 Typst 会被这条路径再编译一遍**（描述程序、headless runner 各一次）。
  - 缓解：设 `CARGO_TARGET_DIR` 到仓库的 `target`（脚本里统一设置），Typst 的编译产物能被复用的部分会复用。首次仍然慢（几分钟），之后增量。
- `.d.ts` 不再入库，`ui/.nsh/` 加进 `.gitignore`（已加 `/.nsh/`，`ui/.nsh/` 也需要匹配，见 `.gitignore` 的 `.nsh/` 规则）。
- 计划 S3（"不写 `"rust"`、手写 golden `.d.ts`"）作废。
- 访问 crates.io 时有一次 TLS 断开（网络），之后用 `CARGO_NET_OFFLINE=true` 重试成功。`nsh` 在依赖已缓存时不需要联网，脚本里可以加这个环境变量。

## 未验证

- T3 headless Rust 测试里 `compile_tsx` + `Headless` + 选目录对话框（`on_pick_paths`）和手动时钟；
- T4 `compile_tsx` 只放在 dev-dependencies，`cargo tree -e normal` 不出现 rolldown/oxc。

`nsh test` 已经能跑 TSX 的 headless 测试（见 `spikes/tsx-pump`，3 个测试通过），所以 TSX 端到端测试不一定要靠 Rust 的 `compile_tsx`。选目录对话框仍然需要 Rust 侧验证。
