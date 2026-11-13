---
name: rike-scope-discipline-stance
description: >-
  Load only when assigned by a stance-review orchestrator. Argues that every
  rike change must be removable by deleting code, without a data migration and
  without enlarging the crate, dependency or state-machine surface. Do not load
  for an ordinary task, even one touching the roadmap, dependencies or crate
  layout; do not use for feature completeness (rike-capability-stance), for
  write safety (rike-write-safety-stance), or for whether derived state may
  persist (rike-derived-purity-stance).
disable-model-invocation: true
metadata:
  stance-axis: blast-radius-vs-breadth
  opposes: rike-capability-stance
---

# Rike scope discipline stance

rike 是一个单人维护的原型，它的主要风险不是"功能不够"，而是"半成品太多"。
一个加了新 crate、新依赖、新持久格式的功能，如果做错了，回退成本不是删代码，
而是"用户的库已经被写过了"。所以 rike 的规则是：改动必须能被 `git revert` 干净
地撤掉，不需要迁移，不需要用户配合。这条规则让 rike 在只有一个人维护的情况下
仍然能保持每个提交都是可发布的。

## Objective

让每一次改动的回退成本等于删除代码：不新增 crate，不新增需要迁移的持久格式，
不新增无法退出的状态机，依赖锁版本，改动规模与当前里程碑匹配。

## Accepts this harm

功能会被拒绝或推迟。`rike-capability-stance` 因此拿不到它要的"日常主力可用"：
页面重命名、`{{query}}`、属性类型、同步都停在路线图上，用户可能因此不用 rike。
这个代价是明码标价的——`AGENTS.md#里程碑边界` 的"M1 不做"清单有 15 项，每一项
都是主动放弃的能力。

## Attacks

- 为一个功能新增 crate，或者把一个小功能拆成新模块 + 新抽象层。
- 新增一个需要迁移或版本号的持久格式；或者新增一个"以后要清理"的临时格式。
- 新增一个状态机，但没有退出路径（例如后台任务无法取消、队列无法清空）。
- 依赖不锁版本，或者为了一个便利功能引入一个大依赖。
- 改动的验证成本超过收益：为了一个边角体验引入新的测试基础设施。
- 在里程碑边界之外扩张：M1 期间做 M2/M3 的事，且没有记录为什么提前。
- 抽象先于第二个实现：为"以后可能换后端"引入 trait/泛型，而只有一个实现。

## Evidence to demand

- 一次 `git revert` 的可行性：改动是否只碰代码，不碰用户数据格式。
- 新增依赖的许可证与体积，以及它替换掉的现有能力（`AGENTS.md#许可证红线`）。
- 新增状态机的退出路径：什么条件下它停止、被取消、被清空。
- 改动规模与里程碑的对应：它属于 M1/M2/M3 的哪一项，还是清单外的新增。

## Does not own

- 这个功能对用户有多重要 → `rike-capability-stance`（本 stance 只说"代价多大"，
  不说"值不值得"）
- 写盘会不会写坏内容 → `rike-write-safety-stance`
- 派生状态能不能落盘 → `rike-derived-purity-stance`
- 打开库要多久 → `rike-instant-open-stance`

## Escalates when

- 一个改动必须引入持久格式或迁移才能成立，且无法在删除后自动重建。
- 一个改动必须新增 crate 或大依赖，而当前里程碑没有为它留位置。
- 一个改动引入的状态机在失败时无法回到初始状态（后台任务、队列、锁）。

## Stable core and changing contract

The stable value is: **回退成本等于删除代码**。当前实现——`AGENTS.md#架构与 crate
边界` 的"新代码放进现有 crate，不要为了小功能新增 crate"、`#选型与锁定版本` 的
精确锁版本、`#里程碑边界` 的 M1 不做清单——都是这个价值在当下的契约表达。它们
可以变（依赖可以升级，crate 可以拆），但"改动可逆"不能变。

When the contract changes, 比对的是回退成本，不是"代码量"。要保留的是：任何新增
形态都有一个删除路径；要测的是：撤掉这次改动后，用户的库仍然能打开。

## Good patterns

- `AGENTS.md#架构与 crate 边界`：依赖方向 `app → native → index、vault → core`
  是单向的，`core` 不依赖 NSH/OpenDAL/tokio，所以任何一层都可以被替换而不牵动
  其他层。
- `AGENTS.md#里程碑边界` 的 M1 不做清单：把"不做"写下来，比事后解释为什么没做
  更省成本。
- `docs/spikes/rich-editor.md`：实验工程在 0.1.0 前删除，只留结论。探针用完就删，
  不让它变成需要维护的第二套代码。

## Bad patterns

- `spikes/rich-editor/` 曾经是一个完整的编辑器替换实验工程，最后因为 `[[x]]`
  往返被转义而整体删除。它没有进入主代码，所以代价只是时间；如果它先合进主路径
  再发现不保真，代价就是一次迁移。教训：把"能不能保真"放在实验里验证，而不是
  在主线里试。
- NSH 曾经锁在一个固定 rev 以保证可复现，后来为了拿到 `InputRef.insertText`
  改成跟随上游 main、不锁 rev（`AGENTS.md#选型与锁定版本`）。这是一次明确的
  交换：可复现性让位给一个能力。教训：锁版本不是免费的，它会在某个具体能力上
  收取代价，所以每次解锁都要记下换来了什么。

这些模式说明这个目标在 rike 里是怎么被违反的；目标本身的契约仍归
`AGENTS.md#架构与 crate 边界` 与 `#里程碑边界` 所有。
