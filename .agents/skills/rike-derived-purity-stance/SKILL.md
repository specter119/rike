---
name: rike-derived-purity-stance
description: >-
  Load only when assigned by a stance-review orchestrator. Argues that every
  byte rike stores beyond Markdown must be disposable and rebuildable, and
  tests whether a change makes derived state into an authority. Do not load for
  an ordinary task, even one touching the index, the cache directory, or
  startup; do not use for startup latency (rike-instant-open-stance), for which
  state has which owner (rike-single-owner-stance), or for write safety
  (rike-write-safety-stance).
disable-model-invocation: true
metadata:
  stance-axis: rebuildable-vs-durable
  opposes: rike-instant-open-stance
---

# Rike derived-purity stance

Markdown 是用户的笔记，rike 的索引、反链、搜索、页面表全部是从它算出来的。
只要这些算出来的东西可以被丢掉再算一遍，rike 就没有自己的数据库，也就没有
"升级失败打不开库"、"索引和文件不一致"、"备份和真相哪个对" 这三类问题。
rike 现在还没有这些 bug，是因为它还没有可损坏的派生状态；一个本地笔记工具
一旦有了这种状态，用户的第一份 bug 报告就是"我的笔记不见了"。

## Objective

让 rike 拥有的、Markdown 之外的持久字节保持为零（`.rike/` 里的备份、冲突副本、
临时文件不算：它们是用户内容的副本，不是推导结果）。任何新增的持久形态都必须
能被 `rm -rf` 掉之后，rike 仍然正确打开同一个库。

## Accepts this harm

每次启动、每次切库都要重新推导一遍：读全部文件、解析、分词、建索引。库越大，
用户等得越久，CPU 和电池也付得越多。`rike-instant-open-stance` 因此拿不到它要的
"打开即读"；它只能靠让等待不可见（先画后算）来缓解，而不能靠"上次算过了"。

## Attacks

- 一个派生结果被写进磁盘，且下次启动会**信任**它，而不是重新推导后再比对。
- 一个派生格式带版本号、带迁移分支、带"损坏则重建"的恢复路径——这三样同时出现，
  说明它已经是权威，只是被伪装成缓存。
- 一个缓存目录被写进笔记库，让用户以为它是笔记的一部分，或者让同步工具把它当
  内容同步。
- 派生数据的删除不再是安全的：删掉缓存会让用户看到报错、丢设置、丢历史。
- 推导输入不再完整：某段界面状态只存在于缓存里，重建后无法恢复。
- 为了省一次推导，把"未解析的原始字节"当成可用数据长期保存，于是解析器升级后
  旧数据仍然被当作有效。

## Evidence to demand

- 一条能重跑的验证：删掉 rike 拥有的全部派生路径后，打开库仍然正确（给出命令和
  输出，不是"应该可以"）。
- 指出具体路径与行：哪个函数写盘、哪个函数读回、读回时是否还比对源文件。
- 如果主张"这只是缓存"，必须给出失效判断的代码位置；没有失效判断的缓存就是权威。
- 一段能失败的反例：改动源文件后，旧缓存是否会被使用。

## Does not own

- 打开库要多久、首帧什么时候出现 → `rike-instant-open-stance`
- 页面列表、索引、catalog 各自归谁所有 → `rike-single-owner-stance`
- 写盘会不会覆盖用户内容 → `rike-write-safety-stance`
- 该不该做这个功能 → `rike-capability-stance`、`rike-scope-discipline-stance`

## Escalates when

- 改动引入了一个**必须**持久化才能成立的能力（例如跨进程增量索引、离线查询），
  而它无法在删除后自动重建。
- 派生格式需要版本号或迁移才能演进，且没有"版本不符就整份丢弃"的分支。
- 缓存位置、命名或生命周期会让用户或同步工具把它误认为笔记内容。

## Stable core and changing contract

The stable value is: **rike 没有不可丢弃的派生状态**。当前实现——`MemIndex` 的
四个 map、`FullText` 的 `Index::create_in_ram`、`Index` trait 的
`begin_batch`/`commit` 接口、`.rike/` 只放备份与冲突副本——都是这个价值在当下的
契约表达，不是价值本身。它们可以随证据改变。

When the contract changes, 比对的是"删掉之后还能不能正确打开库"，不是"有没有缓存
目录"。要保留的是：重建路径永远存在且被测试覆盖；要测的是：源文件变化后旧推导
结果不会被使用。不要因为某种形态现在存在就为它辩护，但要拒绝任何让"可丢弃"变成
口头承诺、无法验证的改动。

## Good patterns

- `crates/rike-index/src/lib.rs#L1-4`：crate 文档第一句就写明这是"derived,
  rebuildable index"，把可丢弃性放在模块契约里，而不是藏在实现里。
- `crates/rike-index/src/fts.rs#L123`：`Index::create_in_ram`，索引活在内存里，
  进程结束就没了，所以不存在"上次的索引"这种状态。
- `AGENTS.md#里程碑边界`：M2 索引后端路线把 turso/hawdb 的定位写成"可重建的派生
  缓存"，在选型阶段就把这条约束前置，而不是事后补。

## Bad patterns

- 把索引当成需要迁移的数据库：`crates/rike-index/src/lib.rs` 曾以
  "an in-memory map now, HawDB later" 描述未来，HawDB 是一个需要 schema 与迁移的
  存储。`AGENTS.md#里程碑边界` 的 2026-11 决策把它改成"所有后端都只是可重建的
  派生缓存"，并把 DuckDB/seekdb 排除在外。教训：把"以后换数据库"写进注释，等于
  给未来的自己留了一个权威。

这些模式说明这个目标在 rike 里是怎么被违反的；目标本身的契约仍归 `AGENTS.md`
（`#选型与锁定版本`、`#里程碑边界`）所有。
