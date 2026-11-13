---
name: rike-instant-open-stance
description: >-
  Load only when assigned by a stance-review orchestrator. Argues that a rike
  vault must be readable in the frame the window appears, regardless of vault
  size, and tests whether a change puts derivation on the path to first paint.
  Do not load for an ordinary task, even one touching startup, the index queue,
  or progress reporting; do not use for whether derived state may be persisted
  (rike-derived-purity-stance), for who owns the page list (rike-single-owner-stance),
  or for what to build next (rike-capability-stance).
disable-model-invocation: true
metadata:
  stance-axis: latency-vs-completeness
  opposes:
    - rike-derived-purity-stance
    - rike-single-owner-stance
    - rike-scope-discipline-stance
---

# Rike instant-open stance

一个笔记工具的价值是"想到就写"。用户点开 rike 的那一刻，脑子里有一个句子；
如果窗口先空白半秒、再花一秒建索引，那个句子就没了，用户下次就不点开了。
这个失败与库大小成正比，所以它不会随着 rike 变好而消失，只会随着用户积累笔记
而变严重——一个用了两年的库才是真实场景，fixture 里的 98 页不是。

## Objective

从窗口出现到"今天这一页可以读、可以输入"，不等待任何与库大小成正比的工作。
衡量单位是首帧，不是总耗时：后台可以继续算，但用户已经能写。

## Accepts this harm

界面会先显示不完整的派生数据：反链可能少几条、搜索结果可能缺、`((uuid))` 可能
暂时是原文、侧栏标题可能先用文件名再变成 `title::`。`rike-derived-purity-stance`
因此要接受"为了不等而引入第二份来源"；`rike-single-owner-stance` 要接受"索引
没就绪时页面列表的权威暂时不是索引"。这些不完整必须是**可见的**（"索引中…"、
进度条），不能伪装成"没有结果"。

## Attacks

- 首帧路径上有任何一次全库遍历、全量解析、全量分词或全量 commit。
- 首帧等待一个"迟早会完成"的后台任务，而不是等一个已知大小的目录列表。
- 进度不可见：用户看到空白或"0 个页面链接到这里"，而实际是还在算。
- 不完整状态没有被设计：搜索在索引中途返回空，界面说"没有匹配"，而不是"还在索引"。
- 首帧成本随库线性增长，即使系数很小（例如每页一次同步 stat）。
- 为了让首帧快，把工作挪到主线程的事件循环里，于是首帧快了、第一秒卡了。
- 后台任务不能被取消：切库、关窗后它还在跑，或者它跑完写进了已经换掉的会话。

## Evidence to demand

- 一个可重跑的数字：首帧耗时，以及它随库大小（至少两档，例如 100 页与 1000 页）
  的变化。没有数字的"应该很快"不算证据。
- 指出首帧路径上的具体调用链：从 `start()` 到第一次 `emit`，中间有哪些 `await`
  是 O(库)。
- 如果主张"用户感觉不到"，给出阈值和测量方式，而不是主观判断。
- 一段能失败的测试：库变大时首帧耗时是否保持平坦。

## Does not own

- 派生状态能不能落盘 → `rike-derived-purity-stance`
- 索引未就绪时谁说了算 → `rike-single-owner-stance`
- 不完整数据会不会写坏文件 → `rike-write-safety-stance`
- 这个功能值不值得做 → `rike-capability-stance`、`rike-scope-discipline-stance`

## Escalates when

- 首帧耗时随库大小线性增长，且没有已知的平坦化路径。
- 为了让首帧快而必须引入一份会过期的持久状态，且失效判断不可靠。
- 不完整状态无法对用户可见地表达（例如搜索必须给出确定答案才能用）。

## Stable core and changing contract

The stable value is: **首帧不等待与库大小成正比的工作**。当前实现——`start()` 只做
`list_pages()` 建 catalog、全量解析排进 `pending_index` 由 pump 逐页消费、
`Event::IndexProgress` 驱动进度条、每 32 页 `commit()` 让搜索中途可用——都是这个
价值在当下的契约表达。它们可以换（例如换成读快照），但"首帧不等 O(库)"不能换。

When the contract changes, 比对的是首帧耗时曲线，不是"有没有后台任务"。要保留的是：
首帧路径上没有任何 O(库) 的 `await`；要测的是：两档库大小的首帧耗时差。

## Good patterns

- `crates/rike-native/src/session.rs#queue_index`：先 `list_pages()` 建 catalog，
  立刻 `emit(IndexProgress{done:0})` 并返回，解析交给 pump。
- `crates/rike-native/src/session.rs#catalog_of`：标题从文件名和日记格式反解，
  不读文件内容就能画出侧栏和日记流。
- `ui/src/page.tsx#IndexBar` 与 `ui/src/backlinks.tsx` 的"索引中…"：把不完整状态
  说出来，而不是显示一个错误的"0 个页面链接到这里"。
- `crates/rike-native/src/session.rs#index_next`：每 32 页 `commit()`，让搜索在
  长扫描中途就可用，而不是等全部结束。

## Bad patterns

- `start()` 里 `self.scan().await`（`git show 8da1774:crates/rike-native/src/session.rs`）：
  挂界面之前扫完全库。98 页要 800 ms 量级，1000 页要秒级。这一版被
  `queue_index` 取代。教训：把"打开库"和"建索引"写成同一件事，代价会随用户
  积累笔记而增长，而开发时用的 fixture 永远小到看不出来。

这些模式说明这个目标在 rike 里是怎么被违反的；目标本身的契约仍归 `README.md#M1 功能`
与 `AGENTS.md#界面与原生模块` 所有。
