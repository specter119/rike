# 常驻泵任务探针

结论：**可行，没有致命问题。** Rike 的 Session 设计（常驻泵任务 + 事件回调 + main 自有 tokio runtime）可以照计划做。

代码：`spikes/tsx-pump/`（独立工程，不在 workspace 里）。运行：

```sh
export PATH=$HOME/.cargo/bin:$PATH TMPDIR=$HOME/.cache/sx-tmp
cd spikes/tsx-pump && nsh test     # 首次编译约 6 分钟，之后几秒
```

| 项 | 结果 | 证据 |
| --- | --- | --- |
| U1 常驻 `pub async fn run()` 里 `rx.lock().await.recv().await`，通道由普通线程（非 NSH）喂入，事件 `post` 到 TS 回调 | 通过 | 5 个事件全部到达，`count 5`、`last 14` |
| U1 通道接收端放在进程级 `OnceLock<Arc<tokio::sync::Mutex<..>>>`，可被新泵任务重新挂接 | 通过（锁一次一个持有者；重建泵任务的完整重载场景未模拟） | 代码见 `pump::run` |
| 换回调：先注册新回调，旧 `Stop` 后执行，旧 `Stop` 不会清掉新回调（注册 id） | 通过 | `swap` 测试，之后 3 个事件全部进新回调，旧回调计数为 0 |
| 无回调期间的事件缓存，下次注册时补发 | 通过 | `events buffered…` 测试，4 个事件补发 |
| 同步 `pub fn` 改 thread_local 并在同一次调用里返回嵌套 DTO（`Vec<#[derive(JsObject)]>`） | 通过 | `rows a1,b2` |
| 异步成员里 `handle.spawn(..).await`（main 自有 runtime，多线程） | 通过 | `Pump.onRuntime()` 返回 42 |
| 同步成员里用保存的 `Handle` 往 main 的 runtime spawn 任务，结果经通道回到回调 | 通过 | `spawnOnRuntime` 测试 |

## 实现要点

- `#[module] pub struct Pump;` + `#[module] impl Pump { .. }`，`registry.module::<pump::Pump>()`。
- TS 里 `Pump.run()` 返回 Promise，不用 await；它在引擎的核心任务里一直跑。
- 同步成员**没有** tokio 上下文：必须在进程级全局里保存 `Handle` 或 runtime，用 `handle.spawn`。
- `Stop::new(move || ..)` 在 TS 手动调用、或所属 owner 销毁时执行，先到先执行；用注册 id 保证旧 `Stop` 不清新回调。
- TS 里 `Pump.onEvent(cb)`：`cb` 在引擎线程执行。

## 未验证

- 热更新、完整重载时泵任务和 thread_local 的实际行为（headless 测试每个 `open` 是一个新引擎，没有模拟热更新）。设计上不依赖它们：Boot 数据和接收端都在进程级全局里，泵任务重启后重新挂接。真正的验证放到 Phase 3 宿主里手测。
