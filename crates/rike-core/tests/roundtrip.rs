use std::fs;
use std::path::{Path, PathBuf};

use proptest::prelude::*;
use rike_core::inline::tokenize;
use rike_core::parse::parse_page;
use rike_core::serialize::serialize_page;
use rike_core::{Block, Item, Page, Property};

fn fixtures(vault: &str) -> Vec<(PathBuf, String)> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/vaults");
    let mut out = Vec::new();
    for dir in ["journals", "pages"] {
        let Ok(entries) = fs::read_dir(root.join(vault).join(dir)) else {
            continue;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            if path.extension().is_some_and(|e| e == "md") {
                let text = fs::read_to_string(&path).unwrap();
                out.push((path, text));
            }
        }
    }
    assert!(!out.is_empty(), "no fixtures found for {vault}");
    out
}

/// Block ids are random for blocks without `id::`, so compare structure only.
fn shape(page: &Page) -> Vec<String> {
    fn block(b: &Block, depth: usize, out: &mut Vec<String>) {
        let props: Vec<String> = b
            .properties
            .iter()
            .map(|p| format!("{}={}", p.raw_key, p.value))
            .collect();
        out.push(format!("{depth}|{:?}|{props:?}", b.text));
        for c in &b.children {
            block(c, depth + 1, out);
        }
    }
    let mut out = Vec::new();
    for item in &page.items {
        match item {
            Item::PageProperties(ps) => out.push(format!("props|{ps:?}")),
            Item::Raw(t) => out.push(format!("raw|{t:?}")),
            Item::Block(b) => block(b, 0, &mut out),
        }
    }
    out
}

#[test]
fn canonical_fixtures_round_trip_byte_identically() {
    for (path, src) in fixtures("basic") {
        let parsed = parse_page(&src);
        assert!(parsed.lossless, "{path:?} should be lossless");
        assert_eq!(serialize_page(&parsed.page), src, "{path:?}");
    }
}

#[test]
fn messy_fixtures_keep_the_tree_and_stay_lossless() {
    for (path, src) in fixtures("messy") {
        let parsed = parse_page(&src);
        assert!(parsed.lossless, "{path:?} should be lossless");
        let written = serialize_page(&parsed.page);
        let again = parse_page(&written);
        assert_eq!(shape(&parsed.page), shape(&again.page), "{path:?}");
        assert_eq!(
            serialize_page(&again.page),
            written,
            "{path:?} not idempotent"
        );
    }
}

#[test]
fn space_indented_file_is_normalized_to_tabs() {
    let src = "- a\n  - b\n    - c\n  - d\n";
    let written = serialize_page(&parse_page(src).page);
    assert_eq!(written, "- a\n\t- b\n\t\t- c\n\t- d\n");
}

#[test]
fn crlf_is_normalized_to_lf() {
    let written = serialize_page(&parse_page("- a\r\n\t- b\r\n").page);
    assert_eq!(written, "- a\n\t- b\n");
}

#[test]
fn code_fence_content_is_not_parsed() {
    let src = "- code\n  ```\n  - not a bullet\n  key:: not a property\n  ```\n";
    let parsed = parse_page(src);
    let block = parsed.page.blocks().next().unwrap();
    assert!(block.children.is_empty());
    assert!(block.properties.is_empty());
    assert_eq!(serialize_page(&parsed.page), src);
}

#[test]
fn duplicate_block_ids_warn_and_first_wins() {
    let id = "11111111-2222-4333-8444-555555555555";
    let src = format!("- a\n  id:: {id}\n- b\n  id:: {id}\n");
    let parsed = parse_page(&src);
    let ids: Vec<_> = parsed.page.blocks().map(|b| b.id).collect();
    assert_eq!(ids[0].0.to_string(), id);
    assert_ne!(ids[0], ids[1]);
    assert_eq!(parsed.warnings.len(), 1);
    assert_eq!(serialize_page(&parsed.page), src);
}

#[test]
fn unmodeled_content_is_kept_as_raw() {
    let parsed = parse_page("---\ntitle: x\n---\n\n- a\n");
    assert!(matches!(parsed.page.items[0], Item::Raw(_)));
    assert!(parsed.lossless);
}

fn arb_text() -> impl Strategy<Value = String> {
    "[a-z中文][a-z0-9中文😀#\\[\\]() ]{0,10}"
}

fn arb_block() -> impl Strategy<Value = Block> {
    let leaf = arb_text().prop_map(Block::new);
    leaf.prop_recursive(3, 12, 3, |inner| {
        (
            arb_text(),
            proptest::option::of("[a-z]{1,6}"),
            proptest::collection::vec(inner, 0..3),
        )
            .prop_map(|(text, prop, children)| {
                let mut b = Block::new(text);
                if let Some(key) = prop {
                    b.properties.push(Property::new(key, "v"));
                }
                b.children = children;
                b
            })
    })
}

proptest! {
    #[test]
    fn serialize_parse_is_stable(blocks in proptest::collection::vec(arb_block(), 0..5)) {
        let page = Page { items: blocks.into_iter().map(Item::Block).collect() };
        let first = serialize_page(&page);
        let parsed = parse_page(&first);
        prop_assert!(parsed.lossless);
        prop_assert_eq!(serialize_page(&parsed.page), first);
    }

    #[test]
    fn tokenizer_ranges_are_valid(text in "[#\\[\\]()a-f0-9\\- 中😀`$]{0,60}") {
        let tokens = tokenize(&text);
        let mut last = 0;
        for t in &tokens {
            prop_assert!(t.range.start <= t.range.end && t.range.end <= text.len());
            prop_assert!(text.is_char_boundary(t.range.start));
            prop_assert!(text.is_char_boundary(t.range.end));
            prop_assert!(t.range.start >= last);
            last = t.range.start;
        }
    }

    #[test]
    fn tokenizer_never_panics_on_arbitrary_text(text in any::<String>()) {
        let _ = tokenize(&text);
    }
}
