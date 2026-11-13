//! rike-index: derived, rebuildable index over parsed pages.
//!
//! Everything here can be recomputed from the Markdown files. The [`Index`]
//! trait keeps the storage replaceable (an in-memory map now, HawDB later).

mod fts;
mod mem;
mod title;

use std::collections::BTreeSet;

use rike_core::BlockId;
use rike_core::config::VaultConfig;
use rike_core::journal::JournalDate;
use rike_core::names::PageKey;

pub use mem::MemIndex;
pub use title::resolve_title;

/// What one index mutation invalidated. Backends report this so the session
/// can tell the UI precisely which derived data went stale, instead of
/// dropping a global version number and re-deriving everything.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PageDelta {
    /// Page keys whose backlink groups changed: every key the page
    /// referenced before or after the mutation. The page's own old and new
    /// key are included only when the key itself changed — a page never
    /// appears in its own backlink group, but a rename moves the query.
    pub backlink_keys: BTreeSet<PageKey>,
}

/// Index mutations accumulated between two `IndexVer` events.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IndexChanges {
    /// Pages upserted since the last flush.
    pub changed: BTreeSet<String>,
    /// Pages removed since the last flush.
    pub removed: BTreeSet<String>,
    /// Page keys whose backlink groups changed since the last flush.
    pub backlink_keys: BTreeSet<PageKey>,
}

impl IndexChanges {
    pub fn record_upsert(&mut self, rel: &str, delta: PageDelta) {
        self.changed.insert(rel.to_owned());
        self.removed.remove(rel);
        self.backlink_keys.extend(delta.backlink_keys);
    }

    pub fn record_remove(&mut self, rel: &str, delta: PageDelta) {
        self.removed.insert(rel.to_owned());
        self.changed.remove(rel);
        self.backlink_keys.extend(delta.backlink_keys);
    }

    /// Folds a later flush's changes into this one; used when coalescing.
    pub fn merge(&mut self, later: IndexChanges) {
        for rel in &later.removed {
            self.changed.remove(rel);
        }
        for rel in &later.changed {
            self.removed.remove(rel);
        }
        self.changed.extend(later.changed);
        self.removed.extend(later.removed);
        self.backlink_keys.extend(later.backlink_keys);
    }

    pub fn is_empty(&self) -> bool {
        self.changed.is_empty() && self.removed.is_empty() && self.backlink_keys.is_empty()
    }
}

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

/// A block matching a search, with its page.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BlockHit {
    pub page: PageEntry,
    pub block: BlockId,
    pub text: String,
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
    /// Inserts or replaces everything known about `source.rel_path` and
    /// reports what the change invalidated.
    fn upsert_page(&mut self, source: PageSource<'_>) -> PageDelta;
    /// Removes `rel_path` and reports what the removal invalidated.
    fn remove_page(&mut self, rel_path: &str) -> PageDelta;

    /// Starts a bulk load: a backend that pays for each commit may defer them
    /// to `commit`. Nothing written during a batch is searchable until then.
    fn begin_batch(&mut self) {}
    /// Commits what the batch wrote so far, and keeps deferring new writes.
    fn commit(&mut self) {}
    /// Ends a bulk load: commits, then writes commit as they come again.
    fn end_batch(&mut self) {}

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

    /// Case-insensitive substring search over block text only, so page title
    /// hits cannot eat the completion panel's budget.
    fn search_blocks(&self, query: &str, limit: usize) -> Vec<BlockHit>;

    fn warnings(&self) -> Vec<IndexWarning>;
}
