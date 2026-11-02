use rike_core::Page;
use rike_core::config::VaultConfig;
use rike_core::journal::JournalDate;
use rike_core::names::file_stem_to_title;

/// Page title and journal date for a page file.
///
/// Journal files map their date to the configured title format so that
/// `[[Oct 31st, 2026]]` resolves to `journals/2026_10_31.md`. Other pages use
/// `title::` when present, else the decoded file stem.
pub fn resolve_title(
    rel_path: &str,
    page: &Page,
    config: &VaultConfig,
) -> (String, Option<JournalDate>) {
    let file = rel_path.rsplit('/').next().unwrap_or(rel_path);
    let stem = file.strip_suffix(".md").unwrap_or(file);

    if rel_path.starts_with("journals/")
        && let Some(date) = config.journal.parse_file_stem(stem)
    {
        return (config.journal.format_title(date), Some(date));
    }

    let explicit = page
        .page_property("title")
        .map(|p| p.value.trim())
        .filter(|v| !v.is_empty());
    match explicit {
        Some(title) => (title.to_owned(), None),
        None => (file_stem_to_title(stem), None),
    }
}

#[cfg(test)]
mod tests {
    use rike_core::parse::parse_page;

    use super::*;

    fn title(rel: &str, src: &str) -> (String, Option<JournalDate>) {
        resolve_title(rel, &parse_page(src).page, &VaultConfig::default())
    }

    #[test]
    fn journal_uses_configured_title_format() {
        let (t, d) = title("journals/2026_10_31.md", "- x\n");
        assert_eq!(t, "Oct 31st, 2026");
        assert_eq!(d, JournalDate::new(2026, 10, 31));
    }

    #[test]
    fn namespace_comes_from_file_stem() {
        assert_eq!(title("pages/项目___日课.md", "- x\n").0, "项目/日课");
    }

    #[test]
    fn title_property_wins_over_file_name() {
        assert_eq!(
            title("pages/x.md", "title:: Real Name\n- a\n").0,
            "Real Name"
        );
    }

    #[test]
    fn unparsable_journal_name_falls_back_to_a_normal_page() {
        let (t, d) = title("journals/notes.md", "- x\n");
        assert_eq!(t, "notes");
        assert_eq!(d, None);
    }
}
