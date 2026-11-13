//! Chinese-aware full-text search over page titles and block text: tantivy
//! with jieba segmentation, riding next to [`MemIndex`](crate::MemIndex)'s own
//! maps. Purely derived — the owning index keeps it in lockstep, so it can be
//! dropped and rebuilt at any time.

use std::collections::BTreeSet;
use std::sync::OnceLock;

use jieba_rs::Jieba;
use tantivy::collector::TopDocs;
use tantivy::query::{BooleanQuery, Query, TermQuery};
use tantivy::schema::document::TantivyDocument;
use tantivy::schema::document::Value;
use tantivy::schema::{Field, IndexRecordOption, STRING, Schema, TextFieldIndexing, TextOptions};
use tantivy::tokenizer::{Token, TokenStream, Tokenizer};
use tantivy::{Index, IndexReader, IndexWriter, ReloadPolicy, Term, doc};

/// Loading the jieba dictionary is the expensive part, so share it
/// process-wide instead of per index.
fn jieba() -> &'static Jieba {
    static JIEBA: OnceLock<Jieba> = OnceLock::new();
    JIEBA.get_or_init(Jieba::new)
}

/// jieba as a tantivy tokenizer, in search mode: besides the normal
/// segmentation it also indexes the parts of compound words, so queries like
/// `检索` still hit a block that only contains `全文检索`.
#[derive(Clone, Copy, Default)]
struct JiebaTokenizer;

impl Tokenizer for JiebaTokenizer {
    type TokenStream<'a> = JiebaTokenStream;

    fn token_stream<'a>(&'a mut self, text: &'a str) -> JiebaTokenStream {
        let mut tokens = Vec::new();
        for (position, word) in jieba().cut_for_search(text, true).into_iter().enumerate() {
            if word.word.trim().is_empty() {
                continue;
            }
            tokens.push(Token {
                offset_from: word.byte_start,
                offset_to: word.byte_end,
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

/// One hit: which page it came from and, for block hits, the block and text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Hit {
    pub rel: String,
    pub block_id: Option<String>,
    pub text: String,
}

/// A RAM-resident tantivy index over one vault. Exactly one writer and one
/// reader; the single-session model guarantees the writer is unique.
pub(crate) struct FullText {
    fields: Fields,
    writer: IndexWriter,
    reader: IndexReader,
    /// A bulk load is running: writes wait for `end_batch` to commit.
    batching: bool,
    /// Documents are in the writer but not committed yet.
    pending: bool,
}

struct Fields {
    rel: Field,
    block_id: Field,
    title: Field,
    text: Field,
}

impl Default for FullText {
    fn default() -> Self {
        Self::new()
    }
}

impl FullText {
    pub fn new() -> Self {
        let jieba_indexing = TextFieldIndexing::default()
            .set_tokenizer("jieba")
            .set_index_option(IndexRecordOption::WithFreqsAndPositions);
        let mut builder = Schema::builder();
        let rel = builder.add_text_field("rel", STRING.set_stored());
        let block_id = builder.add_text_field("block_id", STRING.set_stored());
        let title = builder.add_text_field(
            "title",
            TextOptions::default().set_indexing_options(jieba_indexing.clone()),
        );
        let text = builder.add_text_field(
            "text",
            TextOptions::default()
                .set_indexing_options(jieba_indexing)
                .set_stored(),
        );
        let index = Index::create_in_ram(builder.build());
        index.tokenizers().register("jieba", JiebaTokenizer);
        // One worker thread: rike batches upserts behind INDEX_DELAY anyway,
        // and a small arena keeps per-session memory flat.
        let writer = index
            .writer_with_num_threads(1, 16 * 1024 * 1024)
            .expect("RAM index writer");
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()
            .expect("RAM index reader");
        Self {
            fields: Fields {
                rel,
                block_id,
                title,
                text,
            },
            writer,
            reader,
            batching: false,
            pending: false,
        }
    }

    /// Starts a bulk load: `upsert_page` and `remove_page` stop committing, so
    /// a scan pays one commit instead of one per page. Nothing is searchable
    /// until `end_batch`; the caller must not search meanwhile.
    pub fn begin_batch(&mut self) {
        self.batching = true;
    }

    /// Ends a bulk load and commits what it wrote.
    pub fn end_batch(&mut self) {
        self.batching = false;
        self.commit_pending();
    }

    /// Makes the documents written so far visible, once.
    pub(crate) fn commit_pending(&mut self) {
        if !self.pending {
            return;
        }
        self.pending = false;
        let _ = self.writer.commit();
        self.reader.reload().expect("RAM reader reload");
    }

    /// Replaces everything indexed for `rel`: one title doc plus one doc per
    /// block.
    pub fn upsert_page(&mut self, rel: &str, title: &str, blocks: &[(String, &str)]) {
        self.writer
            .delete_term(Term::from_field_text(self.fields.rel, rel));
        let _ = self.writer.add_document(doc!(
            self.fields.rel => rel,
            self.fields.title => title,
        ));
        for (block_id, text) in blocks {
            let _ = self.writer.add_document(doc!(
                self.fields.rel => rel,
                self.fields.block_id => block_id.as_str(),
                self.fields.text => *text,
            ));
        }
        self.pending = true;
        if !self.batching {
            self.commit_pending();
        }
    }

    pub fn remove_page(&mut self, rel: &str) {
        self.writer
            .delete_term(Term::from_field_text(self.fields.rel, rel));
        self.pending = true;
        if !self.batching {
            self.commit_pending();
        }
    }

    /// Pages whose title matches every token of `query`, best first.
    pub fn search_titles(&self, query: &str, limit: usize) -> Vec<Hit> {
        self.search(self.fields.title, query, limit)
    }

    /// Blocks whose text matches every token of `query`, best first.
    pub fn search_texts(&self, query: &str, limit: usize) -> Vec<Hit> {
        self.search(self.fields.text, query, limit)
    }

    fn search(&self, field: Field, query: &str, limit: usize) -> Vec<Hit> {
        let Some(query) = conjunction(field, query) else {
            return Vec::new();
        };
        let searcher = self.reader.searcher();
        let Ok(top) = searcher.search(&query, &TopDocs::with_limit(limit).order_by_score()) else {
            return Vec::new();
        };
        top.into_iter()
            .filter_map(|(_, addr)| {
                let doc: TantivyDocument = searcher.doc(addr).ok()?;
                Some(Hit {
                    rel: doc.get_first(self.fields.rel)?.as_str()?.to_owned(),
                    block_id: doc
                        .get_first(self.fields.block_id)
                        .and_then(|value| value.as_str())
                        .map(str::to_owned),
                    text: doc
                        .get_first(self.fields.text)
                        .and_then(|value| value.as_str())
                        .unwrap_or_default()
                        .to_owned(),
                })
            })
            .collect()
    }
}

/// All query tokens must appear (AND), in any order. Built by hand instead of
/// via `QueryParser` so arbitrary user input — parens, colons, wildcards — can
/// never be a syntax error.
fn conjunction(field: Field, query: &str) -> Option<BooleanQuery> {
    let tokens: BTreeSet<_> = jieba()
        .cut(query, true)
        .into_iter()
        .map(|word| word.word.trim().to_lowercase())
        .filter(|word| !word.is_empty())
        .collect();
    if tokens.is_empty() {
        return None;
    }
    let subqueries: Vec<Box<dyn Query>> = tokens
        .into_iter()
        .map(|word| {
            Box::new(TermQuery::new(
                Term::from_field_text(field, &word),
                IndexRecordOption::WithFreqsAndPositions,
            )) as Box<dyn Query>
        })
        .collect();
    Some(BooleanQuery::intersection(subqueries))
}
