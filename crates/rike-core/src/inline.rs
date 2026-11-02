//! Inline tokenizer for references inside a block body.
//!
//! The tokenizer is a single left-to-right scan. It skips inline code spans,
//! fenced code blocks and math (`$..$` on a single line, `$$..$$`, `\(..\)` and
//! `\[..\]` over several lines but not across a blank line), and emits tokens
//! ordered by their start offset. Ranges are byte offsets into the input.

use std::ops::Range;

use uuid::Uuid;

use crate::model::BlockId;
use crate::parse::{fence_close, fence_open};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InlineKind {
    /// `[[name]]`. Nested `[[a [[b]]]]` yields the outer and the inner token.
    PageRef { name: String },
    /// `#tag` or `#[[multi word]]`.
    Tag { name: String },
    /// `((uuid))` with a 36-char UUID.
    BlockRef { id: BlockId },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct InlineToken {
    /// Byte range of the whole token in the input, including delimiters.
    pub range: Range<usize>,
    pub kind: InlineKind,
}

/// Finds references in `text`. Skips inline code (`` `..` ``), fenced code and
/// `$..$` math. Results are sorted by `range.start`.
pub fn tokenize(text: &str) -> Vec<InlineToken> {
    let mut tokens = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    let mut i = 0;
    while i < text.len() {
        let ch = text[i..]
            .chars()
            .next()
            .expect("index is on a char boundary");
        if i == 0 || text.as_bytes()[i - 1] == b'\n' {
            let line_end = text[i..].find('\n').map_or(text.len(), |p| i + p);
            let line = &text[i..line_end];
            let next = if line_end < text.len() {
                line_end + 1
            } else {
                line_end
            };
            match fence {
                Some((marker, count)) => {
                    if fence_close(line, marker, count) {
                        fence = None;
                    }
                    i = next;
                    continue;
                }
                None => {
                    if let Some(open) = fence_open(line) {
                        fence = Some(open);
                        i = next;
                        continue;
                    }
                }
            }
        }
        if fence.is_some() {
            i += ch.len_utf8();
            continue;
        }
        match ch {
            '`' => i = skip_code_span(text, i),
            '[' if text[i..].starts_with("[[") => match page_ref_end(text, i) {
                Some(end) => {
                    tokens.push(InlineToken {
                        range: i..end,
                        kind: InlineKind::PageRef {
                            name: text[i + 2..end - 2].to_owned(),
                        },
                    });
                    // Nested references inside the name are reported as well.
                    tokens.extend(tokenize(&text[i + 2..end - 2]).into_iter().map(|token| {
                        InlineToken {
                            range: token.range.start + i + 2..token.range.end + i + 2,
                            kind: token.kind,
                        }
                    }));
                    i = end;
                }
                None => i += 2,
            },
            '#' => match tag(text, i) {
                Some((end, kind)) => {
                    tokens.push(InlineToken {
                        range: i..end,
                        kind,
                    });
                    i = end;
                }
                None => i += 1,
            },
            '(' if text[i..].starts_with("((") => match block_ref(text, i) {
                Some((end, id)) => {
                    tokens.push(InlineToken {
                        range: i..end,
                        kind: InlineKind::BlockRef { id },
                    });
                    i = end;
                }
                None => i += 2,
            },
            '$' => i = skip_math(text, i),
            '\\' => i = skip_tex_delimited(text, i),
            _ => i += ch.len_utf8(),
        }
    }
    tokens.sort_by_key(|token| token.range.start);
    tokens
}

/// End offset of the code span opening at `start`, or `start + run` when it is
/// never closed. A span is closed by the next run of exactly the same length.
fn skip_code_span(text: &str, start: usize) -> usize {
    let open = run_len(text, start, '`');
    let mut i = start + open;
    while i < text.len() {
        let ch = text[i..]
            .chars()
            .next()
            .expect("index is on a char boundary");
        if ch == '`' {
            let run = run_len(text, i, '`');
            if run == open {
                return i + run;
            }
            i += run;
            continue;
        }
        i += ch.len_utf8();
    }
    start + open
}

/// Skips `$..$` when the closing delimiter is on the same line, and `$$..$$`
/// up to a closing `$$` that comes before a blank line.
fn skip_math(text: &str, start: usize) -> usize {
    let open = if text[start..].starts_with("$$") {
        2
    } else {
        1
    };
    let line_end = text[start..].find('\n').map_or(text.len(), |p| start + p);
    let body_start = start + open;
    if body_start <= line_end {
        let close = if open == 2 { "$$" } else { "$" };
        if let Some(found) = text[body_start..line_end].find(close) {
            return body_start + found + close.len();
        }
    }
    if open == 2
        && let Some(end) = closer_before_blank_line(text, body_start, "$$")
    {
        return end;
    }
    start + open
}

/// Skips `\(..\)` and `\[..\]`: TeX's own delimiters, which `TextView` draws as
/// formulas. Any other backslash is passed over alone.
fn skip_tex_delimited(text: &str, start: usize) -> usize {
    let close = match text.as_bytes().get(start + 1) {
        Some(b'(') => "\\)",
        Some(b'[') => "\\]",
        _ => return start + 1,
    };
    closer_before_blank_line(text, start + 2, close).unwrap_or(start + 2)
}

/// End offset of the first `closer` at or after `from`, if no blank line lies
/// between: a formula does not run on through a paragraph break.
fn closer_before_blank_line(text: &str, from: usize, closer: &str) -> Option<usize> {
    let found = from + text[from..].find(closer)?;
    let between = &text[from..found];
    let mut lines = between.split('\n');
    lines.next();
    let inner: Vec<&str> = lines.collect();
    let blank = inner
        .split_last()
        .is_some_and(|(_, middle)| middle.iter().any(|line| line.trim().is_empty()));
    (!blank).then_some(found + closer.len())
}

/// End offset of the `]]` matching the `[[` at `start`, honouring nesting.
fn page_ref_end(text: &str, start: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut i = start;
    while i < text.len() {
        if text[i..].starts_with("[[") {
            depth += 1;
            i += 2;
            continue;
        }
        if text[i..].starts_with("]]") {
            depth -= 1;
            i += 2;
            if depth == 0 {
                return Some(i);
            }
            continue;
        }
        let ch = text[i..]
            .chars()
            .next()
            .expect("index is on a char boundary");
        i += ch.len_utf8();
    }
    None
}

/// Parses `#[[multi word]]` or `#tag` at `start`.
fn tag(text: &str, start: usize) -> Option<(usize, InlineKind)> {
    if !tag_boundary(text, start) {
        return None;
    }
    let rest = &text[start + 1..];
    if rest.starts_with("[[") {
        if let Some(end) = page_ref_end(text, start + 1) {
            return Some((
                end,
                InlineKind::Tag {
                    name: text[start + 3..end - 2].to_owned(),
                },
            ));
        }
        return None;
    }
    let mut i = start + 1;
    while i < text.len() {
        let ch = text[i..]
            .chars()
            .next()
            .expect("index is on a char boundary");
        if is_tag_delimiter(ch) {
            break;
        }
        i += ch.len_utf8();
    }
    if i == start + 1 {
        return None;
    }
    Some((
        i,
        InlineKind::Tag {
            name: text[start + 1..i].to_owned(),
        },
    ))
}

/// A tag must start a word: previous character is absent, whitespace or an
/// opening bracket/quote. This keeps `a#b` and `https://x/#y` from tagging.
fn tag_boundary(text: &str, start: usize) -> bool {
    match text[..start].chars().next_back() {
        None => true,
        Some(prev) => prev.is_whitespace() || matches!(prev, '(' | '[' | '{' | '"' | '\'' | '<'),
    }
}

fn is_tag_delimiter(ch: char) -> bool {
    ch.is_whitespace()
        || matches!(
            ch,
            ',' | '.'
                | '!'
                | '?'
                | ';'
                | ':'
                | '\''
                | '"'
                | '('
                | ')'
                | '['
                | ']'
                | '{'
                | '}'
                | '#'
        )
}

/// Parses `((uuid))` at `start`; the body must be a 36-character UUID.
fn block_ref(text: &str, start: usize) -> Option<(usize, BlockId)> {
    let body = &text[start + 2..];
    let close = body.find("))")?;
    let value = &body[..close];
    if value.len() != 36 {
        return None;
    }
    let uuid = Uuid::parse_str(value).ok()?;
    Some((start + 2 + close + 2, BlockId(uuid)))
}

fn run_len(text: &str, start: usize, marker: char) -> usize {
    text[start..].chars().take_while(|c| *c == marker).count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(text: &str) -> Vec<String> {
        tokenize(text)
            .into_iter()
            .map(|t| match t.kind {
                InlineKind::PageRef { name } | InlineKind::Tag { name } => name,
                InlineKind::BlockRef { id } => id.0.to_string(),
            })
            .collect()
    }

    #[test]
    fn references_inside_multiline_dollar_math_are_not_references() {
        assert_eq!(names("[[a]]\n$$\nx_{[[b]]} #c\n$$\n[[d]]"), ["a", "d"]);
    }

    #[test]
    fn references_inside_tex_delimiters_are_not_references() {
        assert_eq!(names("\\[ x_{[[b]]}\n  #c \\] [[a]]"), ["a"]);
        assert_eq!(names("\\( [[b]] \\) [[a]]"), ["a"]);
    }

    #[test]
    fn an_unclosed_or_paragraph_spanning_formula_hides_nothing() {
        assert_eq!(names("$$ [[a]]"), ["a"]);
        assert_eq!(names("$$ x\n\n[[a]]\n\n$$"), ["a"]);
        assert_eq!(names("\\[ x\n\n[[a]] \\]"), ["a"]);
        assert_eq!(names("a \\ b [[c]]"), ["c"]);
    }
}
