//! Outline parser (line based, lossless).
//!
//! The parser understands the Logseq file-graph subset described in
//! `AGENTS.md`. Everything else is kept verbatim as [`Item::Raw`]. On read it
//! accepts tab and space indentation, CRLF and a leading BOM; serialization
//! always writes the canonical form (tab indentation, LF).
//!
//! Indentation is compared by the number of leading space/tab characters, so a
//! level is "deeper" than its parent when its prefix is longer. That tolerates
//! files written with two spaces per level as well as files that mix tabs and
//! spaces, without needing to infer a fixed indent unit.

use std::collections::HashSet;

use uuid::Uuid;

use crate::model::{Block, BlockId, Item, Page, Property};
use crate::serialize::serialize_page;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WarningKind {
    /// A second block carries an `id::` already used earlier in the page.
    DuplicateBlockId,
    /// Content kept as `Item::Raw` because it is not modeled.
    RawContent,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Warning {
    /// 1-based line number in the source.
    pub line: usize,
    pub kind: WarningKind,
    pub message: String,
}

#[derive(Clone, Debug)]
pub struct Parsed {
    pub page: Page,
    /// False when `serialize(parse(src))` differs from `src` by anything other
    /// than indentation whitespace and line endings. The caller must treat
    /// such a page as read-only.
    pub lossless: bool,
    pub warnings: Vec<Warning>,
}

/// Parses a Logseq-style outline file. Accepts tab and space indentation,
/// CRLF and a leading BOM (the BOM is dropped).
pub fn parse_page(src: &str) -> Parsed {
    let mut parser = Parser {
        lines: split_lines(src),
        idx: 0,
        warnings: Vec::new(),
        seen_ids: HashSet::new(),
    };
    let page = parser.parse();
    let lossless = normalize(src) == normalize(&serialize_page(&page));
    Parsed {
        page,
        lossless,
        warnings: parser.warnings,
    }
}

/// Splits into lines, dropping one trailing artificial empty line for a source
/// that ends with a newline. CRLF line endings are reduced to their content.
fn split_lines(src: &str) -> Vec<&str> {
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    if src.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<&str> = src
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line))
        .collect();
    if lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    lines
}

/// Comparison form used for the losslessness self-check: BOM stripped, LF line
/// endings, leading and trailing whitespace of every line ignored, trailing
/// empty lines dropped. Only content is preserved, whitespace is not.
fn normalize(src: &str) -> String {
    let src = src.strip_prefix('\u{feff}').unwrap_or(src);
    let mut lines: Vec<&str> = src
        .split('\n')
        .map(|line| line.strip_suffix('\r').unwrap_or(line).trim())
        .collect();
    while lines.last().is_some_and(|line| line.is_empty()) {
        lines.pop();
    }
    lines.join("\n")
}

/// Number of leading indentation characters. Space and tab are one byte each,
/// so this doubles as the byte length of the indentation prefix.
fn indent_of(line: &str) -> usize {
    line.chars().take_while(|c| *c == ' ' || *c == '\t').count()
}

/// Body text of a bullet line, i.e. `- ` removed. A bare `-` (empty block) and
/// `- ` both yield `""`.
fn bullet_first_text(line: &str, indent: usize) -> Option<&str> {
    let rest = &line[indent..];
    if rest == "-" {
        return Some("");
    }
    rest.strip_prefix("- ")
}

/// Parses one `key:: value` line. A property needs a non-empty key without
/// whitespace or `:` and either a single space before a non-empty value or no
/// value at all; anything else stays body text so that no byte is rewritten.
fn parse_property(line: &str) -> Option<Property> {
    let (key, after) = line.split_once("::")?;
    if key.is_empty() || key.chars().any(|c| c.is_whitespace() || c == ':') {
        return None;
    }
    if after.is_empty() {
        return Some(Property::new(key, ""));
    }
    let value = after.strip_prefix(' ')?;
    if value.is_empty() {
        return None;
    }
    Some(Property::new(key, value))
}

/// Removes the continuation prefix of a body line: the canonical
/// `indent + two spaces` when present, otherwise the bullet indentation plus up
/// to two indentation characters. Lines that do not extend the bullet
/// indentation lose all leading whitespace.
fn strip_continuation<'a>(line: &'a str, prefix: &str, cont: &str) -> &'a str {
    if let Some(rest) = line.strip_prefix(cont) {
        return rest;
    }
    if let Some(rest) = line.strip_prefix(prefix) {
        let cut = rest
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .take(2)
            .map(char::len_utf8)
            .sum();
        return &rest[cut..];
    }
    line.trim_start()
}

/// Opening code fence: a run of at least three backticks or tildes. The
/// backtick variant must not contain a backtick in its info string.
pub(crate) fn fence_open(line: &str) -> Option<(char, usize)> {
    let trimmed = line.trim_start();
    let marker = trimmed.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let count = trimmed.chars().take_while(|c| *c == marker).count();
    if count < 3 || marker == '`' && trimmed[count..].contains('`') {
        return None;
    }
    Some((marker, count))
}

/// Closing code fence: at least as many markers as the opener, nothing else.
pub(crate) fn fence_close(line: &str, marker: char, count: usize) -> bool {
    let trimmed = line.trim_start();
    let run = trimmed.chars().take_while(|c| *c == marker).count();
    run >= count && trimmed[run..].trim().is_empty()
}

struct Parser<'a> {
    lines: Vec<&'a str>,
    idx: usize,
    warnings: Vec<Warning>,
    seen_ids: HashSet<BlockId>,
}

impl<'a> Parser<'a> {
    fn parse(&mut self) -> Page {
        let mut items: Vec<Item> = Vec::new();
        let mut raw: Option<(String, usize)> = None;
        let mut seen_block = false;
        while self.idx < self.lines.len() {
            let line = self.lines[self.idx];
            let line_no = self.idx + 1;
            let indent = indent_of(line);
            // Top-level bullets must start at column 0; shallow indented
            // markdown lists therefore stay raw instead of becoming blocks.
            if indent == 0 && bullet_first_text(line, 0).is_some() {
                flush_raw(&mut items, &mut raw, &mut self.warnings);
                seen_block = true;
                for block in self.parse_level(0) {
                    items.push(Item::Block(block));
                }
                continue;
            }
            if !seen_block
                && indent == 0
                && let Some(property) = parse_property(line)
            {
                flush_raw(&mut items, &mut raw, &mut self.warnings);
                match items.last_mut() {
                    Some(Item::PageProperties(properties)) => properties.push(property),
                    _ => items.push(Item::PageProperties(vec![property])),
                }
                self.idx += 1;
                continue;
            }
            push_raw(&mut raw, line, line_no);
            self.idx += 1;
        }
        flush_raw(&mut items, &mut raw, &mut self.warnings);
        Page { items }
    }

    /// Parses the blocks of one indentation level. The first bullet of a level
    /// fixes the indent width for its siblings; skipped levels attach to the
    /// nearest shallower block.
    fn parse_level(&mut self, width: usize) -> Vec<Block> {
        let mut blocks = Vec::new();
        while self.idx < self.lines.len() {
            let line = self.lines[self.idx];
            let indent = indent_of(line);
            if indent != width || bullet_first_text(line, indent).is_none() {
                break;
            }
            blocks.push(self.parse_block(indent));
        }
        blocks
    }

    fn parse_block(&mut self, width: usize) -> Block {
        let line = self.lines[self.idx];
        let first_line_no = self.idx + 1;
        self.idx += 1;
        let prefix = &line[..width];
        let cont = format!("{prefix}  ");
        let first = bullet_first_text(line, width).unwrap_or_default();

        let mut text: Vec<&'a str> = vec![first];
        let mut properties: Vec<Property> = Vec::new();
        let mut id_line: Option<usize> = None;
        let mut property_phase = true;
        let mut fence = fence_open(first);
        if fence.is_some() {
            property_phase = false;
        }

        while self.idx < self.lines.len() {
            let current = self.lines[self.idx];
            if let Some((marker, count)) = fence {
                if current.trim().is_empty() {
                    text.push("");
                    self.idx += 1;
                    continue;
                }
                if indent_of(current) <= width {
                    break;
                }
                let body = strip_continuation(current, prefix, &cont);
                if fence_close(body, marker, count) {
                    fence = None;
                }
                text.push(body);
                self.idx += 1;
                continue;
            }
            if current.trim().is_empty() {
                // Interior blank lines are kept, trailing ones are left for
                // the caller (they stay raw at the top level).
                let mut next = self.idx;
                while next < self.lines.len() && self.lines[next].trim().is_empty() {
                    next += 1;
                }
                if next >= self.lines.len() || indent_of(self.lines[next]) <= width {
                    break;
                }
                text.extend(std::iter::repeat_n("", next - self.idx));
                property_phase = false;
                self.idx = next;
                continue;
            }
            let indent = indent_of(current);
            if indent <= width || bullet_first_text(current, indent).is_some() {
                break;
            }
            let body = strip_continuation(current, prefix, &cont);
            if property_phase {
                if let Some(property) = parse_property(body) {
                    if property.key() == "id" && id_line.is_none() {
                        id_line = Some(self.idx + 1);
                    }
                    properties.push(property);
                    self.idx += 1;
                    continue;
                }
                property_phase = false;
            }
            if let Some(open) = fence_open(body) {
                fence = Some(open);
            }
            text.push(body);
            self.idx += 1;
        }

        let id = self.assign_id(&properties, id_line.unwrap_or(first_line_no));
        let children = self.parse_children(width);
        Block {
            id,
            text: text.join("\n"),
            properties,
            children,
        }
    }

    fn parse_children(&mut self, parent_width: usize) -> Vec<Block> {
        if self.idx >= self.lines.len() {
            return Vec::new();
        }
        let line = self.lines[self.idx];
        let indent = indent_of(line);
        if indent <= parent_width || bullet_first_text(line, indent).is_none() {
            return Vec::new();
        }
        self.parse_level(indent)
    }

    /// Resolves a block id from a valid 36-character `id::` value. The first
    /// occurrence wins; later duplicates get a fresh id and a warning while
    /// keeping the property text untouched.
    fn assign_id(&mut self, properties: &[Property], line: usize) -> BlockId {
        let parsed = properties
            .iter()
            .find(|property| property.key() == "id")
            .and_then(|property| {
                let value = property.value.trim();
                if value.len() == 36 {
                    Uuid::parse_str(value).ok()
                } else {
                    None
                }
            });
        match parsed {
            Some(uuid) => {
                let id = BlockId(uuid);
                if self.seen_ids.insert(id) {
                    id
                } else {
                    self.warnings.push(Warning {
                        line,
                        kind: WarningKind::DuplicateBlockId,
                        message: format!(
                            "duplicate block id `{uuid}`; a fresh in-memory id was assigned"
                        ),
                    });
                    BlockId::new()
                }
            }
            None => BlockId::new(),
        }
    }
}

fn push_raw(raw: &mut Option<(String, usize)>, line: &str, line_no: usize) {
    match raw {
        Some((text, _)) => {
            text.push('\n');
            text.push_str(line);
        }
        None => *raw = Some((line.to_owned(), line_no)),
    }
}

fn flush_raw(
    items: &mut Vec<Item>,
    raw: &mut Option<(String, usize)>,
    warnings: &mut Vec<Warning>,
) {
    let Some((text, line)) = raw.take() else {
        return;
    };
    if !text.trim().is_empty() {
        warnings.push(Warning {
            line,
            kind: WarningKind::RawContent,
            message: "content is not modeled and is kept verbatim".to_owned(),
        });
    }
    items.push(Item::Raw(text));
}
