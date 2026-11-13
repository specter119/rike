---
name: rike-write-safety-stance
description: >-
  Load only when assigned by a stance-review orchestrator. Argues that rike must
  never lose or corrupt a byte the user wrote, and tests whether a change makes
  a write path capable of overwriting content it did not fully understand. Do
  not load for an ordinary task, even one touching save, conflict handling, or
  the parser; do not use for feature completeness (rike-capability-stance), for
  startup latency (rike-instant-open-stance), or for whether derived state may
  persist (rike-derived-purity-stance).
disable-model-invocation: true
metadata:
  stance-axis: safety-vs-availability
  opposes: rike-capability-stance
---

# Rike write-safety stance

用户的笔记是唯一不能重做的部分。rike 的索引错了可以重建，界面丑了可以改，
但一次错误的覆盖就是永久损失——而且用户往往几周后才发现。所以 rike 在写盘这条
路上必须比"好用"更保守：认不出的构造原样保留，自检不过的页面只读，拿不准的时候
拒绝并说出来。这些决定每一条都会让某些库"用不了"，而正是这些决定让 rike 可以
被指向一个真实的、有五年历史的 Logseq 库。

## Objective

任何一次写盘，都只能写回 rike 完整理解并逐字节验证过的内容；否则不写，并让用户
知道为什么。衡量标准是"最坏情况下用户损失多少"，不是"平均情况下多顺手"。

## Accepts this harm

有些库打不开编辑：自检失败的页面只读，`:file/name-format` 不是 `:triple-lowbar`
只告警，重复 `id::` 取第一个，多个文件映射到同一页面键不合并。`rike-capability-stance`
因此拿不到"完整接管 Logseq 库"，`rike-instant-open-stance` 也拿不到"随时能退出"
（有 stale 页就拒绝退出）。这些代价是明码标价的，不是意外。

## Attacks

- 一条写路径能写到它没有完整解析过的字节上：raw 段被重新序列化、未知属性被丢弃、
  代码围栏内容被规范化。
- 自检（`serialize(parse(x)) == x` 的等价检查）被放宽、被跳过，或者只在部分入口执行。
- 写前检查被绕过：不走 `base_hash` 比对，或者 stale 状态下仍然写盘。
- 冲突路径先把本地版本写进正式文件，再考虑备份；或者备份失败后继续覆盖。
- 拒绝路径缺失：本该拒绝的操作变成"尽力而为"，且没有告知用户。
- 批量重写：一次操作改写多个文件，而失败时只回滚了一部分。
- 偏移换算或文本替换在错误的坐标系里做，于是写回的位置差几个字节——这类 bug 不会
  报错，只会悄悄改掉相邻的字。
- 只读标记只在一处生效：vault actor 拒绝了，但某个直接写文件的路径没有。

## Evidence to demand

- 指出写路径的完整链条：从界面动作到 `vault` actor 落盘，中间每一处校验。
- 一个能失败的测试：构造一个 rike 不理解的页面，执行该操作，断言文件字节不变。
- 如果主张"这个构造我们理解"，给出往返测试，而不是解析器能读出来。
- 失败时的用户可见结果：拒绝、只读、告警，三者之一，且能被断言。

## Does not own

- 该不该支持某个 Logseq 构造 → `rike-capability-stance`（本 stance 只说"不支持时
  不许写坏"）
- 打开库要多久 → `rike-instant-open-stance`
- 派生状态能不能落盘 → `rike-derived-purity-stance`
- 一份数据归谁 → `rike-single-owner-stance`
- 改动规模 → `rike-scope-discipline-stance`

## Escalates when

- 一个功能要求批量改写多个文件的既有内容（页面重命名、属性迁移、格式规范化），
  而失败回滚不是原子的。
- 一个优化要求放宽写前检查（例如用 `(mtime, len)` 代替内容哈希）。
- 一个功能要求写 rike 没有完整解析的构造（YAML front matter、未识别的 Markdown）。

## Stable core and changing contract

The stable value is: **不丢不坏用户字节；拿不准就拒绝**。当前实现——`base_hash`
写前比对、`Doc.stale` 阻止 `save_now`、冲突副本先于覆盖、`.rike/backups/` 每页
20 份、自检失败只读、`try_leave_inner` 拒绝退出——都是这个价值在当下的契约表达。
它们可以换（例如换成 WAL 或事务式发布），但"最坏情况不丢字节"不能换。

When the contract changes, 比对的是"最坏情况下用户损失什么"，不是"正常路径是否
通过"。要保留的是：每条写路径都有一个拒绝分支；要测的是：不认识的内容在往返后
逐字节不变。

## Good patterns

- `AGENTS.md#数据格式不变量` 第 3 条：`serialize(parse(x))` 自检不过就只读，把
  "不理解"变成"不写"，而不是"尽力写"。
- `AGENTS.md#保存与并发规则`：冲突时先把本地版本写进 `.rike/conflicts/` 并确认
  成功，再返回 `Conflict`；`stale` 时 `save_now` 不写盘，直到用户选边。
- `crates/rike-native/src/session.rs#try_leave_inner`：有 stale 页、有未保存修改、
  有打开的编辑，就拒绝退出并说明原因，而不是"先关再说"。
- `AGENTS.md#数据格式不变量` 第 10 条：字节偏移只在 `core` 内部用，UTF-16 换算
  集中在 session 层并有测试，避免坐标系混用。

## Bad patterns

- 接受补全候选曾经只按偏移拼接文本，不校验它要替换的那段还是不是候选面板看到
  的那段。`f7c35a7` 加入 `Session::replace_range`，先校验区间合法（`InvalidRange`）
  再校验待替换文本逐字相同（`StaleText`），不一致就拒绝。教训："我算出的偏移"
  和"磁盘上现在的字节"是两件事，写入前必须重新确认后者。

这些模式说明这个目标在 rike 里是怎么被违反的；目标本身的契约仍归
`AGENTS.md#数据格式不变量` 与 `#保存与并发规则` 所有。
