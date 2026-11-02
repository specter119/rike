//! Finds the formulas of a Markdown source the way the TextView would.
//! Rendering lives in `rike_native::math`; this crate is its command-line harness.

/// Formulas of a Markdown source, as the TextView would see them:
/// `(tex, display)`.
pub fn formulas(source: &str) -> Vec<(String, bool)> {
    let mut out = Vec::new();
    let mut rest = source;
    while let Some(i) = rest.find(['$', '\\']) {
        let tail = &rest[i..];
        let (open, close, display) = if tail.starts_with("$$") {
            ("$$", "$$", true)
        } else if tail.starts_with("\\[") {
            ("\\[", "\\]", true)
        } else if tail.starts_with("\\(") {
            ("\\(", "\\)", false)
        } else if tail.starts_with('$') {
            ("$", "$", false)
        } else {
            rest = &rest[i + 1..];
            // Skip the escaped character, so `\$` is not a delimiter.
            if let Some(c) = rest.chars().next() {
                rest = &rest[c.len_utf8()..];
            }
            continue;
        };
        let body = &tail[open.len()..];
        match body.find(close) {
            Some(end)
                if !body[..end].trim().is_empty()
                    && (open != "$" || !body[..end].contains('\n')) =>
            {
                out.push((body[..end].trim().to_string(), display));
                rest = &body[end + close.len()..];
            }
            _ => rest = &tail[open.len()..],
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_every_delimiter_form() {
        let found = formulas("a $x^2$ b \\(y\\) c\n\n$$\nz\n$$\n\n\\[ w \\]\n costs \\$5");
        assert_eq!(
            found,
            vec![
                ("x^2".into(), false),
                ("y".into(), false),
                ("z".into(), true),
                ("w".into(), true),
            ]
        );
    }
}
