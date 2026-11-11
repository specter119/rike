use std::collections::{BTreeMap, BTreeSet, HashMap};

use rike_core::BlockId;
use rike_core::inline::{InlineKind, tokenize};
use rike_core::names::PageKey;

use crate::{
    BacklinkGroup, BacklinkItem, BlockHit, BlockInfo, Index, IndexWarning, PageDelta, PageEntry,
    PageSource, SearchHit, resolve_title,
};

struct BlockRecord {
    id: BlockId,
    text: String,
    lower: String,
    ancestors: Vec<String>,
}

struct PageData {
    entry: PageEntry,
    title_lower: String,
    blocks: Vec<BlockRecord>,
    /// Page keys referenced by this page, remembered to undo them on update.
    refs: BTreeSet<(PageKey, usize)>,
}

/// In-memory [`Index`]. Backlinks use an inverted map so that updating one
/// page only touches that page's entries.
#[derive(Default)]
pub struct MemIndex {
    pages: BTreeMap<String, PageData>,
    by_key: HashMap<PageKey, BTreeSet<String>>,
    refs_to: HashMap<PageKey, BTreeSet<(String, usize)>>,
    block_ids: HashMap<BlockId, BTreeSet<(String, usize)>>,
}

impl MemIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Journals newest first, then other pages by title.
    fn ordered(&self) -> Vec<&PageData> {
        let mut pages: Vec<&PageData> = self.pages.values().collect();
        pages.sort_by(|a, b| {
            let rank = |p: &PageData| match p.entry.journal {
                Some(d) => (0, std::cmp::Reverse(Some(d))),
                None => (1, std::cmp::Reverse(None)),
            };
            rank(a)
                .cmp(&rank(b))
                .then_with(|| a.title_lower.cmp(&b.title_lower))
                .then_with(|| a.entry.rel_path.cmp(&b.entry.rel_path))
        });
        pages
    }
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or("").to_owned()
}

/// Page keys referenced from `[[x]]`, `#x` and property values of a block.
fn referenced_keys(block: &rike_core::Block) -> BTreeSet<PageKey> {
    let mut keys = BTreeSet::new();
    scan_text(&mut keys, &block.text);
    for property in &block.properties {
        match property.key().as_str() {
            "id" | "collapsed" => {}
            "tags" => {
                for part in property.value.split(',') {
                    let part = part.trim().trim_start_matches('#');
                    let part = part
                        .strip_prefix("[[")
                        .and_then(|p| p.strip_suffix("]]"))
                        .unwrap_or(part);
                    add_key(&mut keys, part);
                }
                scan_text(&mut keys, &property.value);
            }
            _ => scan_text(&mut keys, &property.value),
        }
    }
    keys
}

fn add_key(keys: &mut BTreeSet<PageKey>, name: &str) {
    let key = PageKey::new(name);
    if !key.as_str().is_empty() {
        keys.insert(key);
    }
}

fn scan_text(keys: &mut BTreeSet<PageKey>, text: &str) {
    for token in tokenize(text) {
        if let InlineKind::PageRef { name } | InlineKind::Tag { name } = token.kind {
            add_key(keys, &name);
        }
    }
}

impl Index for MemIndex {
    fn upsert_page(&mut self, source: PageSource<'_>) -> PageDelta {
        let old_key = self
            .pages
            .get(source.rel_path)
            .map(|old| old.entry.key.clone());
        let mut delta = self.remove_page(source.rel_path);

        let (title, journal) = resolve_title(source.rel_path, source.page, source.config);
        let key = PageKey::new(&title);
        let rel = source.rel_path.to_owned();

        let mut blocks = Vec::new();
        let mut refs = BTreeSet::new();
        let mut stack: Vec<String> = Vec::new();
        for (idx, entry) in source.page.walk().into_iter().enumerate() {
            stack.truncate(entry.depth);
            let block = entry.block;
            for referenced in referenced_keys(block) {
                self.refs_to
                    .entry(referenced.clone())
                    .or_default()
                    .insert((rel.clone(), idx));
                refs.insert((referenced, idx));
            }
            self.block_ids
                .entry(block.id)
                .or_default()
                .insert((rel.clone(), idx));
            blocks.push(BlockRecord {
                id: block.id,
                text: block.text.clone(),
                lower: block.text.to_lowercase(),
                ancestors: stack.clone(),
            });
            stack.push(first_line(&block.text));
        }

        delta
            .backlink_keys
            .extend(refs.iter().map(|(key, _)| key.clone()));
        if let Some(old) = old_key
            && old != key
        {
            delta.backlink_keys.insert(old);
            delta.backlink_keys.insert(key.clone());
        }

        self.by_key
            .entry(key.clone())
            .or_default()
            .insert(rel.clone());
        self.pages.insert(
            rel.clone(),
            PageData {
                title_lower: title.to_lowercase(),
                entry: PageEntry {
                    key,
                    title,
                    rel_path: rel,
                    journal,
                },
                blocks,
                refs,
            },
        );
        delta
    }

    fn remove_page(&mut self, rel_path: &str) -> PageDelta {
        let Some(old) = self.pages.remove(rel_path) else {
            return PageDelta::default();
        };
        if let Some(paths) = self.by_key.get_mut(&old.entry.key) {
            paths.remove(rel_path);
            if paths.is_empty() {
                self.by_key.remove(&old.entry.key);
            }
        }
        for (key, idx) in &old.refs {
            if let Some(set) = self.refs_to.get_mut(key) {
                set.remove(&(rel_path.to_owned(), *idx));
                if set.is_empty() {
                    self.refs_to.remove(key);
                }
            }
        }
        for (idx, block) in old.blocks.iter().enumerate() {
            if let Some(set) = self.block_ids.get_mut(&block.id) {
                set.remove(&(rel_path.to_owned(), idx));
                if set.is_empty() {
                    self.block_ids.remove(&block.id);
                }
            }
        }
        PageDelta {
            backlink_keys: old.refs.iter().map(|(key, _)| key.clone()).collect(),
        }
    }

    fn pages(&self) -> Vec<PageEntry> {
        let mut pages: Vec<&PageData> = self.pages.values().collect();
        pages.sort_by(|a, b| {
            a.title_lower
                .cmp(&b.title_lower)
                .then_with(|| a.entry.rel_path.cmp(&b.entry.rel_path))
        });
        pages.into_iter().map(|p| p.entry.clone()).collect()
    }

    fn journals(&self) -> Vec<PageEntry> {
        self.ordered()
            .into_iter()
            .filter(|p| p.entry.journal.is_some())
            .map(|p| p.entry.clone())
            .collect()
    }

    fn page(&self, key: &PageKey) -> Option<PageEntry> {
        let rel = self.by_key.get(key)?.iter().next()?;
        self.pages.get(rel).map(|p| p.entry.clone())
    }

    fn page_by_path(&self, rel_path: &str) -> Option<PageEntry> {
        self.pages.get(rel_path).map(|p| p.entry.clone())
    }

    fn backlinks(&self, key: &PageKey) -> Vec<BacklinkGroup> {
        let Some(refs) = self.refs_to.get(key) else {
            return Vec::new();
        };
        let mut groups: BTreeMap<&str, BacklinkGroup> = BTreeMap::new();
        for (rel, idx) in refs {
            let Some(page) = self.pages.get(rel) else {
                continue;
            };
            if &page.entry.key == key {
                continue;
            }
            let block = &page.blocks[*idx];
            groups
                .entry(rel.as_str())
                .or_insert_with(|| BacklinkGroup {
                    source: page.entry.clone(),
                    items: Vec::new(),
                })
                .items
                .push(BacklinkItem {
                    block: block.id,
                    text: block.text.clone(),
                    ancestors: block.ancestors.clone(),
                });
        }
        let mut out: Vec<BacklinkGroup> = groups.into_values().collect();
        out.sort_by(|a, b| {
            let rank = |g: &BacklinkGroup| match g.source.journal {
                Some(d) => (0, std::cmp::Reverse(Some(d))),
                None => (1, std::cmp::Reverse(None)),
            };
            rank(a).cmp(&rank(b)).then_with(|| {
                a.source
                    .title
                    .to_lowercase()
                    .cmp(&b.source.title.to_lowercase())
            })
        });
        out
    }

    fn block(&self, id: BlockId) -> Option<BlockInfo> {
        let (rel, idx) = self.block_ids.get(&id)?.iter().next()?;
        let page = self.pages.get(rel)?;
        Some(BlockInfo {
            id,
            text: page.blocks[*idx].text.clone(),
            page: page.entry.clone(),
        })
    }

    fn search(&self, query: &str, limit: usize) -> Vec<SearchHit> {
        let query = query.trim().to_lowercase();
        if query.is_empty() || limit == 0 {
            return Vec::new();
        }
        let ordered = self.ordered();
        let mut hits = Vec::new();
        for page in &ordered {
            if page.title_lower.contains(&query) {
                hits.push(SearchHit::Page(page.entry.clone()));
                if hits.len() >= limit {
                    return hits;
                }
            }
        }
        for page in &ordered {
            for block in &page.blocks {
                if block.lower.contains(&query) {
                    hits.push(SearchHit::Block {
                        page: page.entry.clone(),
                        block: block.id,
                        text: block.text.clone(),
                    });
                    if hits.len() >= limit {
                        return hits;
                    }
                }
            }
        }
        hits
    }

    fn search_blocks(&self, query: &str, limit: usize) -> Vec<BlockHit> {
        let query = query.trim().to_lowercase();
        if query.is_empty() || limit == 0 {
            return Vec::new();
        }
        let mut hits = Vec::new();
        for page in self.ordered() {
            for block in &page.blocks {
                if block.lower.contains(&query) {
                    hits.push(BlockHit {
                        page: page.entry.clone(),
                        block: block.id,
                        text: block.text.clone(),
                    });
                    if hits.len() >= limit {
                        return hits;
                    }
                }
            }
        }
        hits
    }

    fn warnings(&self) -> Vec<IndexWarning> {
        let mut out = Vec::new();
        let mut keys: Vec<_> = self.by_key.iter().filter(|(_, p)| p.len() > 1).collect();
        keys.sort_by(|a, b| a.0.cmp(b.0));
        for (key, paths) in keys {
            out.push(IndexWarning::DuplicatePageKey {
                key: key.clone(),
                rel_paths: paths.iter().cloned().collect(),
            });
        }
        let mut ids: Vec<_> = self
            .block_ids
            .iter()
            .filter(|(_, occ)| occ.len() > 1)
            .collect();
        ids.sort_by(|a, b| a.0.cmp(b.0));
        for (id, occurrences) in ids {
            out.push(IndexWarning::DuplicateBlockId {
                id: *id,
                rel_paths: occurrences.iter().map(|(rel, _)| rel.clone()).collect(),
            });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use rike_core::config::VaultConfig;
    use rike_core::parse::parse_page;

    use super::*;
    use crate::IndexChanges;

    fn put(index: &mut MemIndex, rel: &str, src: &str) -> PageDelta {
        let page = parse_page(src).page;
        index.upsert_page(PageSource {
            rel_path: rel,
            page: &page,
            config: &VaultConfig::default(),
        })
    }

    fn key(name: &str) -> PageKey {
        PageKey::new(name)
    }

    #[test]
    fn upsert_delta_reports_referenced_and_renamed_keys() {
        let mut index = MemIndex::new();
        let delta = put(&mut index, "pages/a.md", "- ref [[b]]");
        assert_eq!(delta.backlink_keys, BTreeSet::from([key("b")]));

        // Re-upserting the same refs reports them again: consumers that
        // missed the previous flush still see the group as dirty.
        let delta = put(&mut index, "pages/a.md", "- ref [[b]] and #c");
        assert_eq!(delta.backlink_keys, BTreeSet::from([key("b"), key("c")]));

        // Renaming adds the moved key — old and new — to the delta.
        put(&mut index, "pages/b.md", "title:: B\n- a");
        let delta = put(&mut index, "pages/b.md", "title:: C\n- a");
        assert!(delta.backlink_keys.contains(&key("b")));
        assert!(delta.backlink_keys.contains(&key("c")));

        // A page without references and without a rename dirties nothing.
        let delta = put(&mut index, "pages/d.md", "- plain text");
        assert!(delta.backlink_keys.is_empty());
    }

    #[test]
    fn remove_delta_reports_the_old_references() {
        let mut index = MemIndex::new();
        put(&mut index, "pages/a.md", "- ref [[b]]");
        let delta = index.remove_page("pages/a.md");
        assert_eq!(delta.backlink_keys, BTreeSet::from([key("b")]));

        // Removing an unknown page reports nothing.
        assert!(
            index
                .remove_page("pages/missing.md")
                .backlink_keys
                .is_empty()
        );
    }

    #[test]
    fn index_changes_supersede_within_a_window() {
        let mut changes = IndexChanges::default();
        changes.record_upsert("pages/a.md", PageDelta::default());
        changes.record_remove("pages/a.md", PageDelta::default());
        assert!(changes.changed.is_empty());
        assert_eq!(changes.removed, BTreeSet::from(["pages/a.md".to_owned()]));

        changes.record_upsert("pages/a.md", PageDelta::default());
        assert_eq!(changes.changed, BTreeSet::from(["pages/a.md".to_owned()]));
        assert!(changes.removed.is_empty());

        let mut later = IndexChanges::default();
        later.record_remove("pages/a.md", PageDelta::default());
        changes.merge(later);
        assert!(changes.changed.is_empty());
        assert_eq!(changes.removed, BTreeSet::from(["pages/a.md".to_owned()]));
    }

    #[test]
    fn backlinks_group_by_source_and_carry_ancestors() {
        let mut index = MemIndex::new();
        put(&mut index, "pages/Target.md", "- hello\n");
        put(
            &mut index,
            "pages/A.md",
            "- parent\n\t- see [[target]] and #Target\n- other\n",
        );
        put(&mut index, "pages/B.md", "- tagged #[[Target]]\n");

        let groups = index.backlinks(&key("Target"));
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].source.title, "A");
        assert_eq!(groups[0].items.len(), 1);
        assert_eq!(groups[0].items[0].ancestors, vec!["parent".to_owned()]);
        assert_eq!(groups[1].source.title, "B");
    }

    #[test]
    fn self_references_are_omitted() {
        let mut index = MemIndex::new();
        put(&mut index, "pages/Loop.md", "- about [[Loop]]\n");
        assert!(index.backlinks(&key("loop")).is_empty());
    }

    #[test]
    fn upsert_replaces_old_references_and_remove_clears_them() {
        let mut index = MemIndex::new();
        put(&mut index, "pages/A.md", "- [[X]]\n");
        assert_eq!(index.backlinks(&key("x")).len(), 1);
        put(&mut index, "pages/A.md", "- [[Y]]\n");
        assert!(index.backlinks(&key("x")).is_empty());
        assert_eq!(index.backlinks(&key("y")).len(), 1);
        index.remove_page("pages/A.md");
        assert!(index.backlinks(&key("y")).is_empty());
        assert!(index.pages().is_empty());
    }

    #[test]
    fn tags_property_and_property_values_count_as_references() {
        let mut index = MemIndex::new();
        put(
            &mut index,
            "pages/A.md",
            "- x\n  tags:: alpha, [[beta]]\n  related:: [[gamma]]\n",
        );
        for name in ["alpha", "beta", "gamma"] {
            assert_eq!(index.backlinks(&key(name)).len(), 1, "{name}");
        }
    }

    #[test]
    fn journal_titles_resolve_so_old_links_still_work() {
        let mut index = MemIndex::new();
        put(&mut index, "journals/2026_10_31.md", "- today\n");
        put(&mut index, "journals/2026_10_30.md", "- yesterday\n");
        put(&mut index, "pages/A.md", "- met on [[Oct 31st, 2026]]\n");

        let entry = index.page(&key("Oct 31st, 2026")).unwrap();
        assert_eq!(entry.rel_path, "journals/2026_10_31.md");
        let journals = index.journals();
        assert_eq!(journals[0].rel_path, "journals/2026_10_31.md");
        assert_eq!(journals[1].rel_path, "journals/2026_10_30.md");
        assert_eq!(index.backlinks(&key("oct 31st, 2026")).len(), 1);
    }

    #[test]
    fn block_lookup_by_id() {
        let mut index = MemIndex::new();
        let id = "6356e902-3b7b-4cb2-8c3e-6a904c813c40";
        put(
            &mut index,
            "pages/A.md",
            &format!("- target\n  id:: {id}\n"),
        );
        let info = index.block(BlockId(id.parse().unwrap())).unwrap();
        assert_eq!(info.text, "target");
        assert_eq!(info.page.title, "A");
    }

    #[test]
    fn search_is_case_insensitive_and_titles_come_first() {
        let mut index = MemIndex::new();
        put(&mut index, "pages/Rust.md", "- nothing\n");
        put(&mut index, "pages/B.md", "- I like RUST a lot\n");
        let hits = index.search("rust", 10);
        assert!(matches!(&hits[0], SearchHit::Page(p) if p.title == "Rust"));
        assert!(matches!(&hits[1], SearchHit::Block { text, .. } if text.contains("RUST")));
        assert_eq!(index.search("rust", 1).len(), 1);
        assert!(index.search("  ", 10).is_empty());
    }

    #[test]
    fn search_matches_chinese_text() {
        let mut index = MemIndex::new();
        put(&mut index, "pages/笔记.md", "- 三省吾身\n");
        assert_eq!(index.search("吾身", 10).len(), 1);
        assert_eq!(index.search("笔记", 10).len(), 1);
    }

    #[test]
    fn colliding_page_keys_are_reported_not_merged() {
        let mut index = MemIndex::new();
        put(&mut index, "pages/Foo.md", "- a\n");
        put(&mut index, "pages/foo.md", "- b\n");
        assert_eq!(index.pages().len(), 2);
        assert!(matches!(
            index.warnings().as_slice(),
            [IndexWarning::DuplicatePageKey { rel_paths, .. }] if rel_paths.len() == 2
        ));
    }

    #[test]
    fn same_block_id_in_two_pages_warns() {
        let mut index = MemIndex::new();
        let id = "11111111-2222-4333-8444-555555555555";
        put(&mut index, "pages/A.md", &format!("- a\n  id:: {id}\n"));
        put(&mut index, "pages/B.md", &format!("- b\n  id:: {id}\n"));
        assert!(matches!(
            index.warnings().as_slice(),
            [IndexWarning::DuplicateBlockId { .. }]
        ));
        assert_eq!(index.block(BlockId(id.parse().unwrap())).unwrap().text, "a");
    }
}
