//! An opened page: the parsed tree plus the flat rows the UI draws.

use rike_core::journal::JournalDate;
use rike_core::{Block, BlockId, Item, Page};

/// One visible block in the flat list. Rows are keyed by `id`, so a row keeps
/// its nodes (and a focused `Input`) when blocks move or change depth.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    pub id: BlockId,
    pub depth: usize,
    pub text: String,
    pub collapsed: bool,
    pub has_children: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Doc {
    pub rel: String,
    pub title: String,
    pub journal: Option<JournalDate>,
    pub page: Page,
    pub rows: Vec<Row>,
    /// False for a page that has no file yet; the file appears on the first
    /// non-empty save.
    pub exists: bool,
    pub read_only: bool,
    /// Edits not yet confirmed by a save.
    pub dirty: bool,
    /// The file changed on disk under unsaved edits and the user has not
    /// chosen a side yet; saving is blocked so the disk version survives.
    pub stale: bool,
}

impl Doc {
    pub fn new(
        rel: String,
        title: String,
        journal: Option<JournalDate>,
        mut page: Page,
        exists: bool,
        read_only: bool,
    ) -> Self {
        ensure_block(&mut page);
        let rows = rows_of(&page);
        Doc {
            rel,
            title,
            journal,
            page,
            rows,
            exists,
            read_only,
            dirty: false,
            stale: false,
        }
    }

    pub fn refresh(&mut self) {
        ensure_block(&mut self.page);
        self.rows = rows_of(&self.page);
    }

    pub fn row(&self, id: BlockId) -> Option<&Row> {
        self.rows.iter().find(|r| r.id == id)
    }

    /// Whether saving would write anything worth a file: a page that has no
    /// file yet and only empty blocks stays virtual.
    pub fn has_content(&self) -> bool {
        self.page.items.iter().any(|item| match item {
            Item::Block(b) => block_has_content(b),
            Item::PageProperties(p) => !p.is_empty(),
            Item::Raw(r) => !r.trim().is_empty(),
        })
    }
}

fn block_has_content(b: &Block) -> bool {
    !b.text.trim().is_empty()
        || !b.properties.is_empty()
        || b.children.iter().any(block_has_content)
}

/// A page always has a block to click into.
fn ensure_block(page: &mut Page) {
    if page.blocks().next().is_none() {
        page.items.push(Item::Block(Block::new("")));
    }
}

pub fn rows_of(page: &Page) -> Vec<Row> {
    fn go(b: &Block, depth: usize, out: &mut Vec<Row>) {
        out.push(Row {
            id: b.id,
            depth,
            text: b.text.clone(),
            collapsed: b.is_collapsed(),
            has_children: !b.children.is_empty(),
        });
        if !b.is_collapsed() {
            for c in &b.children {
                go(c, depth + 1, out);
            }
        }
    }
    let mut out = Vec::new();
    for b in page.blocks() {
        go(b, 0, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use rike_core::parse::parse_page;

    #[test]
    fn rows_skip_collapsed_children() {
        let parsed = parse_page("- a\n\tcollapsed:: true\n\t- b\n- c\n");
        let rows = rows_of(&parsed.page);
        assert_eq!(rows.len(), 2);
        assert!(rows[0].collapsed && rows[0].has_children);
    }

    #[test]
    fn empty_page_gets_one_block_and_stays_virtual() {
        let doc = Doc::new(
            "pages/x.md".into(),
            "x".into(),
            None,
            Page::default(),
            false,
            false,
        );
        assert_eq!(doc.rows.len(), 1);
        assert!(!doc.has_content());
    }
}
