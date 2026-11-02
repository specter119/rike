//! rike-index: derived, rebuildable index over parsed pages.
//!
//! Everything here can be recomputed from the Markdown files. The [`Index`]
//! trait keeps the storage replaceable (an in-memory map now, HawDB later).

mod mem;
mod title;

use rike_core::BlockId;
use rike_core::config::VaultConfig;
use rike_core::journal::JournalDate;
use rike_core::names::PageKey;

pub use mem::MemIndex;
pub use title::resolve_title;

/// One indexed page file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageEntry {
    pub key: PageKey,
    pub title: String,
    pub rel_path: String,
    /// Set for files in `journals/` whose name matches the journal format.
    pub journal: Option<JournalDate>,
}

/// Input for [`Index::upsert_page`].
pub struct PageSource<'a> {
    pub rel_path: &'a str,
    pub page: &'a rike_core::Page,
    pub config: &'a VaultConfig,
}

/// A block that references a page, with the path of its ancestors.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BacklinkItem {
    pub block: BlockId,
    pub text: String,
    /// First lines of the ancestors, outermost first.
    pub ancestors: Vec<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BacklinkGroup {
    pub source: PageEntry,
    pub items: Vec<BacklinkItem>,
}

/// Where a block lives; used to render `((uuid))` and to navigate to it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockInfo {
    pub id: BlockId,
    pub text: String,
    pub page: PageEntry,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SearchHit {
    Page(PageEntry),
    Block {
        page: PageEntry,
        block: BlockId,
        text: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IndexWarning {
    /// Several files map to the same page key. Nothing is merged.
    DuplicatePageKey {
        key: PageKey,
        rel_paths: Vec<String>,
    },
    /// The same `id::` is used by blocks in different pages; the first wins.
    DuplicateBlockId { id: BlockId, rel_paths: Vec<String> },
}

pub trait Index {
    /// Inserts or replaces everything known about `source.rel_path`.
    fn upsert_page(&mut self, source: PageSource<'_>);
    fn remove_page(&mut self, rel_path: &str);

    /// All pages sorted by title (case-insensitive).
    fn pages(&self) -> Vec<PageEntry>;
    /// Journal pages, newest first.
    fn journals(&self) -> Vec<PageEntry>;
    fn page(&self, key: &PageKey) -> Option<PageEntry>;
    fn page_by_path(&self, rel_path: &str) -> Option<PageEntry>;

    /// Blocks referencing `key` through `[[x]]`, `#x` or property values,
    /// grouped by source page. References from the page itself are omitted.
    fn backlinks(&self, key: &PageKey) -> Vec<BacklinkGroup>;
    fn block(&self, id: BlockId) -> Option<BlockInfo>;

    /// Case-insensitive substring search over titles and block text.
    /// Title matches come first.
    fn search(&self, query: &str, limit: usize) -> Vec<SearchHit>;

    fn warnings(&self) -> Vec<IndexWarning>;
}
