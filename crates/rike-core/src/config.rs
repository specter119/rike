//! Minimal extraction of a few keys from `logseq/config.edn`.
//! This is deliberately not an EDN parser.

use crate::journal::JournalFormat;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FileNameFormat {
    TripleLowbar,
    Legacy,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VaultConfig {
    pub journal: JournalFormat,
    pub file_name_format: FileNameFormat,
}

impl Default for VaultConfig {
    fn default() -> Self {
        Self {
            journal: JournalFormat::default(),
            file_name_format: FileNameFormat::TripleLowbar,
        }
    }
}

impl VaultConfig {
    pub fn warnings(&self) -> Vec<String> {
        let mut out = Vec::new();
        if self.file_name_format != FileNameFormat::TripleLowbar {
            out.push(
                "`:file/name-format` is not `:triple-lowbar`; rike only supports the `:triple-lowbar` format, so namespaced page file names may not map to page titles correctly"
                    .to_owned(),
            );
        }
        out
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Value {
    Str(String),
    Keyword(String),
}

/// Extracts `:journal/page-title-format`, `:journal/file-name-format` and
/// `:file/name-format`. Unknown or malformed input falls back to defaults.
pub fn parse_config_edn(edn: &str) -> VaultConfig {
    let text = strip_comments(edn);
    let mut config = VaultConfig::default();

    if let Some(Value::Str(s)) = find_value(&text, ":journal/page-title-format")
        && !s.is_empty()
    {
        config.journal.page_title = s;
    }
    if let Some(Value::Str(s)) = find_value(&text, ":journal/file-name-format")
        && !s.is_empty()
    {
        config.journal.file_name = s;
    }
    match find_value(&text, ":file/name-format") {
        Some(Value::Keyword(k)) if k == ":legacy" => {
            config.file_name_format = FileNameFormat::Legacy
        }
        _ => {}
    }
    config
}

/// Drops `;` comments while leaving string literals untouched.
fn strip_comments(edn: &str) -> String {
    let mut out = String::with_capacity(edn.len());
    let (mut in_str, mut escaped, mut in_comment) = (false, false, false);
    for c in edn.chars() {
        if in_comment {
            if c == '\n' {
                in_comment = false;
                out.push(c);
            }
            continue;
        }
        if in_str {
            out.push(c);
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => {
                in_str = true;
                out.push(c);
            }
            ';' => in_comment = true,
            _ => out.push(c),
        }
    }
    out
}

fn is_delimiter(c: char) -> bool {
    c.is_whitespace() || matches!(c, ',' | '{' | '}' | '[' | ']' | '(' | ')')
}

fn find_value(text: &str, key: &str) -> Option<Value> {
    for (start, _) in text.match_indices(key) {
        let before_ok = text[..start].chars().next_back().is_none_or(is_delimiter);
        let rest = &text[start + key.len()..];
        let after_ok = rest.chars().next().is_none_or(is_delimiter);
        if !before_ok || !after_ok {
            continue;
        }
        let rest = rest.trim_start_matches(|c: char| c.is_whitespace() || c == ',');
        return read_value(rest);
    }
    None
}

fn read_value(rest: &str) -> Option<Value> {
    if let Some(body) = rest.strip_prefix('"') {
        let mut out = String::new();
        let mut chars = body.chars();
        while let Some(c) = chars.next() {
            match c {
                '"' => return Some(Value::Str(out)),
                '\\' => match chars.next()? {
                    'n' => out.push('\n'),
                    't' => out.push('\t'),
                    other => out.push(other),
                },
                other => out.push(other),
            }
        }
        return None;
    }
    if rest.starts_with(':') {
        let end = rest.find(is_delimiter).unwrap_or(rest.len());
        return Some(Value::Keyword(rest[..end].to_owned()));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = r#"{:meta/version 1

 ;; Currently, we support either "Markdown" or "Org".
 :preferred-format "Markdown"

 ;; :journal/page-title-format "yyyy年MM月dd日"
 :journal/page-title-format "EEE, dd.MM.yyyy"

 :journal/file-name-format "yyyy-MM-dd"

 :file/name-format :triple-lowbar
 :ui/enable-tooltip? true}
"#;

    #[test]
    fn reads_the_three_keys_and_ignores_comments() {
        let cfg = parse_config_edn(SAMPLE);
        assert_eq!(cfg.journal.page_title, "EEE, dd.MM.yyyy");
        assert_eq!(cfg.journal.file_name, "yyyy-MM-dd");
        assert_eq!(cfg.file_name_format, FileNameFormat::TripleLowbar);
        assert!(cfg.warnings().is_empty());
    }

    #[test]
    fn missing_or_malformed_input_gives_defaults() {
        assert_eq!(parse_config_edn(""), VaultConfig::default());
        assert_eq!(
            parse_config_edn("{:journal/page-title-format"),
            VaultConfig::default()
        );
        assert_eq!(
            parse_config_edn("{:journal/page-title-format 42}"),
            VaultConfig::default()
        );
    }

    #[test]
    fn legacy_format_warns() {
        let cfg = parse_config_edn("{:file/name-format :legacy}");
        assert_eq!(cfg.file_name_format, FileNameFormat::Legacy);
        assert_eq!(cfg.warnings().len(), 1);
    }

    #[test]
    fn longer_key_with_same_prefix_is_not_matched() {
        let cfg = parse_config_edn(r#"{:journal/page-title-format-extra "x"}"#);
        assert_eq!(cfg.journal, JournalFormat::default());
    }

    #[test]
    fn handles_escapes_and_semicolons_inside_strings() {
        let cfg = parse_config_edn(r#"{:journal/page-title-format "a;\"b\" yyyy"}"#);
        assert_eq!(cfg.journal.page_title, "a;\"b\" yyyy");
    }
}
