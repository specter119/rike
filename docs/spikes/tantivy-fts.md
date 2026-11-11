# tantivy + jieba 中文全文检索 spike

> 代码：`spikes/tantivy-fts/`（`cargo run --release` 可复现）。
> tantivy 0.27 + jieba-rs 0.11，2026-11-11 在 M600 上测得。

## 问题

M2 需要中文全文检索。评估过 HawDB/DuckDB/Turso/seekdb（见 AGENTS.md M2 路线）后，
先用 tantivy（Lucene 风格的 Rust 检索库）+ jieba 分词直接接在 MemIndex 侧，
验证它作为 rike `Index` 后端的技术可行性和成本。

## 结果

| 指标 | 数值 | 说明 |
| --- | --- | --- |
| 建 10k 块索引 | ~93 ms | 含 jieba 分词；Rust 侧全量重建完全可接受 |
| 单块增量更新（delete+add+commit） | ~53 ms | 每次 commit 都落盘；rike 的 300ms 防抖会把同批块合进一个 commit |
| 词查询（8 命中） | ~71 µs | |
| 短语查询 | ~298 µs | WithFreqsAndPositions 索引选项 |
| 短语前缀查询（补全用） | ~26 µs | `PhrasePrefixQuery`，`((索引` 场景 |
| block-id 精确查找 | ~14 µs | `STRING` 字段 + TermQuery，backlinks/块信息用 |
| 索引体积 | 0.6 MB / 10k 块 | 纯派生数据，可随时删掉重建 |

**结论：可行，成本可接受，作为 M2 的中文检索方案。** 增量更新 53ms 是单块
commit 的上界；rike 现有 INDEX_DELAY 防抖天然批量，实际每批一个 commit。
查询全在微秒级，比 MemIndex 的线性扫描只快不慢。

## 集成注意（M2 实现时）

- **API 变化大**：0.27 的 tokenizer 是 `tantivy-tokenizer-api` v0.5（GAT 风格
  `type TokenStream<'a>` + `advance/token/token_mut`），`TopDocs` 需要
  `.order_by_score()` 才是 Collector，`doc!` 宏要显式 `use tantivy::doc;`。
  网上旧教程基本全过时，以 spike 代码为准。
- **jieba 的 `cut` 返回 Token 数组**（`word.word` 字段），不是字符串切片。
- **`IndexWriter` 是目录独占锁**：同一 index 目录同时只能有一个 writer，且旧
  writer 必须 `drop` 后才能再建——rike 的单 Session 模型天然满足。
- **字段设计**：`block_id`/`page_rel` 用 `STRING`（精确匹配），`text` 用
  `TextFieldIndexing::default().set_tokenizer("jieba")`；QueryParser 会按字段
  配置选分词器。
- **Index::create_in_dir 要求目录已存在**（先 `create_dir_all`）。
- IndexWriter 默认堆 64MB 可调小；spike 里每文档都 STORED 了 text（0.6MB 里
  占大头），实际只需存 block_id 定位，体积还能再小。
- 增量失效（`IndexVer` 携带变更 rel 集合）仍然必须先做——它决定 TS 侧重算
  风暴，与用什么检索库无关。
