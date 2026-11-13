# Rike 立场地图

这份地图记录 rike 会为哪些目标牺牲别的东西，以及这些目标在哪里互相冲突。
它先于任何 stance 文件写成，是作者步骤必须通过的闸门。stance 文件在
`.agents/skills/rike-*-stance/SKILL.md`，冲突账本在
`.agents/skills/rike-stance-registry/SKILL.md`。

维护规则：新增或修改立场先改这里，再改 stance 文件；只改 stance 文件不改地图，
下一次 pass 就无法判断某对冲突是被论证过还是被假设的。

## 1. Stakes

| Id | Objective (one line) | Evidence | What it would demand |
| --- | --- | --- | --- |
| S1 | Markdown 是唯一真相；rike 自己产生的派生状态必须可丢弃、可重建，不能成为需要迁移或恢复的权威。 | `AGENTS.md#里程碑边界`（M2 索引后端路线末句）；`AGENTS.md#选型与锁定版本`（索引行）；`crates/rike-index/src/lib.rs#L1-4`；`crates/rike-index/src/fts.rs#L123`（`create_in_ram`） | 拒绝把索引落盘；每次启动重新推导；派生格式不写版本号、不做迁移、不做损坏恢复；删掉 `.rike/` 之外的一切派生文件，rike 仍然能打开库。 |
| S2 | 窗口出现的那一帧库就可读可写，与库多大无关；打开库前不允许有用户可见的等待。 | `crates/rike-native/src/session.rs#queue_index`（解析前返回）；`crates/rike-native/src/session.rs#catalog_of`；`Event::IndexProgress`；`ui/src/page.tsx#IndexBar`；`README.md#M1 功能`（Journals 先显示今天） | 从当前最便宜的来源画界面；接受派生数据暂时不完整；为了不等，可以引入第二份列表来源、进度状态和缓存。 |
| S3 | 每份数据只有一个权威；副本只能被事件整体作废后重取，永不回写。 | `AGENTS.md#界面与原生模块`（"凡是'丢了会丢数据或写坏文件'的状态只归 Rust"、"永不双向写"、"`leave` 是退出/切库的唯一权威"）；`crates/rike-native/src/session.rs#Inner`（`RefCell` + `gen`）；`crates/rike-native/src/session.rs#coalesce` | 一个数据一个所有者；第二个来源要么被派生出来，要么带明确的作废规则；跨层写入一律经过权威方校验。 |
| S4 | 任何编辑、迁移或缓存写入都不能丢或写坏用户写下的字节；拿不准就拒绝并告知。 | `AGENTS.md#数据格式不变量`（无损兜底、只读页、重复 `id::`、页面键冲突）；`AGENTS.md#保存与并发规则`（`base_hash`、stale、冲突副本、备份）；`crates/rike-native/src/session.rs#try_leave_inner`；`README.md#数据安全` | 自检失败就只读；stale 时拒写；退出可以被拒绝；认不出的构造原样保留；宁可少一个功能，不可多一次覆盖。 |
| S5 | rike 要能被 Logseq 用户当日常主力用；挡住日常使用的功能缺口就是 bug。 | `README.md#M1 功能`；`README.md#暂不支持`；`README.md#路线图`；`AGENTS.md#里程碑边界`（M2/M3） | 补齐属性类型、未链接引用、页面重命名、查询、vim 模式、同步；接受更多状态、更多依赖、更多迁移。 |
| S6 | 每次改动都必须是可逆的：能用删代码回退，不需要数据迁移，不扩大 crate / 依赖 / 状态机的地盘。 | `AGENTS.md#架构与 crate 边界`（"新代码放进现有 crate，不要为了小功能新增 crate"）；`AGENTS.md#里程碑边界`（M1 不做清单、M3 推迟）；`AGENTS.md#选型与锁定版本` | 拒绝功能；一个 crate；依赖锁版本；新状态机必须有退出路径；改动规模与里程碑匹配。 |

## 2. Oppositions

| Pair | Contended resource | A demands | B resists | B demands | A resists | Recorded decision |
| --- | --- | --- | --- | --- | --- | --- |
| S1 vs S2 | 磁盘上由 rike 拥有的派生字节 | 索引不落盘，每次启动重建 | 重建让打开库要等全量解析 | 把推导结果存下来，下次直接读 | 存下来的东西会过期、会损坏、需要版本号 | `git show 8da1774:crates/rike-native/src/session.rs`（`start()` 里 `self.scan().await`，先扫完再挂界面）；`AGENTS.md#里程碑边界`（"所有后端都只是可重建的派生缓存"） |
| S2 vs S3 | 索引未就绪期间页面列表的权威 | 用 catalog 先画，不等索引 | catalog 是页面列表的第二个来源 | 页面列表只由索引产生，UI 只投影它 | 只由索引产生就必须等索引 | `crates/rike-native/src/session.rs#pages`（`index.pages()` 与 `catalog` 求并集）；`#journals`（日记流改读 `catalog`） |
| S4 vs S5 | rike 接受并重写的 Logseq 构造范围 | 认不出就只读、就拒绝、就告警不合并 | 只读和拒绝让库用不下去 | 支持重命名、查询、属性类型，把库完整接管 | 重命名和查询要批量重写引用，一次写错就是全库损坏 | `AGENTS.md#数据格式不变量` 第 3 条（自检失败 → 只读）、第 5 条（页面键冲突只告警不合并）；`AGENTS.md#里程碑边界`（M1 不做页面重命名） |
| S5 vs S6 | 下一次改动的地盘 | 补 M2 功能，让 rike 能日常用 | 每补一个功能就多一份状态和迁移 | 只做能删代码回退的改动 | 只做可逆改动就永远补不上功能 | `AGENTS.md#里程碑边界`（M1 不做 15 项、M3 推迟同步）；`AGENTS.md#架构与 crate 边界`（不新增 crate）；`docs/reviews/2026-11-13-stance-pass.md#32`（Rescan 是否重解析全库） |
| S2 vs S6 | 引擎线程上的派生工作预算 | 扫描期搜索也要可用，派生不完整必须可见 | 每页一次进度/列表事件把界面拖成 O(页数²) | 派生工作合并、去抖、有退出路径 | 合并与去抖让“不完整”多等一个窗口 | `crates/rike-native/src/session.rs#index_from_disk`（`stale` 时逐页 `emit(Event::Pages)`）与 `#refresh_catalog`（300 ms 去抖）并存；`docs/reviews/2026-11-13-stance-pass.md#23` |

五对都至少有一处已记录的决策，不是假设。

## 3. Unopposed stakes

没有。S1–S6 每一行都出现在至少一对里。

## 4. Missing stakes

| Theme | Where it appears | Why no stake holds it |
| --- | --- | --- |
| 中文文本处理的正确性（jieba 分词、NFC、UTF-16 偏移） | `AGENTS.md#数据格式不变量` 第 10 条；`crates/rike-index/src/fts.rs`；`crates/rike-native/src/session.rs#utf16_to_byte` | 这是实现约束，不是会被牺牲的目标：没有任何一方会主张"中文匹配可以不准"。它需要的是领域测试，不是 stance。 |
| 许可证红线（不得引入 GPL/AGPL） | `AGENTS.md#许可证红线` | 硬约束，没有取舍空间；任何一方都不会为它让步，也不会为它牺牲别的。 |
| 输入法 / Wayland / NSH 平台坑 | `AGENTS.md#输入法`、`AGENTS.md#NSH 已知坑` | 环境事实与绕过方案，不是目标。 |
| 测试与交付纪律（fmt/clippy/test/ui-test 全绿） | `AGENTS.md#工具链与命令` | 流程门槛；它约束所有 stance，不偏向任何一方。 |

## 5. Candidate stance set

| Stance | Objective | In pairs |
| --- | --- | --- |
| `rike-derived-purity-stance` | 派生状态可丢弃，Markdown 是唯一权威 | S1 vs S2 |
| `rike-instant-open-stance` | 窗口出现即可读，与库大小无关 | S1 vs S2；S2 vs S3；S2 vs S6 |
| `rike-single-owner-stance` | 一份数据一个权威，副本只作废不回写 | S2 vs S3 |
| `rike-write-safety-stance` | 不丢不坏用户字节，拿不准就拒绝 | S4 vs S5 |
| `rike-capability-stance` | 能当日常主力用，功能缺口就是 bug | S4 vs S5；S5 vs S6 |
| `rike-scope-discipline-stance` | 每次改动可逆、地盘不扩大 | S5 vs S6；S2 vs S6 |

## 6. Deviations

- **S1 与 S6 在同向检查上部分重合**：对"把索引落盘"这件事两者都会反对（S1 说它成为新权威，S6 说它扩大地盘）。保留为两个 stance 的理由是它们在别处分开：S1 不管新增一个纯内存 memo（S6 会问成本），S6 不管新增一个没有派生状态的依赖（S1 无话可说）。若以后某个 pass 发现两者对同一改动给出同一条 finding，应合并。
- **S5（capability）同时对立 S4 与 S6**：`metadata.opposes` 写两者（列表），
  `rike-scope-discipline-stance` 与 `rike-write-safety-stance` 各自回指它，元数据
  与账本的五对对立一一对应。
- **S2 vs S6 已提升为正式第 5 行**：2026-11-13 的 pass 里 `instant-open` 与
  `scope-discipline` 从两个角度独立命中同一个 surface（扫描期逐页 `Event::Pages`
  与 300 ms 去抖并存），符合“下一次 pass 再次出现就提升”的条件。`stance-design`
  的“2–4 对”是指导不是上限；六条 stance 里没有一条只靠一对维系。
- **S1 vs S6 仍是候选，不提升**：两者对 `.rike/pending/` 同向反对，但理由不同
  （前者说它是伪装成缓存的权威，后者说它新增了撤不掉的落盘格式），不构成“同一条
  finding”的合并证据。
- **里程碑边界本身没有独立 stance**：M1 不做清单被 S4（拒绝重命名）和 S6（拒绝扩地盘）分别引用，不单独设 stance，避免与两者重复。
