# 2026-11-13 立场 pass 记录

按 `hydra-crux-review` 跑的一次完整 pass：六个 stance 各一个 reviewer（fresh，只读
自己的 stance 文件），编排者按 surface 归并、逐对 contest、分类，最后只升级真正
不可合并的那一条。

## 1. 绑定

| 项 | 值 |
| --- | --- |
| Target | `cindy/quirky-mendel@e2db02c`（`feat: background index, a pre-parse catalog, and a new UI`），工作区干净，HEAD == master |
| 范围 | `git diff 8da1774..e2db02c`（后台索引、预解析 catalog、界面重做） |
| 参与的 stance | 六个全部：`derived-purity`、`instant-open`、`single-owner`、`write-safety`、`capability`、`scope-discipline` |
| 在范围内的对立 | 四对全部（catalog 权威、`rel_for_title` 写路径、`Pages` 发送时机、`Index` trait 扩地盘） |
| 约束项（不重开） | 本表 `Resolved without escalation` 中**已落地**的三条：catalog 作为索引未就绪期的页面列表来源；`rel_for_title` 的查找顺序；`Pages` 走 `coalesce` |
| 方法 | 只读 survey，不改代码；reviewer 不看账本、不看地图、不看彼此的产出 |

## 2. 发现（按 surface 归并）

同一 surface 由多个 reviewer 独立命中的，只记一次，并列出命中者。

### 2.1 `rel_for_title` 的 catalog 层答不出 `title::`（S-A）

命中者：`derived-purity`、`single-owner`、`write-safety`、`capability`（四人独立）。

`catalog_entry` 的键只来自文件名/日记格式（`session.rs#catalog_entry`），而页面身份
由 `title::` 优先（`rike-index/src/title.rs#resolve_title`）。索引还没解析到
`pages/note.md`（`title:: Bar`）时跟 `[[Bar]]`，catalog 层命中不了，落到
`pages/Bar.md` 兜底 → 库里出现两个文件声明同一个页面键，链接跳到新的空页，唯一
痕迹是一条去重告警。上一次 pass 把这条记为"catalog 层已解决"，实际只解决了文件名
标题的页。

### 2.2 页面列表在本地编辑后不重发

命中者：`single-owner`、`derived-purity`。

`index_page`/`index_parsed` 返回 `catalog_disagrees` 的结果，但六个本地路径的调用点
全部丢弃（`session.rs:957,1107,1406,1409,1480,1560`）；`Event::Pages` 只在
`session.rs:733,1580,1937,2004,2035,2065,2086` 发出，没有一处是本地编辑。改一个
`title::` 后 `pages()` 变了，侧栏不变，直到某次无关的 create/remove 或重启。

### 2.3 扫描期逐页 `Event::Pages`（S2 vs S6）

命中者：`instant-open`、`scope-discipline`。

`index_from_disk` 在 `announce || stale` 时发 `Pages`（`session.rs:2065`），而
`stale = catalog_disagrees` 对每个 `title::` 页都为真；TS 侧每个 `Pages` 都做全量
`Rike.pages()` + `Rike.journals()`（`ui/src/store.ts:270`）。一个大量使用 `title::`
的库，扫描期是 O(页数²) 次全量列表，正好绕开这次改动自己加的 300 ms 目录去抖。

### 2.4 catalog 的作废规则有缺口

命中者：`single-owner`、`capability`、`scope-discipline`（三人独立，角度不同）。

- `Changed` 分支（`session.rs#external_change`）从不 `catalog_changed()`；而
  `rescan_dir` 用 `announce_creation=false` 调 `refresh_page`（`actor.rs:492`），
  所以"整目录拷进来"的页进了索引、能搜、能链，但侧栏和 Journals 看不见，直到一次
  无关事件把 catalog 弄脏。
- `refresh_catalog` 只换 `catalog`，从不把新 rel 排进 `pending_index`；`scan` 会排。
  两条"重新列目录"的路径语义不一致，于是目录里能出现索引永远不知道的页。

### 2.5 `refresh_catalog` 先消费标志再列目录

命中者：`derived-purity`、`single-owner`。

`mem::take(&mut catalog_dirty)` 在 `list_pages()` 之前（`session.rs:2074`），失败分支
直接 `return`（`2079`）：一次失败就把"列表变了"这件事花掉，不重试、不通知。`scan`
在同样的失败上会通知。

### 2.6 Rescan 的语义（S5 vs S6）

命中者：`scope-discipline`、`capability`。

`scan()` 用 `docs`（只有被打开过的页）判断"已索引"（`session.rs:2025`），所以一次
`Rescan` 把全库重新解析、重新 upsert，并发出覆盖全库 rel 的 `IndexVer`；扫描中途再
`Rescan` 还会把已解析的页重新排队。反过来，`docs`/索引集当判据又意味着"已索引 =
尝试过一次"，watcher 漏掉的一次修改永远不会被重新解析。

### 2.7 索引读盘推进了写前检查的令牌

命中者：`write-safety`。

`VaultHandle::load` 无条件写 `state.base_hash = Some(hash)`（`actor.rs:223`），后台
索引读盘走的正是这条；`refresh_page` 在 `base_hash == hash` 时早退（`actor.rs:425`）。
于是"漏掉的 watcher 事件 + 一次索引读"会把可检测的冲突变成静默覆盖。

### 2.8 其余（单点、无对立，按 bug 处理）

| 位置 | 事实 | 命中者 |
| --- | --- | --- |
| `session.rs#refresh_catalog` | 失败只 `return`，不恢复标志、不唤醒 pump（`index_timer` 只由 `PumpMsg::CatalogDirty` 装上） | derived-purity |
| `session.rs#report_index_warnings` | `warned` 只插不删（`session.rs:891`），警告修好后再次出现会被永久吞掉 | derived-purity |
| `rike-index/src/fts.rs#commit_pending` | 先清 `pending` 再 `writer.commit()` 并吞掉错误（`fts.rs:167`）；`reader.reload().expect(...)` 失败会 panic 引擎线程 | derived-purity |
| `session.rs#backlinks` | `indexing()` 为真时仍给确定计数，只挡住 0 | derived-purity、capability |
| `session.rs#complete` | 只读索引，`pages()` 却和 catalog 求并集：侧栏看得见的页在 `[[` 面板里没有候选 | capability、single-owner |
| `session.rs#journals` | 先全量收集再 `truncate`；`pages()` 的比较器里每次比较分配两个小写字符串 | instant-open |
| `session.rs#index_next` | 队列被打断（shutdown/leave）时 `begin_batch` 没有对应的 `end_batch`，本次会话剩余时间搜索不再提交 | scope-discipline |
| `crates/rike-index/src/lib.rs#Index` | `begin_batch`/`commit`/`end_batch` 在 trait 上，只有一个实现，`session` 里本来就是具体类型 `MemIndex` | scope-discipline |
| `session.rs#catalog_entry` | 与 `resolve_title` 各写一份"文件名/日记格式 → 标题" | scope-discipline |
| `Cargo.toml:34,39` | `jieba-rs = "0.11"`、`tantivy = "0.27"` 是 caret，违反 `AGENTS.md` 自己的 0.x 精确锁定政策；`AGENTS.md#选型与锁定版本` 的索引行仍写"tantivy 先行" | scope-discipline |
| `vault/src/actor.rs#list_pages` | 仍逐文件 `stat`，`PageFile` 仍有 `len`/`modified`，无消费者 | instant-open |

## 3. Contest 与分类

每条候选冲突都发回受损的一方，问"这个设计下你的目标还损失什么，什么证据能表明没有
损失"。只有两侧都说清楚才算合并。

### 3.1 合并（两侧都确认，或确认后附条件）

| 设计 | 服务 | 条件（reviewer 自己提的） |
| --- | --- | --- |
| `pages_dirty` + `touch_pages()`：所有"列表变了"的点只调它，立即作废缓存并置位，pump 在窗口结束后发一次 `Pages`；`refresh_catalog` 不再自己发 | instant-open（每窗口一次 O(n log n)）、single-owner（合并只归一处）、derived-purity（失效立即、发射去抖，两件事分开）、capability（导入期列表不饿死） | 覆盖全部发射点；发射前重查标志而不是清掉；有 max-wait 上限；状态归属表新增该行 |
| `refresh_catalog` 失败时恢复 `catalog_dirty` 并 `send(PumpMsg::CatalogDirty)` | derived-purity、single-owner | 重试有上限；失败期间谁是权威要写进文档 |
| `scan` 与 `refresh_catalog` 合成一个"按 listing 对账"函数：更新 catalog + 把新 rel 排进 `pending_index`，**不删索引条目**；删除仍只归 watcher 的 `Removed` 分支 | single-owner（列表变化一个触发）、scope-discipline（一个函数、catalog 仍是 listing 的函数）、instant-open（列表错误不改索引）、write-safety（不因一次列表丢掉 `title::` 的归属记忆）、capability（不因瞬时缺席删页） | 对账函数不碰 `index_total`/`begin_batch`；列表失败先返回 |
| 索引读盘走不推进 `base_hash` 的路径（`peek`），写前令牌只由"用户打开/载入"推进 | write-safety | `peek` 仍走 `path::validate`；不跳过首份备份的记账；不发 `Changed`；`base_hash` 仍为 `None` |
| `begin_batch/commit/end_batch` 移到 `MemIndex` 的固有方法 | scope-discipline（净删除）、instant-open（扫描期搜索仍可用） | pump 在队列空时补 `end_batch`；保留每 32 页的 commit 节奏 |
| `catalog_entry` 与 `resolve_title` 共用一个"文件名/日记格式 → 标题"的函数 | scope-discipline | 签名只有 `(rel, config)`，`title::` 分支留在 `resolve_title`；`catalog_of` 不触发任何 vault 读 |
| `warned` 每次报告后只保留 `index.warnings()` 里仍存在的消息 | derived-purity | 一条"修好再弄坏会再通知"的测试 |
| `commit_pending` 只在 `commit()` 成功后清 `pending`，失败标记重建 | derived-purity | `reader.reload()` 也走错误分支，不 panic；有"重建后搜索结果等于全量重建"的测试 |
| 反链面板在 `indexing()` 时显示"N 个页面链接到这里（索引中 done/total）" | capability、derived-purity | 用已有的 `indexProgress()`；`indexing` 的置位/清位只有 `scan`/`index_next` 两处 |
| `tantivy`/`jieba-rs` 精确锁版本，索引行写进 `AGENTS.md#选型与锁定版本` | scope-discipline | 许可证能从 `cargo metadata` 复验 |

### 3.2 未解决证据（有测量，有归属，本 pass 不裁决）

| 冲突 | 两边的损失 | 什么能定 | 归属 |
| --- | --- | --- | --- |
| Rescan 是否重新解析全库（capability vs scope-discipline） | capability：用索引 rel 集判"已索引"会让 watcher 漏掉的一次修改永远不刷新；scope-discipline：全库重解析 + 全库 `IndexVer` 是 O(n) 白工 | 1k/10k 页库上一次 `Rescan` 的墙钟时间与 `IndexVer` 后的界面代价；若在可接受预算内，保留全量重解析，`docs` 判据只修成 `page_by_path`；否则需要 mtime/len 或内容哈希，而那与"去掉每文件 stat"直接冲突 | 下一个 pass，或做索引后端 benchmark 的那次 |
| `complete()` 是否并入 catalog（capability vs derived-purity） | capability：首次打开时 `[[已有页]]` 没有候选，是日常动作；derived-purity：catalog 的标题是猜的，插进 Markdown 会变成永久未链接引用 | 真实库里 `title::` ≠ 文件名标题的页数占比；以及"候选只列索引里没有的键"能否避免插入猜出来的标题 | 与 S-A 的裁决一起定（两者根因相同） |

### 3.3 不可合并（升级给用户）

见第 4 节。

## 4. 升级：S-A

```text
Conflict: S-A — write-safety vs instant-open（capability 付代价）
Resource: 首帧与"今天这一页可读"之间的启动预算——具体是"索引未就绪期间，为了知道
          某个标题属于哪个文件，允不允许在首帧路径上读文件"

write-safety 说：任何一次写盘只能写回 rike 完整理解并逐字节验证过的内容；拿不准就
不写，并让用户知道为什么。跟一个链接就新建文件，等于用未完成的派生决定用户库的
形状。

instant-open 说：从窗口出现到"今天这一页可读、可打字"，不能等任何与库大小成正比
的事；不完整必须可见，而不是被藏起来。

Buffer attempts:
  1. catalog 建表时读每个文件的 title:: —— 失败：把 O(n) 次文件读放回首帧路径，
     正是 e2db02c 删掉的那笔开销。
  2. indexing() 为真且兜底路径不在目录里就拒绝 —— 失败：拒绝的正是"这页真的还不
     存在"这个最常见的情况，移民首次打开是分钟级扫描，等于入口处堵住。
  3. 兜底路径先按 rel_path 在目录里找一次 —— 失败：title:: 页的兜底路径
     （pages/<标题>.md）本来就没有任何目录条目声明，重复文件照建。
  4. 溢写到 .rike/pending/ —— 服务双方，但新增落盘格式 + 合并/改名状态机，且
     .rike/ 被 path::is_page 与 watcher 排除，合并依赖"索引追上"这个可能永不发生的
     事件；derived-purity 与 scope-discipline 都反对，write-safety 要求崩溃后仍能
     呈现这些字节。

Option A: catalog 只为"索引尚未解析的 rel"读一次文件头，取 title::
  Serves: write-safety（不猜路径、不新建重复文件）；capability（不拒绝任何操作）
  Costs: instant-open 首帧从 2 次 getdents 变成 N 次读（只限扫描窗口内的 rel，
         不是每次重列）；scope-discipline 多一条"列表期读文件"的路径
  Reversible: 是，删掉那段读即可；但 catalog 一旦有 title::，"列表不读文件"这条
         契约就变了，文档要跟着改

Option B: indexing() 为真且兜底路径不在目录里时拒绝，并给出可见提示
  Serves: write-safety（不写猜出来的路径）；instant-open（首帧不变）
  Costs: capability——移民首次打开是分钟级扫描，这期间"写今天的日记并链一个新页"
         被堵住，且没有 ETA
  Reversible: 是，删掉拒绝分支即可

Option C: 兜底路径改为 .rike/pending/ 的临时页，索引追上后合并
  Serves: write-safety（无覆盖、无删除）、instant-open（不阻塞）
  Costs: scope-discipline（新落盘格式 + 状态机）、derived-purity（.rike/ 里的用户
         字节不是可丢弃的派生）、write-safety（合并是 delete+create，非原子）
  Reversible: 否——一旦有用户的字节只存在于 .rike/pending/，撤销这个设计要先迁移

Option D: 保持现状，接受扫描窗口内的重复文件
  Serves: instant-open（零成本）、capability（不拒绝）
  Costs: write-safety——重复文件永久存在，链接指向哪个由 MemIndex 的 iter().next()
         决定，用户对着一个文件编辑、内容在另一个文件里；git revert 撤不掉
  Reversible: 否，用户库里的重复文件要人工合并

What becomes irreversible: 选 A 会改"catalog = listing 的函数、零 IO"这条契约；
选 C 会新增一种 rike 拥有的、装着用户字节的落盘格式；选 D 把代价写进用户库。
Decision needed: 索引未就绪期间跟一个指向 title:: 页的链接时，是读文件确认（A）、
拒绝并提示（B）、写临时页（C），还是接受重复文件（D）？
```

编排者的建议（不作为选项的一部分）：A 的代价是可测量的，且只作用于扫描窗口内的
rel；B 的代价落在移民首次打开这个最坏的时机。但 A 必须复用 `resolve_title`，不能
自己再写一份 `title::` 解析，否则会造出第二个页面键来源——那正是这次 pass 在
2.1 里抓到的东西。

## 5. 账本更正

上一次 pass 记在 `Resolved without escalation` 里的两条，代码里没有：

| 行 | 账本说 | 代码 |
| --- | --- | --- |
| 扫描期 commit 节奏 | 删掉 `index_next` 里每 32 页的 commit，改由 `index_timer` 分支 commit | `session.rs:719` 仍是 `done % 32 == 0 → index.commit()`；`index_timer`（`session.rs:769`）只服务 `IndexDirty` |
| `list_pages` 的每文件 `stat` | 去掉 `stat`，`PageFile` 只留 `rel` | `PageFile` 仍有 `len`/`modified`（`vault/src/lib.rs:240`），`list_pages` 仍逐文件 `stat`（`actor.rs:189`）；`AGENTS.md:136` 仍写"`(mtime, len)` 只用来快速跳过" |

两者都已改成"记录未落地"。教训：账本写"已裁决"和"已落地"是两件事，`Evidence`
列必须能指向代码或测试。

## 6. 本 pass 对立场集的改动

- `rike-capability-stance`、`rike-instant-open-stance` 的 `metadata.opposes` 改成
  列表，与账本的四行对立一一对应（原先各只写一个，`scope-discipline` 与
  `single-owner` 的回指落空）。
- `rike-single-owner-stance` 的 Objective 原是两个并列从句（一个权威 **且** 位置
  声明过），把"位置必须被声明"标成不可交易的约束。
- 地图把 `instant-open` vs `scope-discipline` 从"候选对"提升为正式第 5 行：本 pass
  里 `instant-open` 与 `scope-discipline` 从两个角度独立命中同一个 surface（扫描期
  逐页 `Pages`），符合地图自己写的提升条件。`derived-purity` vs `scope-discipline`
  仍是候选（本 pass 两者对 `.rike/pending/` 同向反对，但理由不同，不构成合并证据）。

## 7. 外部参考：其他实现怎么处理页面身份与改名

只看**文档**，不读源码。AGPL 项目（Logseq、tine、lsdoc、mldoc）一律跳过；GPL 项目
（MediaWiki、org-roam）按 `AGENTS.md#许可证红线` 里 docu.md 的先例只作设计参考。

| 项目 | 许可证 | 页面身份 | 别名放在哪 | 改名怎么办 |
| --- | --- | --- | --- | --- |
| Obsidian | 闭源，文档公开 | 文件名（`[[文件名]]` / `[[路径/文件名]]`） | frontmatter `aliases:` | 默认自动更新全库内部链接，可改为"先问我" |
| Dendron | Apache-2.0 | 文件名（层级即 ID） | frontmatter `title:`，只给 graph 节点打标签 | `dendron rename` 重构：改名、改层级、移动子树、改 header 都同步更新链接 |
| Foam | MIT | 文件名 | — | `F2` 改标题，同步更新指向它的 wikilink |
| MediaWiki | GPL-2.0 | 页面标题 | — | 移动页面 + **在旧标题留 redirect 页** |
| org-roam | GPL-3.0 | **文件/标题上的 `:ID:`**，链接写 `id:foo` | — | 改文件名、改标题都不影响链接 |
| **Logseq** | AGPL（**只看公开讨论，不看源码**） | **标题**（`title::` 优先），文件名是它的编码 | `alias::`，**参与解析** | 改名时全库更新引用；改 `alias::` 不跟着改，旧链接会指向空页 |

Logseq 的 alignment 方向和其他四家**相反**，而且它需要 `title::` 的原因很具体：

- 社区原话：“since Logseq uses titles as filenames”——标题决定文件名，不是反过来
- 文件名编码是**有损的**：`.` 表示 namespace，所以 `A. A. Milne.md` 会被解析成 namespace；
  社区的做法是给它加一行 `title:: A. A. Milne` 把字面标题钉住
- 所以 `title::` 不是“第二个来源”，而是**在编码装不下时把标题钉回去的唯一手段**

rike 的文件名编码是**无损的**（`names.rs#title_to_file_stem` 单射，连 `_` 的歧义都用 `%5F`
处理了；`file_stem_to_title` 是它的逆，有测试），所以 rike 不需要 `title::` 来还原标题。
这就是 P1 的真正依据：不是“行业惯例”，而是**rike 和 Logseq 在编码能力上不同，
所以身份归属也不同**。

三种机制：

**A. 文件名即身份 + 改名时重写引用**（Obsidian、Dendron、Foam、MediaWiki 的标题）。
主流做法。触发点都是**应用内**的改名动作。

**B. 持久化 ID 即身份**（org-roam）。手册原文：

> A node is any headline or top level file with an ID.
> We link between nodes using Org's standard ID link (e.g. `id:foo`).

代价：链接不可读、不可移植；每个节点必须写 ID。它自己的 `org-roam.db` 是纯派生
缓存，不参与链接解析。注意 Dendron 也存 `id:`，但链接仍按文件名——"存了 UUID"和
"用 UUID 当身份"是两件事。

**C. 旧名留墓碑**（MediaWiki）。移动页面时：

> Creates a new page "A", whose content is a redirect to page "B"
>
> When the move operation leaves behind a redirect, it should not result in any broken
> links except the possibility of *double redirects*.

两个要点：墓碑是**内容**（重启、同步、换工具都还在），不是数据库；它自己承认了
竞态——"Avoid trying to move and edit a page simultaneously"，否则编辑可能落到
redirect 上。

**D. 别名只做显示**（Obsidian `aliases`、Dendron `title`）。两者都不参与链接解析。
Obsidian 明确解释：链接目标写真实文件名而不是别名，是为了和其他 wikilink 实现互通。

**外部改名**：四家都没有文档化的处理。MediaWiki 的 redirect 是唯一"旧名继续有效"
的机制，但它只在应用内 move 时创建。

## 8. 提议（正在对抗复核，尚未落账）

| # | 提议 | 依据 |
| --- | --- | --- |
| P1 | 页面身份只来自文件名（日记格式 + `___`/百分号解码）；`title::` 降级为普通属性，不参与身份 | Logseq 用标题当身份是因为它的文件名编码**有损**（`.` = namespace）；rike 的编码无损，不需要 `title::`。其余四家（Obsidian/Dendron/Foam/MediaWiki）的身份都只来自文件名。**这是一次有意的分叉，不是惯例** |
| P2 | 别名用 `alias::`（M2）：参与链接解析 + 显示，永不被写成链接目标 | Obsidian 的 aliases 是同类做法 |
| P3 | rike 内改名 = 原子重写全库前向引用（`[[x]]`/`#x`/`#[[x]]`），可选留 `alias:: 旧名` 作墓碑 | Dendron/Obsidian 的重写 + MediaWiki 的 redirect |
| P4 | rike 外改名 = 检测（会话内 tombstone / 持久索引）→ 提议，用户确认后写 `alias::` 或重写引用；DB 不参与解析 | 没有先例，MediaWiki 的墓碑给了正确形态 |

已知的未解点（复核时必须回答）：`alias::` 和 `title::` 一样是**文件里的**键，catalog
看不见它——如果 `[[旧名]]` 在索引未就绪期间要解析到别名，就会重新踩上 S-A。

## 9. 对抗复核（会议）

六个 stance 各一个 reviewer，两轮：第一轮各自陈述损失与最强反对意见；第二轮拿到
其他各方的反对意见后逐条反驳或让步。结果见下一节。

### 9.1 P1 收敛：全员接受，附条件

- 身份 = 文件名；`title::` 与 `alias::` 都是**派生别名键**：只读、永不回写、由
  `IndexVer` 作废。`single-owner` 的表述：**身份键 = 文件名；`title::`/`alias::` =
  派生别名键**。任何写路径（`save`/`replace`/TSX）在类型上不接受别名当键。
- 不一致时**保留通知**（`write-safety` 要求），但只能从唯一权威出：`emit_index_ver`
  → `report_index_warnings` → `Inner::warned` 去重，不得在每页索引路径里二次扫描。
- 同批改 `AGENTS.md#数据格式不变量` 第 5 条与状态归属表。`single-owner` 明确拒绝
  "P1 是纯删除"的定性：它改变了不变量的语义，不是删代码。
- 收益（`derived-purity`、`instant-open` 都认）：删 `resolve_title` 的 explicit 分支、
  删 `catalog_disagrees`、删扫描期逐页 `Pages`。

### 9.2 P2 的核心缺陷（会议独立命中三次）

`instant-open`、`single-owner`、`scope-discipline` 各自指出：`alias::` 和 `title::`
一样是**文件内容**，catalog 只从目录列表派生、看不见它。索引未就绪时 `[[旧名]]`
落到 `pages/旧名.md` 兜底并新建重复文件——S-A 原样复活。`instant-open` 的话最准：

> 今天这个洞由 `title::` 承担，而 107 页 0 个 `title::`，等于休眠；P2 把它推成主功能
> = 唤醒它。

修法（`single-owner` 提出，`instant-open` 接受）：

1. `rel_for_title` 返回 `Option`，缺省分支**不写盘**；"页面不存在"由 UI 显式确认创建
2. `indexing()` 为真时显示"索引中…"：有界、自愈、无重试按钮、与"不存在"可区分
3. alias 键是派生、只读、由 `IndexVer` 作废；`PageDelta` 增加 alias-key dirtiness
4. 因此 `capability` **撤回**"`alias::` 与 P1 同批"的硬要求：洞从"写错文件"降为
   "有界延迟"；`alias::` 的写入与 UI 归 M2

### 9.3 P3：拒绝"原子重写"，改为"逐页可拒 + 文件名最后改"

`write-safety` 与 `scope-discipline` 都指出"全库事务"在现有架构里不存在：actor 逐页
原子写、自检失败的页只读，所以部分成功是必然的。收敛形态：

1. dry-run diff + 阻塞页清单
2. 逐页经 vault actor 写（`base_hash` + 自检 + 只读/stale 拒绝），逐页可拒、被拒列名
3. **全部成功后最后改文件名**；任何一页被拒就不改文件名，整条幂等可重跑

- 不新增持久文件（`scope-discipline`）。manifest 被 `write-safety` 拒绝：resume 若
  信任 manifest 而不逐页重验 `base_hash`，就是 `f7c35a7` 那个教训的翻版；而改名幂等，
  残余状态本来就能从库里读出来。
- `scope-discipline` 的"只重写已加载的页"被 `write-safety` 纠正为"索引里引用旧标题
  的页先加载再处理"（加载是读，写仍逐页可拒），否则未加载页永久悬空。
- 代价：库里有只读或 stale 页时，改名**被拒绝**。
- 崩溃后：可观察，但**不被通知**（`write-safety` 明确接受这个代价，因为跨会话公告
  需要一个必须被信任的持久意图文件，比沉默更危险）。

### 9.4 P4：不存 rike 自己的映射，墓碑写进内容

- `derived-purity`：单次启动的目录 diff 能过 `rm -rf .rike/*` 测试，但它不是墓碑——
  猜不出链式改名（A→B→C），也猜不出"改名同时改内容"。
- `capability` 让步：不存"旧名 → 新名"映射；改名时把旧名写进被改名页面自己的
  `alias:: 旧名`。这是内容，删掉 `.rike/` 和索引后仍在，`derived-purity` 的测试能过。
- 所以 P4 = 会话内检测（或单快照 diff）→ 提议 → 确认后写 `alias::` 或重写引用。
  DB 不参与链接解析。

### 9.5 需要用户确认的三条

1. **有只读/stale 页的库不能改名**（`write-safety` 的硬要求）。
2. **改名崩溃后是"可观察"而不是"被通知"**：重开库看到 `old.md` 还在、部分
   `[[old]]` 已变 `[[new]]`、`[[new]]` 显示未解析。没有跨会话公告。
3. **`alias::` 成为改名的硬依赖**：用户日后手删 `alias::`，旧链接静默断掉，改名时
   必须告知。

### 9.6 状态归属表新增行（`single-owner` 要求，缺一行不签）

| 状态 | 权威 | 副本 | 作废事件 |
| --- | --- | --- | --- |
| 页面身份键 | 文件名（NFC + lowercase，`list_pages`） | 索引 key→rel | `Created`/`Removed` → `catalog_dirty` 300 ms 重建 → `Pages` |
| 别名键（`title::`/`alias::`） | 文件内容，仅索引期读取，永不回写 | `Index` 的 alias→rel | `IndexVer`（changed/removed rels + 被引用页键并集） |
| 未解析标题 | 索引就绪性本身 | 无副本，不落盘不猜路径 | `rel_for_title` 返回 `None` → 拒绝并提示 |
| 删除 | 文件是否存在（`Removed`） | 索引 removed 集合 | `Pages` 重建 |
| TSX 侧标题/别名展示 | 无（只读 Rust） | `pageMap` 条目 | `page`/`PageChanged` 重取 |

## 10. 回滚 P1：官方文档推翻了它

用户要求先查 Logseq 官方文档（`logseq/docs`，**只看文档不看源码**），再决定。查到的两条直接推翻了 P1：

> `title` (1) (page) - **overrides the title of a page to be different from the file name.**
> —— `pages/Built-in Properties.md`

> `:triple-lowbar`：Use triple underscore `___` for slash `/`；Use Percent-encoding for
> other invalid characters；**No more `title::` property required**（仍可手动使用）
> —— `pages/Filename format.md`

两条合起来说明：

1. `title::` 是官方定义的身份覆盖，不是"冗余断言"。P1 的"身份只来自文件名"与官方行为冲突。
2. rike 的 `___` + 百分号编码**就是** `:triple-lowbar`。在这个格式里 `title::` 不是必需的
   （所以 rike 从不写它是对的），但用户手动写的时候 Logseq 会认。

所以 P1 回滚：`crates/` 全部还原到 `e2db02c`，`AGENTS.md` 的不变量 #5、状态归属表、
索引警告三处同步还原（保留 `docs/reference/logseq-properties.md` 的指针）。
`cargo test --workspace` 全部通过。

**代价：S-A 重新打开。** catalog 又看不见 `title::` 了。但新证据缩小了它的范围：
在 `:triple-lowbar` 库里 `title::` 只出现在用户手动覆盖的页面上（罕见）；在 `:legacy`
库里它是常态（官方要求给编码过的页面补 `title::`）。所以要么按 `:file/name-format`
分档处理，要么统一让 catalog 为未解析的页读一次文件头。

**同时暴露的兼容缺口**（详见 `docs/reference/logseq-properties.md` §10）：

- 页面属性值不建链（官方明确会建链）
- 逗号分隔只做了块级 `tags`
- 引号抑制链接没实现
- 带标签的引用 `[display]([[page]])` 没实现
- `{{embed}}`、优先级 `[#A]`、`^^highlight^^`、图片 `{:height}` 没实现
