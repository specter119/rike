//! Canonical serialization: LF line endings, tab indentation, `- ` bullets,
//! continuation lines indented by parent indent + 2 spaces.

use crate::model::{Block, Item, Page, Property};

/// Marker of a bullet. A block with an empty body is written as a bare `-`
/// (no trailing space); the parser accepts `-` and `- ` alike.
const BULLET: &str = "-";

/// Extra indentation of continuation lines and block properties relative to
/// the bullet of the owning block.
const CONTINUATION: &str = "  ";

pub fn serialize_page(page: &Page) -> String {
    let mut out = String::new();
    for item in &page.items {
        match item {
            Item::PageProperties(properties) => {
                for property in properties {
                    write_property(&mut out, "", property);
                }
            }
            Item::Raw(text) => {
                out.push_str(text);
                out.push('\n');
            }
            Item::Block(block) => write_block(&mut out, block, 0),
        }
    }
    out
}

fn write_block(out: &mut String, block: &Block, depth: usize) {
    let indent = "\t".repeat(depth);
    let continuation = format!("{indent}{CONTINUATION}");

    let mut lines = block.text.split('\n');
    let first = lines.next().unwrap_or_default();
    out.push_str(&indent);
    out.push_str(BULLET);
    if !first.is_empty() {
        out.push(' ');
        out.push_str(first);
    }
    out.push('\n');

    for property in &block.properties {
        write_property(out, &continuation, property);
    }
    for line in lines {
        out.push_str(&continuation);
        out.push_str(line);
        out.push('\n');
    }
    for child in &block.children {
        write_block(out, child, depth + 1);
    }
}

fn write_property(out: &mut String, indent: &str, property: &Property) {
    out.push_str(indent);
    out.push_str(&property.raw_key);
    out.push_str("::");
    if !property.value.is_empty() {
        out.push(' ');
        out.push_str(&property.value);
    }
    out.push('\n');
}

#[cfg(test)]
mod tests {
    use crate::model::{Block, Item, Page, Property};
    use crate::parse::parse_page;

    use super::serialize_page;

    #[test]
    fn empty_page_is_empty_string() {
        assert_eq!(serialize_page(&Page::default()), "");
    }

    #[test]
    fn writes_items_in_order() {
        let mut page = Page::default();
        page.items.push(Item::PageProperties(vec![
            Property::new("title", "项目/日课"),
            Property::new("tags", "项目, 笔记"),
        ]));
        page.items.push(Item::Block(Block::new("第一条")));
        page.items.push(Item::Raw("段落".to_owned()));
        let mut block = Block::new("多行\n正文");
        block.properties.push(Property::new("collapsed", "true"));
        block.children.push(Block::new("子块"));
        page.items.push(Item::Block(block));

        assert_eq!(
            serialize_page(&page),
            "title:: 项目/日课\ntags:: 项目, 笔记\n- 第一条\n段落\n- 多行\n  collapsed:: true\n  正文\n\t- 子块\n"
        );
    }

    #[test]
    fn empty_block_has_no_trailing_space() {
        let mut page = Page::default();
        page.items.push(Item::Block(Block::new("")));
        assert_eq!(serialize_page(&page), "-\n");
    }

    #[test]
    fn empty_property_value_has_no_trailing_space() {
        let mut page = Page::default();
        let mut block = Block::new("x");
        block.properties.push(Property::new("collapsed", ""));
        page.items.push(Item::Block(block));
        assert_eq!(serialize_page(&page), "- x\n  collapsed::\n");
    }

    #[test]
    fn canonical_input_round_trips_byte_identically() {
        let src =
            "title:: t\n- a\n  id:: 3f2a1b4c-5d6e-4f70-8a9b-0c1d2e3f4a5b\n  \n  续行\n\t- b\n-\n";
        let parsed = parse_page(src);
        assert_eq!(serialize_page(&parsed.page), src);
        assert!(parsed.lossless);
        assert!(parsed.warnings.is_empty());
    }
}
