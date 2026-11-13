---
name: rike-stance-registry
description: >-
  Use to find which stance owns a question in rike, which stances oppose each
  other, and which conflicts are still open. Read by the orchestrator before
  assigning a stance review or recording a resolution. Do not use as a review
  stance itself, and do not hand it to a reviewer.
---

# Rike stance registry

下面的 stance 是目标，不是清单。它们被刻意对立：每一个要求的东西，都由另一个
付代价。一次 pass 如果在触及已列对立的改动上什么都没找到，应该复查；但干净的
pass 也是可能的结果，不是审查出错的证据。不要为了满足地图而制造冲突。

这个文件只给编排者看。不要交给 reviewer：它写明了编排者预期的对立，读过它的
reviewer 会去"找到"它们。

## Stances

| Stance | Axis | Objective (one line) | Accepts |
| --- | --- | --- | --- |
| `rike-derived-purity-stance` | rebuildable-vs-durable | Markdown 之外的持久派生字节为零 | 每次启动重新推导，库越大等得越久 |
| `rike-instant-open-stance` | latency-vs-completeness | 首帧可读，与库大小无关 | 派生数据暂时不完整且必须可见 |
| `rike-single-owner-stance` | authority-vs-locality | 一份数据一个声明过的权威 | 界面不能乐观更新，每次操作要往返 |
| `rike-write-safety-stance` | safety-vs-availability | 不丢不坏用户字节，拿不准就拒绝 | 有些库只能只读，退出会被拒绝 |
| `rike-capability-stance` | capability-vs-cost | 真实 Logseq 用户的日常流程能走完 | 写安全与改动规模要为它让步 |
| `rike-scope-discipline-stance` | blast-radius-vs-breadth | 回退成本等于删除代码 | 功能被拒绝或推迟 |

## Oppositions

每一行是一对必须一起审的对象。只审一侧不算 stance review。

| Pair | Contended resource | Where these objectives are known to compete |
| --- | --- | --- |
| `rike-derived-purity-stance` vs `rike-instant-open-stance` | 磁盘上由 rike 拥有的派生字节 | 启动路径：先扫全库再挂界面（`8da1774`），与"把推导结果存下来"的诉求 |
| `rike-instant-open-stance` vs `rike-single-owner-stance` | 索引未就绪期间页面列表的权威 | 后台索引：catalog 与索引求并集、日记流改读 catalog |
| `rike-write-safety-stance` vs `rike-capability-stance` | rike 接受并重写的 Logseq 构造范围 | 只读兜底、页面键冲突不合并、页面重命名被推迟 |
| `rike-capability-stance` vs `rike-scope-discipline-stance` | 下一次改动的地盘 | M1 不做清单、M3 推迟同步、NSH 从锁 rev 改为跟随 main；Rescan 是否重解析全库 |
| `rike-instant-open-stance` vs `rike-scope-discipline-stance` | 引擎线程上的派生工作预算 | 后台索引：扫描期逐页 `Event::Pages` 与 300 ms 去抖（2026-11-13 pass，两侧独立命中同一 surface）；`Index` trait 上的批处理协议 |

## Contract surfaces

stance 冲突的裁决落在这里。一个说不出落点的裁决是不完整的。

- `AGENTS.md#界面与原生模块` — Rust/TSX 分工、状态归属表、退出与切库权威
- `AGENTS.md#数据格式不变量` — 往返保真、只读页、页面键、文件名编码
- `AGENTS.md#保存与并发规则` — `base_hash`、stale、冲突、备份、保存节奏
- `AGENTS.md#里程碑边界` — M1/M2/M3 边界、索引后端路线
- `AGENTS.md#架构与 crate 边界`、`#选型与锁定版本` — crate 划分、依赖与锁版本
- `README.md#数据安全`、`#路线图`、`#暂不支持` — 用户可见的承诺与边界
- `docs/stances.md` — 立场地图本身（stakes、oppositions、deviations）

## Resolved without escalation

两个 stance 都接受的缓冲空间解法。记下来，下一次 pass 不再重开。这里的裁决不是
目标的妥协，而是同时服务两者的设计。

| Conflict | Resolution | Serves | Evidence | State |
| --- | --- | --- | --- | --- |
| 索引未就绪时按标题导航会新建重复文件（write-safety vs capability） | `rel_for_title` 在索引未命中后、journal 解析之后查 `catalog`（`PageKey` 键，只读 rel，无 IO）；**不做**“索引完成时把内容搬到正确 rel”的 pass | write-safety（不写猜出来的路径、不覆盖、不删）、scope-discipline（不引入隐式重命名，M1 不做清单仍然有效）、capability（不拒绝写入、不阻塞退出） | `session.rs#rel_for_title`；`session.rs#catalog_of`；`AGENTS.md#里程碑边界`（M1 不做页面重命名） | 已落地；**只覆盖文件名标题的页**，`title::` 页的残余见 `## Decisions that sacrificed a stance`（只支持 `:triple-lowbar`） |
| 同上，`title::` 页面的残余重复（catalog 的键只来自文件名） | 若必须关闭：provisional 期间内容溢写到 `.rike/pending/<hash>.md`，键解析出来后**纯创建**到正确 rel；不移动、不删除已有文件 | write-safety（无覆盖、无删除、崩溃不丢）、scope-discipline（不新增 vault rename/delete 能力）、capability（零中断） | `rike-index/src/title.rs#resolve_title`；`vault/src/actor.rs#save`（`base_hash`） | 条件性，未落地；2026-11-13 pass 证明残余是活的，已升级 |
| 扫描期 commit 节奏（instant-open vs capability 的搜索新鲜度） | 删掉 `index_next` 里每 32 页的 commit；改由 `index_timer` 分支 commit（**必须放在 `index_changes.is_empty()` 早退之前**），保留 `end_batch` 的 commit；pump 索引分支每轮最多 4 页后裸 `yield_now`（循环写在 handler 里，不写进 select 的 future，否则 `BorrowMutError`） | instant-open（不占引擎线程）、capability（扫描期搜索仍可用，且窗口 ≤ `INDEX_DELAY`） | `session.rs#index_next`；`session.rs#emit_index_ver`；`fts.rs#commit_pending` | **未落地**：`session.rs:719` 仍是 `done % 32` 的 commit，`index_timer`（`session.rs:769`）只服务 `IndexDirty`。2026-11-13 pass 核实 |
| catalog 与磁盘的收敛（instant-open vs scope-discipline） | `Created`/`Removed` 只置 `catalog_dirty` 并复用已有的 300 ms 定时器；到期后一次 `list_pages()` 重建 catalog 并合并发 `Pages`（每文件 `stat` 的去除是另一行，未落地）。**不做**定点增删 | scope-discipline（catalog 仍等于 listing 的函数，无第二写者、无新不变式）、instant-open（事件 tick 上无 O(库) IO，去抖合并） | `session.rs#on_vault_event`；`session.rs#scan`；`vault/src/actor.rs#list_pages` | 已落地（`session.rs#refresh_catalog`）；`Changed` 分支的作废缺口见下行 |
| `list_pages` 的每文件 `stat`（instant-open vs write-safety） | 去掉 `stat`，`PageFile` 只留 `rel`；同时把 `AGENTS.md#保存与并发规则` 里“`(mtime, len)` 只用来快速跳过”改成否定式（该快速路径在代码里**从未存在**） | instant-open（首帧系数从 O(n) 次 op 往返降到 2 次 `getdents`）、write-safety（删掉一个不存在的旁路，并关闭“以后有人照文档补一个 metadata 跳过”的门） | `vault/src/actor.rs#list_pages`；`vault/src/lib.rs#PageFile`；`AGENTS.md#保存与并发规则` | **未落地**：`PageFile` 仍有 `len`/`modified`（`vault/src/lib.rs:240`），`list_pages` 仍逐文件 `stat`（`actor.rs:189`），`AGENTS.md:136` 仍写“`(mtime, len)` 只用来快速跳过”。2026-11-13 pass 核实 |
| `Event::Pages` 的发送时机（instant-open vs single-owner） | `Pages` 走已有的 `coalesce`，不在每个 `Created`/`Removed` 同步 `deliver` | instant-open（消除批量导入的 `O(n²)` 全量 clone+sort）、single-owner（合并仍只有一处实现） | `session.rs#emit`；`session.rs#coalesce` | 部分落地：`coalesce` 已有，但 `index_from_disk` 仍逐页 `emit(Event::Pages)`（`session.rs:2065`），见下行 |
| 页面列表重发的唯一入口（instant-open vs single-owner） | `pages_dirty` + `touch_pages()`：所有“列表变了”的点只调它——立即作废 `known_pages` 缓存并置位，pump 在窗口结束后发一次 `Pages`；`refresh_catalog` 不再自己发 | instant-open（每窗口一次 O(n log n)）、single-owner（合并只归一处）、derived-purity（失效立即、发射去抖）、capability（导入期列表不饿死） | `docs/reviews/2026-11-13-stance-pass.md#31` | 未落地；条件：覆盖全部发射点、发射前重查标志、有 max-wait |
| 本地 `title::` 编辑后列表不重发（single-owner vs instant-open） | `index_page`/`index_parsed` 的 `catalog_disagrees` 结果在六个调用点被丢弃（`session.rs:957,1107,1406,1409,1480,1560`）；改为交给 `touch_pages()` | single-owner、capability | 同上 | 未落地 |
| catalog 的作废规则（single-owner vs scope-discipline） | 一条规则：索引知道而 catalog 没列的 rel 也置 `catalog_dirty`；`scan` 与 `refresh_catalog` 合成一个“按 listing 对账”函数（更新 catalog + 把新 rel 排进 `pending_index`），**不删索引条目** | single-owner（列表变化一个触发）、scope-discipline（一个函数）、instant-open（列表错误不改索引）、write-safety（不因一次列表丢掉 `title::` 归属）、capability（不因瞬时缺席删页） | `session.rs#scan`；`session.rs#refresh_catalog`；`actor.rs#rescan_dir` | 未落地；删除仍只归 watcher 的 `Removed` 分支 |
| 索引读盘推进写前令牌（write-safety vs instant-open） | 索引读盘走不推进 `base_hash` 的 `peek`；令牌只由“用户打开/载入”推进 | write-safety（可检测的冲突不被静默覆盖） | `vault/src/actor.rs#load`（`actor.rs:223`）；`#refresh_page`（`actor.rs:425`） | 未落地；条件：`peek` 仍 `path::validate`、不跳过首份备份、不发 `Changed` |
| `Index` trait 的批处理协议（scope-discipline vs instant-open） | `begin_batch/commit/end_batch` 移到 `MemIndex` 固有方法，等第二个后端出现再上 trait | scope-discipline（净删除）、instant-open（扫描期搜索仍可用） | `rike-index/src/lib.rs#Index`；`session.rs:532`（字段已是具体 `MemIndex`） | 未落地；条件：pump 在队列空时补 `end_batch`、保留每 32 页 commit |

## Open essential conflicts

等用户裁决的冲突。每条必须按
[escalation.md](../../hydra-crux/references/escalation.md) 的格式。

| Conflict | Pair | Status | Decision record |
| --- | --- | --- | --- |
| Rescan 是否重解析全库 | `rike-capability-stance` vs `rike-scope-discipline-stance` | 未解决证据：需 1k/10k 页的 `Rescan` 墙钟与 `IndexVer` 后的界面代价 | `docs/reviews/2026-11-13-stance-pass.md#32` |

## Decisions that sacrificed a stance

用户裁决 essential conflict 后，输的一方不会被删除。记下牺牲，让下一个 reviewer
看见代价。

| Decision | Sacrificed stance | What was given up | Why |
| --- | --- | --- | --- |
| 2026-11-13：只支持 `:triple-lowbar` 文件名格式，S-A 降为文档化限制 | `rike-capability-stance` | `:legacy` 库（0.8.9 之前的图）里页面名靠 `title::` 才能还原，rike 只告警不保证正确；用户得先在 Logseq 里升级格式 | 官方文档：`:triple-lowbar` 是 0.8.9 起的新默认，`title::` 在其中非必需；支持 `:legacy` 意味着 catalog 必须读每个文件头，与“首帧不读文件”的契约冲突 |

## Evidence

这套 stance 活着的证据。不是每次 review 的日志——只记改变了东西的 pass。

| Date | Pass | Conflict found | Effect on design or set |
| --- | --- | --- | --- |
| 2026-11-13 | `cindy/quirky-mendel@e2db02c`（后台索引 + 预解析 catalog + 界面重做；只读 survey） | `write-safety` vs `capability`：`rel_for_title` 的 catalog 层对 `title::` 页答不出归属，跟链会新建重复文件（四个 stance 独立命中）。`instant-open` vs `single-owner`：`catalog_disagrees` 的结果在六个本地路径被丢弃。`instant-open` vs `scope-discipline`：扫描期逐页 `Pages` 使 TS 侧全量重取，O(页数²)。`single-owner` vs `scope-discipline`：`Changed` 与 `refresh_catalog` 两条路径都不把新 rel 排进索引。`write-safety` vs `instant-open`：索引读盘推进 `base_hash`。 | 六个 stance 全部参与，两轮 contest。九条收敛为带条件的缓冲设计，一条升级给用户（S-A），一条记为未解决证据（Rescan）。**账本更正**：上表“扫描期 commit 节奏”与“每文件 `stat`”两行原先记为已落地，代码里没有，已改为“未落地”。地图把 `instant-open` vs `scope-discipline` 提升为正式第 5 行；`opposes` 元数据改成列表，`single-owner` 的 Objective 拆出不可交易约束。完整记录：`docs/reviews/2026-11-13-stance-pass.md` |
| 2026-11-13 | 对抗复核会议 + P1 落地（`rike-index/src/title.rs`、`mem.rs`、`session.rs`） | `alias::` 参与解析会在索引未就绪时复活 S-A（`instant-open`、`single-owner`、`scope-discipline` 三方独立命中）；“全库原子重写”在逐页 actor 上不存在（`write-safety`、`scope-discipline`）；外部改名的墓碑不可重算（`derived-purity`） | P3 改为“逐页可拒 + 文件名最后改”，P4 改为“墓碑写进 `alias::`”，P2 降级到 M2 且以 `rel_for_title -> Option` 为前提 |
| 2026-11-13 | 官方文档核对（`logseq/docs`，只看文档）+ **P1 回滚** | 官方文档写明 `title` 是内建页面属性且“overrides the title of a page to be different from the file name”；同时 rike 的编码就是 `:triple-lowbar`，其中 `title::` 非必需但仍被承认 | P1 全量回滚（`crates/` 还原、`AGENTS.md` 三处还原、测试全绿）。S-A 重新打开，但范围按 `:file/name-format` 分档：`:legacy` 常态、`:triple-lowbar` 罕见。兼容缺口清单落在 `docs/reference/logseq-properties.md#10` |
| 2026-11-13 | `worktree@8da1774+uncommitted`（后台索引 + 界面重做） | `instant-open` vs `single-owner`：catalog 是页面列表的第二个来源，且 `Created`/`Removed` 从不更新它（`session.rs:666`、`:1969` 是唯一两次填充）——侧栏与日记流在会话内会长期偏离磁盘。`write-safety` vs `capability`：索引期间 `rel_for_title` 落到 `pages/<stem>.md`，会静默新建重复页面。`scope-discipline` vs `instant-open`：`Index` trait 新增 `begin_batch`/`commit`/`end_batch` 时还没有第二个实现，且空库时 `begin_batch` 没有对应的 `end_batch`（`queue_index` 无条件开批，`index_next` 在队列空时早退）。 | 六个 stance 全部参与，两轮 contest 后三处冲突均收敛为缓冲设计（见上表），无需升级。地图新增两条待补对立：`instant-open` vs `scope-discipline`（后台索引进状态机）、`derived-purity` vs `scope-discipline`（两者在“索引落盘”上同向，已在 `docs/stances.md#6. Deviations` 记录）。`single-owner` 的“catalog 是第二来源”被承认为**已声明**的派生来源，代价是它必须有作废事件——这一条写进了缓冲设计，而不是 stance 文件。 |
