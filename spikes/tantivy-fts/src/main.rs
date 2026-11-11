//! Spike: can tantivy + jieba be the Chinese full-text search behind rike's
//! `Index` trait? Measures what the M2 route needs:
//!
//! 1. build time for a vault-sized index (10k blocks, mixed CJK),
//! 2. incremental single-block update (delete + add) latency,
//! 3. query latency (term / phrase / prefix-for-completion),
//! 4. index size on disk.
//!
//! Run with `cargo run --release` from this directory.

use std::time::Instant;

use jieba_rs::Jieba;
use tantivy::collector::TopDocs;
use tantivy::query::{PhrasePrefixQuery, QueryParser, TermQuery};
use tantivy::schema::{IndexRecordOption, Schema, TextOptions, TextFieldIndexing, STRING};
use tantivy::tokenizer::{Token, TokenStream, Tokenizer};
use tantivy::{Index, IndexWriter, ReloadPolicy, Term, doc};

/// jieba as a tantivy tokenizer: the standard CJK segmentation choice.
#[derive(Clone)]
struct JiebaTokenizer(Jieba);

impl Tokenizer for JiebaTokenizer {
    type TokenStream<'a> = JiebaTokenStream;

    fn token_stream<'a>(&'a mut self, text: &'a str) -> JiebaTokenStream {
        let mut tokens = Vec::new();
        let mut offset = 0;
        for (position, word) in self.0.cut(text, true).into_iter().enumerate() {
            let start = offset;
            offset += word.word.len();
            if word.word.trim().is_empty() {
                continue;
            }
            tokens.push(Token {
                offset_from: start,
                offset_to: start + word.word.len(),
                position,
                position_length: 1,
                text: word.word.to_lowercase(),
            });
        }
        JiebaTokenStream { tokens, index: 0 }
    }
}

struct JiebaTokenStream {
    tokens: Vec<Token>,
    index: usize,
}

impl TokenStream for JiebaTokenStream {
    fn advance(&mut self) -> bool {
        self.index += 1;
        self.index <= self.tokens.len()
    }

    fn token(&self) -> &Token {
        &self.tokens[self.index - 1]
    }

    fn token_mut(&mut self) -> &mut Token {
        &mut self.tokens[self.index - 1]
    }
}

const BLOCKS: usize = 10_000;

/// Synthesized vault text: sentence pool the blocks draw from.
const SENTENCES: &[&str] = &[
    "今天开始写日课的原型，主要保留大纲编辑和每日页面",
    "TODO 读完 NSH 文档，评估渲染性能",
    "会议记录：讨论索引后端的选择，tantivy 与 turso 的对比",
    "这个想法来自 Logseq 的块引用机制，((uuid)) 指向具体块",
    "中文全文检索需要分词器，jieba 是常见选择",
    "properties:: 属性类型与页面模板放在 M2",
    "the quick brown fox jumps over the lazy dog",
    "混合中英文的笔记很常见 mixed CJK and English notes",
];

fn main() -> tantivy::Result<()> {
    let mut builder = Schema::builder();
    let block_id = builder.add_text_field("block_id", STRING);
    let page_rel = builder.add_text_field("page_rel", STRING);
    let jieba_indexing = TextFieldIndexing::default()
        .set_tokenizer("jieba")
        .set_index_option(IndexRecordOption::WithFreqsAndPositions);
    let text = builder.add_text_field(
        "text",
        TextOptions::default().set_indexing_options(jieba_indexing).set_stored(),
    );
    let schema = builder.build();

    let dir = std::env::temp_dir().join(format!("rike-tantivy-spike-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir)?;
    let index = Index::create_in_dir(&dir, schema)?;
    index.tokenizers().register("jieba", JiebaTokenizer(Jieba::new()));

    // ---- 1. build 10k blocks ----
    let t = Instant::now();
    let mut writer: IndexWriter = index.writer(64 * 1024 * 1024)?;
    for i in 0..BLOCKS {
        let sentence = SENTENCES[i % SENTENCES.len()];
        let page = format!("pages/项目___{}/{}.md", i % 100, i % 7);
        writer.add_document(doc!(
            block_id => format!("uuid-{i}"),
            page_rel => page,
            text => format!("{sentence} 补充说明编号{i}"),
        ))?;
    }
    writer.commit()?;
    println!("build {BLOCKS} blocks: {:?}", t.elapsed());
    drop(writer);

    let reader = index
        .reader_builder()
        .reload_policy(ReloadPolicy::Manual)
        .try_into()?;
    let searcher = reader.searcher();

    // ---- 2. incremental update: one block ----
    let t = Instant::now();
    let mut writer = index.writer(64 * 1024 * 1024)?;
    writer.delete_term(Term::from_field_text(block_id, "uuid-42"));
    writer.add_document(doc!(
        block_id => "uuid-42",
        page_rel => "pages/项目___0/0.md",
        text => "修改后的块：讨论 tantivy 的增量更新延迟",
    ))?;
    writer.commit()?;
    reader.reload()?;
    println!("incremental update 1 block (delete+add+commit): {:?}", t.elapsed());

    // ---- 3. queries ----
    let query_parser = QueryParser::for_index(&index, vec![text]);
    let query = query_parser.parse_query("中文 全文检索")?;
    let t = Instant::now();
    let hits = searcher.search(&query, &TopDocs::with_limit(8).order_by_score())?;
    println!("term query \"中文 全文检索\": {:?} ({} hits)", t.elapsed(), hits.len());

    let query = query_parser.parse_query("\"索引后端的选择\"")?;
    let t = Instant::now();
    let hits = searcher.search(&query, &TopDocs::with_limit(8).order_by_score())?;
    println!("phrase query: {:?} ({} hits)", t.elapsed(), hits.len());

    // Completion-style prefix: blocks containing a phrase starting "索引"
    // (what `((索引` would need).
    let term = Term::from_field_text(text, "索引");
    let query = PhrasePrefixQuery::new(vec![term]);
    let t = Instant::now();
    let hits = searcher.search(&query, &TopDocs::with_limit(8).order_by_score())?;
    println!("phrase-prefix query \"索引\": {:?} ({} hits)", t.elapsed(), hits.len());

    // Exact-block lookup by id (backlinks / block info).
    let query = TermQuery::new(
        Term::from_field_text(block_id, "uuid-42"),
        IndexRecordOption::Basic,
    );
    let t = Instant::now();
    let hits = searcher.search(&query, &TopDocs::with_limit(1).order_by_score())?;
    println!("block-id lookup: {:?} ({} hits)", t.elapsed(), hits.len());

    // ---- 4. size ----
    let size: u64 = walk(&dir);
    println!("index size on disk: {:.1} MB", size as f64 / 1e6);
    let _ = std::fs::remove_dir_all(&dir);
    Ok(())
}

fn walk(dir: &std::path::Path) -> u64 {
    std::fs::read_dir(dir)
        .map(|it| {
            it.map(|e| {
                let e = e.unwrap();
                if e.file_type().unwrap().is_dir() {
                    walk(&e.path())
                } else {
                    e.metadata().unwrap().len()
                }
            })
            .sum()
        })
        .unwrap_or(0)
}
