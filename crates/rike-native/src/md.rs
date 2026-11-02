//! Turns a block's source text into the Markdown that `TextView` draws.
//!
//! References become links with the private schemes `sx-page:` and
//! `sx-block:` so a press on one is reported through `on_link_press`.

use rike_core::BlockId;
use rike_core::inline::{InlineKind, tokenize};
use rike_core::task::{Marker, parse_marker};

pub const PAGE_SCHEME: &str = "sx-page:";
pub const BLOCK_SCHEME: &str = "sx-block:";

pub fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

pub fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(hex) = s.get(i + 1..i + 3)
            && let Ok(v) = u8::from_str_radix(hex, 16)
        {
            out.push(v);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// The block's task marker; the checkbox or label is drawn by the UI.
pub fn task_of(text: &str) -> Option<Marker> {
    parse_marker(text).map(|(m, _)| m)
}

/// `lookup` gives the text of a block referenced by `((uuid))`. A task
/// marker is left out; finished tasks are struck through.
pub fn render(text: &str, lookup: &dyn Fn(BlockId) -> Option<String>) -> String {
    let (body, done) = match parse_marker(text) {
        Some((m, rest)) => (rest, !m.is_open()),
        None => (text, false),
    };
    let mut out = String::with_capacity(text.len() + 16);
    let mut body_out = String::with_capacity(body.len() + 16);
    let mut last = 0;
    for token in tokenize(body) {
        if token.range.start < last {
            continue;
        }
        body_out.push_str(&body[last..token.range.start]);
        match token.kind {
            InlineKind::PageRef { name } => {
                body_out.push_str(&format!(
                    "[{}]({PAGE_SCHEME}{})",
                    escape(&name),
                    encode(&name)
                ));
            }
            InlineKind::Tag { name } => {
                body_out.push_str(&format!(
                    "[#{}]({PAGE_SCHEME}{})",
                    escape(&name),
                    encode(&name)
                ));
            }
            InlineKind::BlockRef { id } => match lookup(id) {
                Some(target) => {
                    let first = target.lines().next().unwrap_or("");
                    body_out.push_str(&format!(
                        "[{}]({BLOCK_SCHEME}{})",
                        escape(&first.replace('\n', " ")),
                        id.0
                    ));
                }
                None => body_out.push_str(&format!("`(({}))`", id.0)),
            },
        }
        last = token.range.end;
    }
    body_out.push_str(&body[last..]);
    let body_out = hard_breaks(&body_out);
    if done {
        out.push_str("~~");
        out.push_str(body_out.trim_end());
        out.push_str("~~");
    } else {
        out.push_str(&body_out);
    }
    out
}

/// Link text may not contain unbalanced brackets.
fn escape(s: &str) -> String {
    s.replace('[', "\\[").replace(']', "\\]")
}

/// Logseq keeps line breaks inside a block; Markdown would fold them.
fn hard_breaks(s: &str) -> String {
    if s.contains("```") || !s.contains('\n') {
        return s.to_owned();
    }
    s.replace('\n', "  \n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn none(_: BlockId) -> Option<String> {
        None
    }

    #[test]
    fn encode_roundtrips_cjk_and_spaces() {
        let s = "项目/日课 a#b";
        assert_eq!(decode(&encode(s)), s);
    }

    #[test]
    fn page_refs_and_tags_become_links() {
        let out = render("see [[Foo Bar]] and #todo", &none);
        assert!(out.contains("[Foo Bar](sx-page:Foo%20Bar)"), "{out}");
        assert!(out.contains("[#todo](sx-page:todo)"), "{out}");
    }

    #[test]
    fn markers_are_left_to_the_ui() {
        assert_eq!(render("TODO write", &none), "write");
        assert_eq!(render("DONE write", &none), "~~write~~");
        assert_eq!(render("CANCELED write", &none), "~~write~~");
        assert_eq!(task_of("LATER x"), Some(Marker::Later));
        assert_eq!(task_of("x"), None);
    }

    #[test]
    fn unknown_block_ref_is_shown_as_code() {
        let id = BlockId::new();
        let out = render(&format!("x (({}))", id.0), &none);
        assert!(out.contains('`'), "{out}");
    }

    #[test]
    fn known_block_ref_links_with_text() {
        let id = BlockId::new();
        let out = render(&format!("(({}))", id.0), &|_| Some("hello".into()));
        assert_eq!(out, format!("[hello](sx-block:{})", id.0));
    }

    #[test]
    fn line_breaks_are_kept() {
        assert_eq!(render("a\nb", &none), "a  \nb");
    }
}
