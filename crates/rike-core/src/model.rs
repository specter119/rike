//! Page and block data model.
//!
//! The model is lossless by construction: anything the parser does not
//! understand is kept verbatim (`Item::Raw`), property keys keep their
//! original spelling, and only indentation whitespace and line endings are
//! normalized on serialization.

use uuid::Uuid;

/// Stable in-memory identity of a block.
///
/// Equals the persisted `id::` value when the source has a valid one,
/// otherwise a fresh random UUID that is only written to disk once the block
/// is referenced (see [`Block::ensure_persisted_id`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct BlockId(pub Uuid);

impl BlockId {
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for BlockId {
    fn default() -> Self {
        Self::new()
    }
}

/// One `key:: value` line. `raw_key` keeps the original spelling so that
/// serialization does not rewrite the user's text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Property {
    pub raw_key: String,
    pub value: String,
}

impl Property {
    pub fn new(raw_key: impl Into<String>, value: impl Into<String>) -> Self {
        Self {
            raw_key: raw_key.into(),
            value: value.into(),
        }
    }

    /// Normalized key used for lookups and indexing: lowercase, `_` → `-`.
    pub fn key(&self) -> String {
        self.raw_key.to_lowercase().replace('_', "-")
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Block {
    pub id: BlockId,
    /// Body text without bullet marker, indentation prefix or property lines.
    /// Multi-line bodies are joined with `\n`.
    pub text: String,
    /// All block properties in file order, including `id` and `collapsed`.
    pub properties: Vec<Property>,
    pub children: Vec<Block>,
}

impl Block {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            id: BlockId::new(),
            text: text.into(),
            properties: Vec::new(),
            children: Vec::new(),
        }
    }

    pub fn property(&self, key: &str) -> Option<&Property> {
        self.properties.iter().find(|p| p.key() == key)
    }

    /// True when an `id::` property exists in the source/model.
    pub fn has_persisted_id(&self) -> bool {
        self.property("id").is_some()
    }

    /// Adds an `id::` property equal to `self.id` if it is not there yet.
    /// Returns true when the model changed.
    pub fn ensure_persisted_id(&mut self) -> bool {
        if self.has_persisted_id() {
            return false;
        }
        self.properties
            .push(Property::new("id", self.id.0.to_string()));
        true
    }

    pub fn is_collapsed(&self) -> bool {
        self.property("collapsed")
            .is_some_and(|p| p.value.trim() == "true")
    }

    pub fn set_collapsed(&mut self, collapsed: bool) {
        self.properties.retain(|p| p.key() != "collapsed");
        if collapsed {
            self.properties.push(Property::new("collapsed", "true"));
        }
    }
}

/// A top-level piece of a page file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Item {
    /// `key:: value` lines before the first bullet.
    PageProperties(Vec<Property>),
    Block(Block),
    /// Verbatim lines the parser does not model (YAML front matter,
    /// paragraphs outside bullets, ...). Joined with `\n`, no trailing newline.
    Raw(String),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Page {
    pub items: Vec<Item>,
}

impl Page {
    pub fn blocks(&self) -> impl Iterator<Item = &Block> {
        self.items.iter().filter_map(|i| match i {
            Item::Block(b) => Some(b),
            _ => None,
        })
    }

    pub fn blocks_mut(&mut self) -> impl Iterator<Item = &mut Block> {
        self.items.iter_mut().filter_map(|i| match i {
            Item::Block(b) => Some(b),
            _ => None,
        })
    }

    pub fn page_properties(&self) -> impl Iterator<Item = &Property> {
        self.items.iter().flat_map(|i| match i {
            Item::PageProperties(ps) => ps.as_slice(),
            _ => &[],
        })
    }

    pub fn page_property(&self, key: &str) -> Option<&Property> {
        self.page_properties().find(|p| p.key() == key)
    }

    /// Depth-first walk over all blocks in document order.
    pub fn walk(&self) -> Vec<WalkEntry<'_>> {
        fn go<'a>(
            b: &'a Block,
            depth: usize,
            parent: Option<BlockId>,
            out: &mut Vec<WalkEntry<'a>>,
        ) {
            out.push(WalkEntry {
                block: b,
                depth,
                parent,
            });
            for c in &b.children {
                go(c, depth + 1, Some(b.id), out);
            }
        }
        let mut out = Vec::new();
        for b in self.blocks() {
            go(b, 0, None, &mut out);
        }
        out
    }

    pub fn find(&self, id: BlockId) -> Option<&Block> {
        self.walk()
            .into_iter()
            .map(|e| e.block)
            .find(|b| b.id == id)
    }
}

#[derive(Clone, Copy, Debug)]
pub struct WalkEntry<'a> {
    pub block: &'a Block,
    pub depth: usize,
    pub parent: Option<BlockId>,
}
