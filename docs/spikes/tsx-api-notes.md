# NSH 原生模块 API 笔记（桥接层用）

来源：NSH rev `a5036af` 的 `docs/dev/rust-classes.md`、`docs/dev/rust-modules.md`、`crates/nsh-cli/src/templates/mixed/lib.rs`。这些是读文档得到的事实，尚未在 Rike 里实测（实测见 `docs/spikes/tsx.md`）。

## 声明方式

```rust
use nsh_core::prelude::*;

#[native_module("rike")]            // TS 里 import ... from "rike"
fn native(registry: &mut Registry) {
    registry.module::<Rike>();      // 也可 .class::<T>() / .register::<Component>()
}

#[module]                           // 模块 = 一组函数；struct 和 impl 都要标
pub struct Rike;

#[module]
impl Rike {
    pub fn render_math(tex: String, display: bool, dark: bool) -> Option<Formula> { .. } // 同步：TS 直接拿值
    pub fn set_draft(rel: String, gen: u32, ..) -> bool { .. }                         // 同步
    pub async fn open(path: String) -> Result<Opened, String> { .. }                   // 核心任务 → Promise
    pub fn quit(cx: &mut ModuleCx) { .. }                                              // op：按顺序执行，返回 () 为 void
    pub fn on_event(listener: Callback<Event>) -> Stop { .. }                          // 注册监听
}
```

- 每个 crate 只能有一个 `#[native_module]`。`#[module(flat)]` 让函数成为顶层导出而不是对象成员。
- 没有可见性（`pub`）的函数是辅助函数，不导出。
- 签名决定调用方式：`fn`（无 `cx`）同步；`fn(cx: &mut ModuleCx)` 是 op（返回值则为 Promise）；`async fn` 是核心任务（Promise）；返回 `Later<T>` 的非 async 函数在调用时同步启动、后台完成。
- `Result<T, E: Display>` → 同步抛异常或 Promise reject。panic 被捕获成 Error。
- 同步函数**不能**用信号、`dialog`、`theme()`、timer（响应式图不在那里，会 panic）。async 成员和 module 函数里可以。

## 数据类型

参数和返回值：数字、`bool`、`String`、`Option`、`Vec`、`JsonValue`、`#[derive(JsObject)]` 结构体、`#[derive(PropEnum)]` 枚举。`Vec<u8>` 是 `Uint8Array`。末尾的 `Option` 参数在 TS 里可省略。同步调用直接转换，不走 JSON。整数参数不接受小数。

## Callback / Stop

- `Callback<A, R = ()>`：JS 函数。可存进全局或 `thread_local`，可多次调用，丢掉最后一个 clone 才释放。
- `cb.post(arg)`：在引擎线程任意位置调用，排队，下一个 JS 回合执行，与其他 post 保持顺序。`cb.call_async(arg).await` 等结果。同步调用里才能用 `cb.call(arg)`（立即执行）。
- 返回 `Stop`（`Stop::new(f)`）：TS 得到 `() => void`。`f` 只执行一次：JS 调用它，或调用所属的响应式 owner / 热更新的模块 run 结束时，先到先执行。
- 回调属于 VM；VM 停了之后 Rust 手里的回调什么也不做。

## 其他

- `sys: System` 参数由调用填充（data 目录、Access、Quotas、`sys.locals.get::<T: Local>()` 每 VM 一份，VM 结束时 `Local::close`）。
- `CallCx`（同步调用里）：`cx.start(future)` 启动核心任务，`cx.on_cleanup(f)` 绑定到 owner 或热更新。
- 类型声明由宏生成到 `.nsh/types/<specifier>.d.ts`（`nsh` 命令行在 describe 时写出）。
- `nsh.json` 的 `"rust"` 接受 crate 目录或列表；每个 crate 一个 specifier。plan 里决定不写 `"rust"`，改用 `nativeModule("rike")`，这一点需要实测（见 Phase 0 的 P14）。
- 同一进程里的 Rust 宿主：`set_ui_sets(vec![nsh_base::registry(), rike_native::registry()])`。`registry()` 的名字由宏生成，待实测确认。
- 同一线程：JS 和 Rust 组件、同步成员、核心任务都在引擎线程上，所以 `thread_local` 的 Session 可以被它们共享；tokio 线程不能碰它。
