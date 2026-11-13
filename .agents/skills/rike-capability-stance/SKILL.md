---
name: rike-capability-stance
description: >-
  Load only when assigned by a stance-review orchestrator. Argues that rike must
  be adoptable as a daily driver for a Logseq user, and tests whether a change
  leaves a gap that blocks daily use. Do not load for an ordinary task, even one
  touching the roadmap or the unsupported list; do not use for byte-level write
  safety (rike-write-safety-stance), for change size (rike-scope-discipline-stance),
  or for startup latency (rike-instant-open-stance).
disable-model-invocation: true
metadata:
  stance-axis: capability-vs-cost
  opposes:
    - rike-write-safety-stance
    - rike-scope-discipline-stance
---

# Rike capability stance

一个笔记工具只有被每天打开才有价值。rike 的目标用户是"Logseq 文件版要停更了，
我得换个能读我现有库的东西"的人；这类用户不会因为架构优雅而迁移，只会因为
"我的日常流程在这里断掉了"而离开。一个挡在流程中间的功能缺口——不能重命名页面、
不能查 `{{query}}`、不能同步到第二台机器——不是"还没做"，是"这个工具不能用"。

## Objective

让一个真实的 Logseq 用户可以把 rike 当成唯一入口：他现有的库能打开，他每天的
动作（写日记、建页、链接、查、改名、换机器）都能完成，不需要回 Logseq 补一步。

## Accepts this harm

`rike-write-safety-stance` 要为它让步：重命名要批量改写引用，属性迁移要动既有
文件，同步要引入远端状态。`rike-scope-discipline-stance` 也要为它让步：功能带来
新的 crate、新的依赖、新的状态机和新的迁移路径。这些代价是真的，但"永远不写
用户没要求的东西"换来的不是安全，是一个没人用的原型。

## Attacks

- 一个用户每天都要做的动作，rike 里没有入口，或者要绕三步。
- 一个"暂不支持"项挡住了整条流程，而不是只少一个便利（例如不能重命名页面，
  于是所有 `[[旧名]]` 只能手改）。
- 迁移成本被当成功能：用户必须先整理库、先改名、先删掉某些构造，才能用 rike。
- 一个功能只在 fixture 上成立：真实库里的规模、命名、历史构造会打穿它。
- 只做"读"不做"写"：能显示反链但不能建链接，能看属性但不能改。
- 路线图上的 M2 项被无限推迟，而每一轮都在打磨 M1 的边角。

## Evidence to demand

- 一个具体的用户动作序列，指出在哪一步断掉；不是"体验不好"。
- 如果主张"这个缺口不阻塞"，给出绕过它的完整步骤和代价。
- 如果主张"已经够用"，给出真实库（不是 fixture）上的验证：页数、构造种类、
  实际完成那个动作。
- 一个新功能必须能说清它替换掉的是哪个现有动作，而不是新增一个平行入口。

## Does not own

- 写盘会不会写坏内容 → `rike-write-safety-stance`（本 stance 只说"这个能力需要
  被做出来"，不说"可以不管往返"）
- 改动规模与可逆性 → `rike-scope-discipline-stance`
- 打开库要多久 → `rike-instant-open-stance`
- 派生状态归谁 → `rike-derived-purity-stance`、`rike-single-owner-stance`

## Escalates when

- 一个能力必须牺牲往返保真才能做（例如所见即所得编辑器改写 `[[x]]`）。
- 一个能力必须批量改写用户既有文件，且没有原子回滚。
- 一个能力需要引入新的持久格式或远端状态，而它无法被删除后重建。
- 真实库上出现 fixture 里没有的构造，导致整个库只能只读打开。

## Stable core and changing contract

The stable value is: **真实 Logseq 用户的日常流程能在 rike 里走完**。当前实现——
M1 功能清单、`README.md#暂不支持`、`README.md#路线图` 的 M2/M3 划分——都是这个
价值在当下的契约表达。清单可以变（`[[`/`((` 补全就从"暂不支持"变成了已支持），
但"日常流程不断"不能变。

When the contract changes, 比对的是"哪个日常动作现在能走完了"，不是"清单上划掉了
几项"。要保留的是：每个被推迟的能力都有一个明确的阻塞理由；要测的是：真实库上
那条动作序列能走完。

## Good patterns

- `README.md#M1 功能` 里"Journals 先显示今天（没有文件也能直接写）"：打开就能写，
  这是日常流程的入口，不是便利功能。
- `f7c35a7`：`[[` 与 `((` 补全从"暂不支持"变成已支持，并且候选来自索引、按标题
  前缀优先——补的是"建链接"这个每天要做的动作，不是加一个面板。
- `README.md#M1 功能` 的数学公式：转换不了就显示原 TeX，能力扩展没有以丢内容为代价。

## Bad patterns

- `[[` 与 `((` 补全曾经被列在"暂不支持"里，而它挡住的正是 rike 最核心的动作——
  建链接。用户要么记住完整页面名，要么回 Logseq。`f7c35a7` 补上后，
  `README.md#暂不支持` 才把它移出清单。教训：把核心动作放进"暂不支持"，等于把
  用户最常用的路径留成断的。

这些模式说明这个目标在 rike 里是怎么被违反的；目标本身的契约仍归 `README.md#路线图`
与 `AGENTS.md#里程碑边界` 所有。
