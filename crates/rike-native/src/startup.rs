//! Startup decision and vault-folder checks (pure functions plus thin filesystem wrappers).

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::state::{self, Entry};

/// How a directory looks before opening it as a vault.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VaultKind {
    /// Contains at least one of the vault markers.
    Vault,
    /// Empty, or holds only hidden entries.
    Empty,
    /// Anything else; the host asks the user to confirm before opening it.
    Other,
}

/// Classifies directory entries as `(file name, is directory)`.
pub fn classify(names: &[(String, bool)]) -> VaultKind {
    const MARKERS: [&str; 4] = ["journals", "pages", "logseq", ".rike"];
    if names
        .iter()
        .any(|(name, is_dir)| *is_dir && MARKERS.contains(&name.as_str()))
    {
        return VaultKind::Vault;
    }
    if names.iter().all(|(name, _)| name.starts_with('.')) {
        return VaultKind::Empty;
    }
    VaultKind::Other
}

/// Lists a directory as `(file name, is directory)`, skipping non-UTF-8 names.
pub fn listing(path: &Path) -> io::Result<Vec<(String, bool)>> {
    let mut names = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let Ok(name) = entry.file_name().into_string() else {
            continue;
        };
        // Follow symlinks: a `journals` link still marks a vault.
        let is_dir = fs::metadata(entry.path()).is_ok_and(|meta| meta.is_dir());
        names.push((name, is_dir));
    }
    Ok(names)
}

/// Classifies a directory. An unreadable directory counts as [`VaultKind::Other`].
pub fn looks_like_vault(path: &Path) -> VaultKind {
    listing(path).map_or(VaultKind::Other, |names| classify(&names))
}

/// Why `path` must not be used as a vault, if it must not. `path` is expected to be
/// canonical; `home` is a parameter so the check is testable.
pub fn is_refused(path: &Path, home: Option<&Path>) -> Option<String> {
    if path.has_root() && path.parent().is_none() {
        return Some("不能用文件系统根目录作为笔记库".to_owned());
    }
    let home = home?;
    if path == home {
        return Some("不能用主目录作为笔记库".to_owned());
    }
    if home.starts_with(path) {
        return Some(format!(
            "不能用主目录的上级目录 {} 作为笔记库",
            path.display()
        ));
    }
    None
}

/// What the host should show after start-up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    Open {
        path: PathBuf,
        create: bool,
        view: Option<String>,
    },
    Welcome {
        notice: Option<String>,
    },
}

/// A command-line directory wins; otherwise only the first recent vault is considered.
pub fn decide(
    cli_vault: Option<&Path>,
    recent: &[Entry],
    is_dir: impl Fn(&Path) -> bool,
    home: Option<&Path>,
) -> Decision {
    decide_with(cli_vault.map(|path| (path, false)), recent, is_dir, home)
}

/// Like [`decide`], but `--vault` can create a missing directory.
pub fn decide_with(
    cli_vault: Option<(&Path, bool)>,
    recent: &[Entry],
    is_dir: impl Fn(&Path) -> bool,
    home: Option<&Path>,
) -> Decision {
    if let Some((path, create)) = cli_vault {
        if let Some(reason) = is_refused(path, home) {
            return Decision::Welcome {
                notice: Some(reason),
            };
        }
        if is_dir(path) {
            return Decision::Open {
                path: path.to_path_buf(),
                create: false,
                view: None,
            };
        }
        if create {
            return Decision::Open {
                path: path.to_path_buf(),
                create: true,
                view: None,
            };
        }
        return Decision::Welcome {
            notice: Some(format!("找不到库目录 {}", path.display())),
        };
    }

    let Some(first) = recent.first() else {
        return Decision::Welcome { notice: None };
    };
    if let Some(reason) = is_refused(&first.path, home) {
        return Decision::Welcome {
            notice: Some(reason),
        };
    }
    if is_dir(&first.path) {
        Decision::Open {
            path: first.path.clone(),
            create: false,
            view: first.view.clone(),
        }
    } else {
        Decision::Welcome {
            notice: Some(format!("上次的库 {} 找不到了", first.path.display())),
        }
    }
}

/// Drops a saved view when its page file no longer exists. `exists` receives the
/// vault-relative path.
pub fn view_is_valid(view: &str, exists: impl Fn(&str) -> bool) -> Option<String> {
    if view == "journals" {
        return Some(view.to_owned());
    }
    let rel = view.strip_prefix("page:")?;
    (state::rel_is_page_path(rel) && exists(rel)).then(|| view.to_owned())
}

/// The view to restore from a saved `journals` / `page:<rel>` text: the page only when
/// its file still exists in `root`.
pub fn view_from_saved(saved: Option<&str>, root: &Path) -> crate::session::View {
    use crate::session::View;
    let valid = saved.and_then(|v| view_is_valid(v, |rel| root.join(rel).is_file()));
    match valid.as_deref().and_then(|v| v.strip_prefix("page:")) {
        Some(rel) => View::Page(rel.to_owned()),
        None => View::Journals,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, view: Option<&str>) -> Entry {
        Entry {
            path: PathBuf::from(path),
            view: view.map(str::to_owned),
        }
    }

    fn names(entries: &[(&str, bool)]) -> Vec<(String, bool)> {
        entries
            .iter()
            .map(|(name, is_dir)| ((*name).to_owned(), *is_dir))
            .collect()
    }

    fn is_vault(path: &Path) -> bool {
        path == Path::new("/vault")
    }

    #[test]
    fn classify_finds_vault_markers() {
        for marker in ["journals", "pages", "logseq", ".rike"] {
            assert_eq!(
                classify(&names(&[(marker, true)])),
                VaultKind::Vault,
                "{marker} should mark a vault"
            );
        }
        assert_eq!(
            classify(&names(&[(".git", true), ("journals", true)])),
            VaultKind::Vault
        );
    }

    #[test]
    fn classify_requires_markers_to_be_directories() {
        assert_eq!(classify(&names(&[("journals", false)])), VaultKind::Other);
        assert_eq!(
            classify(&names(&[("notes.txt", false), ("pages.md", false)])),
            VaultKind::Other
        );
    }

    #[test]
    fn classify_empty_and_hidden_only_directories() {
        assert_eq!(classify(&[]), VaultKind::Empty);
        assert_eq!(
            classify(&names(&[(".git", true), (".hidden", false)])),
            VaultKind::Empty
        );
    }

    #[test]
    fn looks_like_vault_reads_the_directory() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(looks_like_vault(dir.path()), VaultKind::Empty);

        fs::write(dir.path().join("notes.txt"), "hi").unwrap();
        assert_eq!(looks_like_vault(dir.path()), VaultKind::Other);

        fs::create_dir(dir.path().join(".rike")).unwrap();
        assert_eq!(looks_like_vault(dir.path()), VaultKind::Vault);

        assert_eq!(
            looks_like_vault(&dir.path().join("missing")),
            VaultKind::Other
        );
    }

    #[test]
    fn looks_like_vault_accepts_an_empty_file_marker_as_other() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("journals"), "").unwrap();
        assert_eq!(looks_like_vault(dir.path()), VaultKind::Other);
    }

    #[test]
    fn refuses_root_home_and_ancestors_of_home() {
        let home = Path::new("/home/alice");
        assert!(is_refused(Path::new("/"), Some(home)).is_some());
        assert!(is_refused(Path::new("/home/alice"), Some(home)).is_some());
        assert!(is_refused(Path::new("/home"), Some(home)).is_some());
        assert!(is_refused(Path::new("/"), None).is_some());
        assert_eq!(is_refused(Path::new("/home/alice/vault"), Some(home)), None);
        assert_eq!(is_refused(Path::new("/tmp/vault"), Some(home)), None);
        assert_eq!(is_refused(Path::new("/home/alice"), None), None);
    }

    #[test]
    fn refusal_reasons_are_chinese() {
        let notice = is_refused(Path::new("/"), None).unwrap();
        assert!(notice.contains("根目录"), "{notice}");
        let notice = is_refused(Path::new("/home/alice"), Some(Path::new("/home/alice"))).unwrap();
        assert!(notice.contains("主目录"), "{notice}");
    }

    #[test]
    fn decide_opens_the_cli_directory() {
        assert_eq!(
            decide(Some(Path::new("/vault")), &[], is_vault, None),
            Decision::Open {
                path: PathBuf::from("/vault"),
                create: false,
                view: None
            }
        );
    }

    #[test]
    fn decide_refuses_the_cli_directory() {
        let home = Path::new("/home/alice");
        let decision = decide(Some(home), &[], is_vault, Some(home));
        match decision {
            Decision::Welcome {
                notice: Some(notice),
            } => assert!(notice.contains("主目录")),
            other => panic!("expected a welcome decision, got {other:?}"),
        }
    }

    #[test]
    fn decide_welcomes_when_the_cli_directory_is_missing() {
        match decide(Some(Path::new("/missing")), &[], is_vault, None) {
            Decision::Welcome {
                notice: Some(notice),
            } => {
                assert!(notice.contains("/missing"), "{notice}");
            }
            other => panic!("expected a welcome decision, got {other:?}"),
        }
    }

    #[test]
    fn decide_with_creates_a_missing_cli_directory() {
        assert_eq!(
            decide_with(Some((Path::new("/missing"), true)), &[], is_vault, None),
            Decision::Open {
                path: PathBuf::from("/missing"),
                create: true,
                view: None
            }
        );
        // An existing directory is never re-created.
        assert_eq!(
            decide_with(Some((Path::new("/vault"), true)), &[], is_vault, None),
            Decision::Open {
                path: PathBuf::from("/vault"),
                create: false,
                view: None
            }
        );
    }

    #[test]
    fn decide_opens_the_first_recent_vault_with_its_view() {
        let recent = [
            entry("/vault", Some("page:pages/Inbox.md")),
            entry("/other", Some("journals")),
        ];
        assert_eq!(
            decide(None, &recent, is_vault, None),
            Decision::Open {
                path: PathBuf::from("/vault"),
                create: false,
                view: Some("page:pages/Inbox.md".to_owned())
            }
        );
    }

    #[test]
    fn decide_reports_a_missing_recent_vault() {
        let recent = [entry("/gone", None), entry("/vault", None)];
        match decide(None, &recent, is_vault, None) {
            Decision::Welcome {
                notice: Some(notice),
            } => {
                assert!(notice.starts_with("上次的库"), "{notice}");
                assert!(notice.contains("/gone"), "{notice}");
            }
            other => panic!("expected a welcome decision, got {other:?}"),
        }
    }

    #[test]
    fn decide_welcomes_without_recents() {
        assert_eq!(
            decide(None, &[], is_vault, None),
            Decision::Welcome { notice: None }
        );
    }

    #[test]
    fn decide_refuses_a_recent_home() {
        let home = Path::new("/home/alice");
        match decide(None, &[entry("/home/alice", None)], is_vault, Some(home)) {
            Decision::Welcome {
                notice: Some(notice),
            } => assert!(notice.contains("主目录")),
            other => panic!("expected a welcome decision, got {other:?}"),
        }
    }

    #[test]
    fn views_survive_only_when_their_page_exists() {
        assert_eq!(
            view_is_valid("journals", |_| false),
            Some("journals".to_owned())
        );
        assert_eq!(
            view_is_valid("page:pages/Inbox.md", |rel| rel == "pages/Inbox.md"),
            Some("page:pages/Inbox.md".to_owned())
        );
        assert_eq!(view_is_valid("page:pages/Inbox.md", |_| false), None);
        assert_eq!(view_is_valid("page:../escape.md", |_| true), None);
        assert_eq!(view_is_valid("bogus", |_| true), None);
    }

    #[test]
    fn saved_view_needs_an_existing_page() {
        use crate::session::View;
        let dir = std::env::temp_dir().join(format!("rike-view-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("pages")).unwrap();
        std::fs::write(dir.join("pages/A.md"), "- a\n").unwrap();
        assert_eq!(
            view_from_saved(Some("page:pages/A.md"), &dir),
            View::Page("pages/A.md".into())
        );
        assert_eq!(
            view_from_saved(Some("page:pages/B.md"), &dir),
            View::Journals
        );
        assert_eq!(view_from_saved(Some("journals"), &dir), View::Journals);
        assert_eq!(view_from_saved(None, &dir), View::Journals);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
