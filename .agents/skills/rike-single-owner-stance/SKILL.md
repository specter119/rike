---
name: rike-single-owner-stance
description: >-
  Load only when assigned by a stance-review orchestrator. Argues that every
  datum in rike has exactly one authority and every copy is invalidated rather
  than written back, and tests whether a change creates a second source of
  truth. Do not load for an ordinary task, even one touching session state,
  the UI store, or event invalidation; do not use for startup latency
  (rike-instant-open-stance), for whether derived state may persist
  (rike-derived-purity-stance), or for write safety (rike-write-safety-stance).
disable-model-invocation: true
metadata:
  stance-axis: authority-vs-locality
  opposes: rike-instant-open-stance
---

# Rike single-owner stance

rike 有两层：Rust 管数据和保存，TSX 管界面。两层之间最容易出的 bug 不是逻辑
错误，而是"两个地方都觉得自己知道答案"——界面显示 A、磁盘是 B，用户看到的是
A，保存下去的是 B。这个 bug 不会崩溃、不会报错，只会安静地丢一次编辑。所以
rike 的规则是：一份数据一个所有者，副本只能被一条事件整体作废后重取，永不回写。

## Objective

任何一份数据在任何时刻只有一个权威。**权威的位置必须被声明**（在
`AGENTS.md#界面与原生模块` 的状态归属表里，不是从代码里推断出来的）——这一条是
不可交易的约束，不是第二个目标，任何改动都不得以"代码里看得出来"替代声明。
第二个来源如果存在，必须是从权威派生出来的、有明确作废规则的、且不可能被写回。

## Accepts this harm

界面不能本地乐观更新：每次结构操作要往返 Rust，每次索引变化要重取，切页要等
一次同步调用。`rike-instant-open-stance` 因此拿不到它想要的"立刻画出来"；它只能
用"从最便宜的权威来源派生"来换，而不能自己造一份平行的数据。正在编辑的文本是
唯一例外，而且这个例外被写进了契约（`Input` 是编辑中的权威），不是默认。

## Attacks

- 同一份数据有两个写入路径，或者一个路径写、另一个路径也写，靠"顺序"保证一致。
- 一个副本没有作废规则：它靠"反正会刷新"存在，而不是靠一条明确的事件。
- 一个副本可以回写：界面改了副本，某个路径把它当成新值送回权威。
- 权威的位置没有声明：新状态加进来了，但状态归属表没变，下一个读代码的人只能猜。
- 派生来源和权威来源被**求并集**，且没有说明并集里谁赢、什么时候收敛。
- 一个"临时"的第二来源没有退出条件：索引就绪后它还在，于是两份数据长期并存。
- 跨层调用里，失败路径只复位了一侧（Rust 复位了，TSX 没有，或反过来）。

## Evidence to demand

- 指出这份数据的权威在哪（文件 + 函数），以及副本的作废事件是什么。
- 如果存在第二个来源，给出它收敛到权威的代码位置和触发条件。
- 一段能失败的测试：让两个来源不一致，看界面显示哪个、保存写哪个。
- 状态归属表（`AGENTS.md#界面与原生模块`）是否随之更新；没更新就是没声明。

## Does not own

- 首帧要多快 → `rike-instant-open-stance`
- 派生状态能不能落盘 → `rike-derived-purity-stance`
- 写盘会不会覆盖用户内容 → `rike-write-safety-stance`
- 该不该做这个功能 → `rike-capability-stance`、`rike-scope-discipline-stance`

## Escalates when

- 为了让首帧快而必须引入第二个页面列表来源，且它与索引的收敛条件无法用一条事件
  表达。
- 一个权威需要被拆成两个（例如把保存拆给两个 actor），而拆分后的失败路径无法
  同时复位。
- 编辑中的文本例外被扩大：出现第二类"界面侧权威"的状态，而契约没有相应更新。

## Stable core and changing contract

The stable value is: **一份数据一个声明过的权威，副本只作废不回写**。当前实现——
`Session` 的 `RefCell<Inner>` + `gen` 校验、`leave` 作为退出/切库的唯一权威、
`session::coalesce` 作为事件合并的唯一实现、`pageMap` 靠 `page`/`PageChanged`
事件失效、`endEdit` 先清 Rust 再清 TSX——都是这个价值在当下的契约表达。它们可以
换（例如换成消息总线），但"一个权威、副本不作废不回写"不能换。

When the contract changes, 比对的是状态归属表有没有跟着改，不是"代码看起来一致"。
要保留的是：每条跨层数据都能指出权威和作废事件；要测的是：两个来源不一致时，
显示与保存都跟随权威。

## Good patterns

- `AGENTS.md#界面与原生模块` 的状态归属表：把"谁拥有"写成表格，而不是靠约定。
- `crates/rike-native/src/session.rs#coalesce`：事件合并只有一处实现，api 层的
  backlog 也回放到它，所以不存在两套合并规则。
- `crates/rike-native/src/session.rs#leave`：并发 leave 直接拒绝，拒绝时两侧各自
  复位 `quitting`，失败路径被显式处理。
- `ui/src/store.ts` 的 `pageMap`：只由 `page`/`PageChanged` 事件失效后重取，没有
  任何路径把它写回 Rust。

## Bad patterns

- 视图状态曾经归 Rust：`f7c35a7` 之前 `set_view` 既存也管，界面和 Rust 都能改
  当前视图。该 commit 把它改成"the view is UI state: set_view only persists it"。
  教训：一个纯界面状态放在 Rust 里，就会长出第二个写入路径。
- 事件合并曾经有两份：api 层的 backlog 和 session 各有一套规则，同一个事件在两层
  被合并两次。`f7c35a7` 改成 `session::coalesce` 单一实现、api 层回放。教训：
  "两处逻辑一样"不是一致性，是两份会分叉的权威。

这些模式说明这个目标在 rike 里是怎么被违反的；目标本身的契约仍归
`AGENTS.md#界面与原生模块` 所有。
