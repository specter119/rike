//! Recent-vaults state file: pure parsing and ordering, plus a small locked IO layer.
//!
//! The file lives in `$XDG_STATE_HOME/rike/vaults` and holds one `<path>[\t<view>]` per
//! line, most recent first, at most [`MAX_ENTRIES`] lines.

use std::fs;
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

/// Upper bound on the number of remembered vaults.
pub const MAX_ENTRIES: usize = 20;

/// One remembered vault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    /// `journals` or `page:<rel>`; `None` when the last view is unknown or invalid.
    pub view: Option<String>,
}

/// `$XDG_STATE_HOME/rike/vaults`, or `~/.local/state/rike/vaults` when the variable
/// is unset or relative (the XDG spec says relative paths must be ignored).
pub fn state_file_path(xdg_state_home: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    if let Some(dir) = xdg_state_home.filter(|dir| dir.is_absolute()) {
        return Some(dir.join("rike").join("vaults"));
    }
    home.map(|home| {
        home.join(".local")
            .join("state")
            .join("rike")
            .join("vaults")
    })
}

/// Whether `view` is structurally valid. Existence of the page is checked by the caller.
pub fn view_is_well_formed(view: &str) -> bool {
    if view == "journals" {
        return true;
    }
    view.strip_prefix("page:").is_some_and(rel_is_page_path)
}

/// A vault-relative page path: under `journals/` or `pages/`, ending in `.md`, without
/// parent or current segments and without characters that would break the line format.
pub fn rel_is_page_path(rel: &str) -> bool {
    if !(rel.starts_with("journals/") || rel.starts_with("pages/")) || !rel.ends_with(".md") {
        return false;
    }
    !rel.contains("..")
        && !rel.chars().any(|c| matches!(c, '\t' | '\n' | '\r' | '\0'))
        && rel
            .split('/')
            .all(|segment| !segment.is_empty() && segment != ".")
}

/// Parses the file. Bad lines and duplicate paths are skipped; an invalid view is dropped
/// but its line is kept.
pub fn parse(text: &str) -> Vec<Entry> {
    let mut entries: Vec<Entry> = Vec::new();
    for line in text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        let (path, view) = match line.split_once('\t') {
            Some((path, view)) => (path, Some(view)),
            None => (line, None),
        };
        let path = Path::new(path);
        if !path.is_absolute() || !is_storable_path(path) {
            continue;
        }
        if entries.iter().any(|entry| entry.path == path) {
            continue;
        }
        entries.push(Entry {
            path: path.to_path_buf(),
            view: view
                .filter(|view| view_is_well_formed(view))
                .map(str::to_owned),
        });
        if entries.len() == MAX_ENTRIES {
            break;
        }
    }
    entries
}

/// Renders the file. Paths that cannot be written (non-UTF-8, tabs, newlines) and invalid
/// views are skipped, as are duplicates and entries past [`MAX_ENTRIES`].
pub fn render(entries: &[Entry]) -> String {
    let mut out = String::new();
    let mut written: Vec<&Path> = Vec::new();
    for entry in entries {
        if written.len() == MAX_ENTRIES {
            break;
        }
        let Some(path) = entry
            .path
            .to_str()
            .filter(|_| is_storable_path(&entry.path))
        else {
            continue;
        };
        if written.contains(&entry.path.as_path()) {
            continue;
        }
        out.push_str(path);
        if let Some(view) = entry
            .view
            .as_deref()
            .filter(|view| view_is_well_formed(view))
        {
            out.push('\t');
            out.push_str(view);
        }
        out.push('\n');
        written.push(&entry.path);
    }
    out
}

/// Moves an existing entry to the front (keeping its view) or inserts a new one without a
/// view, then truncates to [`MAX_ENTRIES`].
pub fn push_front(entries: &mut Vec<Entry>, path: &Path) {
    if !is_storable_path(path) {
        return;
    }
    if let Some(index) = entries.iter().position(|entry| entry.path == path) {
        let entry = entries.remove(index);
        entries.insert(0, entry);
    } else {
        entries.insert(
            0,
            Entry {
                path: path.to_path_buf(),
                view: None,
            },
        );
    }
    entries.truncate(MAX_ENTRIES);
}

/// Changes the view of an existing entry in place: no reordering, no insertion.
pub fn set_view(entries: &mut [Entry], path: &Path, view: Option<&str>) {
    if let Some(entry) = entries.iter_mut().find(|entry| entry.path == path) {
        entry.view = view
            .filter(|view| view_is_well_formed(view))
            .map(str::to_owned);
    }
}

/// Forgets a vault.
pub fn remove(entries: &mut Vec<Entry>, path: &Path) {
    entries.retain(|entry| entry.path != path);
}

fn is_storable_path(path: &Path) -> bool {
    path.to_str().is_some_and(|text| {
        !text.is_empty() && !text.chars().any(|c| matches!(c, '\t' | '\n' | '\r' | '\0'))
    })
}

static NEXT_SEQ: AtomicU64 = AtomicU64::new(1);

/// Reserves the sequence number of a write request. Take it on the thread that receives
/// the request (not on the blocking thread) so queued writes keep request order.
pub fn next_seq() -> u64 {
    NEXT_SEQ.fetch_add(1, Ordering::SeqCst)
}

/// Rewrites `file` with `f` applied to the entries read from disk.
///
/// One-shot: it always writes. For queued writes that may overtake each other, share a
/// [`StateWriter`] instead so a stale request can be skipped.
pub fn update(file: &Path, f: impl FnOnce(&mut Vec<Entry>)) -> io::Result<bool> {
    StateWriter::new(file).apply(next_seq(), f)
}

#[derive(Debug)]
struct Inner {
    file: PathBuf,
    /// Highest request sequence number this writer has written.
    last_written: AtomicU64,
}

/// Serialises writes to one state file. Clones share the sequence-number guard; use clones
/// of the same writer for every write to the same file.
#[derive(Debug, Clone)]
pub struct StateWriter {
    inner: Arc<Inner>,
}

impl StateWriter {
    pub fn new(file: impl Into<PathBuf>) -> Self {
        Self {
            inner: Arc::new(Inner {
                file: file.into(),
                last_written: AtomicU64::new(0),
            }),
        }
    }

    pub fn file(&self) -> &Path {
        &self.inner.file
    }

    /// Reserves a sequence number for a request whose [`StateWriter::apply`] runs later.
    pub fn reserve(&self) -> u64 {
        next_seq()
    }

    /// Applies a reserved request. Returns `false` when a newer request was already written.
    pub fn apply(&self, seq: u64, f: impl FnOnce(&mut Vec<Entry>)) -> io::Result<bool> {
        write(&self.inner.file, seq, &self.inner.last_written, f)
    }

    /// Reserves a sequence number and applies the request at once; returns that number.
    pub fn submit(&self, f: impl FnOnce(&mut Vec<Entry>)) -> u64 {
        let seq = self.reserve();
        if let Err(err) = self.apply(seq, f) {
            log::warn!("cannot write {}: {err}", self.inner.file.display());
        }
        seq
    }
}

fn write(
    file: &Path,
    seq: u64,
    last_written: &AtomicU64,
    f: impl FnOnce(&mut Vec<Entry>),
) -> io::Result<bool> {
    if let Some(parent) = file
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)?;
    }
    let lock_path = lock_path(file);
    let lock = fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(&lock_path)?;
    match lock.lock() {
        Ok(()) => {}
        // Without file locking a lost update is still better than a failed write.
        Err(err) if err.kind() == ErrorKind::Unsupported => {
            log::debug!(
                "file locking is unsupported for {}: {err}",
                lock_path.display()
            );
        }
        Err(err) => return Err(err),
    }

    // Checked under the lock: a newer request that finished first wins, an older one is
    // skipped without touching the file.
    if seq <= last_written.load(Ordering::SeqCst) {
        return Ok(false);
    }
    let text = fs::read_to_string(file).unwrap_or_default();
    let mut entries = parse(&text);
    f(&mut entries);
    write_atomic(file, render(&entries).as_bytes(), seq)?;
    last_written.store(seq, Ordering::SeqCst);
    Ok(true)
}

fn write_atomic(file: &Path, bytes: &[u8], seq: u64) -> io::Result<()> {
    let name = file.file_name().map_or_else(
        || "vaults".into(),
        |name| name.to_string_lossy().into_owned(),
    );
    let tmp = file.with_file_name(format!("{name}.tmp.{}.{seq}", std::process::id()));
    let result = fs::File::create(&tmp).and_then(|mut tmp_file| {
        tmp_file.write_all(bytes)?;
        fs::rename(&tmp, file)
    });
    if result.is_err() {
        let _ = fs::remove_file(&tmp);
    }
    result
}

fn lock_path(file: &Path) -> PathBuf {
    match file.file_name() {
        Some(name) => {
            let mut name = name.to_os_string();
            name.push(".lock");
            file.with_file_name(name)
        }
        None => file.with_extension("lock"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;

    fn entry(path: &str, view: Option<&str>) -> Entry {
        Entry {
            path: PathBuf::from(path),
            view: view.map(str::to_owned),
        }
    }

    #[test]
    fn state_file_path_prefers_absolute_xdg() {
        assert_eq!(
            state_file_path(Some(Path::new("/xdg")), Some(Path::new("/home/a"))),
            Some(PathBuf::from("/xdg/rike/vaults"))
        );
        assert_eq!(
            state_file_path(None, Some(Path::new("/home/a"))),
            Some(PathBuf::from("/home/a/.local/state/rike/vaults"))
        );
        assert_eq!(
            state_file_path(Some(Path::new("relative")), Some(Path::new("/home/a"))),
            Some(PathBuf::from("/home/a/.local/state/rike/vaults"))
        );
        assert_eq!(state_file_path(None, None), None);
    }

    #[test]
    fn roundtrip_is_byte_identical() {
        let text =
            "/home/a/My Vault\tjournals\n/home/b/vault\tpage:pages/Inbox.md\n/home/c/vault\n";
        let entries = parse(text);
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].view.as_deref(), Some("journals"));
        assert_eq!(entries[1].view.as_deref(), Some("page:pages/Inbox.md"));
        assert_eq!(entries[2].view, None);
        assert_eq!(render(&entries), text);
        assert_eq!(parse(&render(&entries)), entries);
    }

    #[test]
    fn bad_lines_are_skipped() {
        let text = "\n\
            relative/vault\n\
            /good/vault\tjournals\n\
            /good/vault\tpage:pages/A.md\n\
            /bad\tview.txt\n\
            /also-bad\tpage:../escape.md\n";
        let entries = parse(text);
        assert_eq!(entries.len(), 3);
        // The first duplicate wins, including its view.
        assert_eq!(entries[0], entry("/good/vault", Some("journals")));
        // An invalid view is dropped but the path survives.
        assert_eq!(entries[1], entry("/bad", None));
        assert_eq!(entries[2], entry("/also-bad", None));
    }

    #[test]
    fn unknown_view_prefixes_are_dropped() {
        assert_eq!(parse("/v\tbogus\n"), vec![entry("/v", None)]);
        assert_eq!(parse("/v\tpage:md/A.md\n"), vec![entry("/v", None)]);
        assert_eq!(parse("/v\tpage:pages/A.txt\n"), vec![entry("/v", None)]);
    }

    #[test]
    fn crlf_lines_are_accepted() {
        assert_eq!(
            parse("/v\tjournals\r\n"),
            vec![entry("/v", Some("journals"))]
        );
    }

    #[test]
    fn parse_caps_the_list() {
        let text: String = (0..MAX_ENTRIES + 5)
            .map(|i| format!("/vault{i}\n"))
            .collect();
        assert_eq!(parse(&text).len(), MAX_ENTRIES);
    }

    #[test]
    fn views_must_point_at_page_files() {
        assert!(view_is_well_formed("journals"));
        assert!(view_is_well_formed("page:pages/Inbox.md"));
        assert!(view_is_well_formed("page:journals/2026_11_01.md"));
        assert!(!view_is_well_formed("Page:pages/Inbox.md"));
        assert!(!view_is_well_formed("pages/Inbox.md"));
        assert!(!view_is_well_formed("page:/pages/Inbox.md"));
        assert!(!view_is_well_formed("page:pages//Inbox.md"));
        assert!(!view_is_well_formed("page:pages/./Inbox.md"));
        assert!(!view_is_well_formed("page:pages/../Inbox.md"));
        assert!(!view_is_well_formed("page:pages/Inbox.md/"));
        assert!(!view_is_well_formed("page:pages/Inbox.md\n"));
        assert!(!view_is_well_formed("page:pages/In\tbox.md"));
        assert!(!view_is_well_formed("page:pages/Inbox.txt"));
        assert!(!view_is_well_formed("page:pages/..hidden.md"));
        assert!(!view_is_well_formed(""));
        assert!(!view_is_well_formed("page:pages/In..box.md"));
    }

    #[test]
    fn render_rejects_unwritable_paths() {
        assert_eq!(render(&[entry("/v\tx", None)]), "");
        assert_eq!(render(&[entry("", None)]), "");
        #[cfg(unix)]
        {
            use std::ffi::OsStr;
            use std::os::unix::ffi::OsStrExt;
            let path = PathBuf::from(OsStr::from_bytes(b"/v/\xff"));
            assert_eq!(render(&[Entry { path, view: None }]), "");
        }
    }

    #[test]
    fn render_drops_duplicates_and_invalid_views() {
        let entries = vec![
            entry("/a", Some("journals")),
            entry("/a", Some("page:pages/A.md")),
            entry("/b", Some("bogus")),
        ];
        assert_eq!(render(&entries), "/a\tjournals\n/b\n");
    }

    #[test]
    fn push_front_keeps_the_view_and_caps_the_list() {
        let mut entries = vec![entry("/a", Some("journals")), entry("/b", None)];
        push_front(&mut entries, Path::new("/b"));
        assert_eq!(
            entries,
            vec![entry("/b", None), entry("/a", Some("journals"))]
        );
        push_front(&mut entries, Path::new("/c"));
        assert_eq!(entries[0], entry("/c", None));
        assert_eq!(entries.len(), 3);

        let mut many: Vec<Entry> = (0..MAX_ENTRIES)
            .map(|i| entry(&format!("/vault{i}"), None))
            .collect();
        push_front(&mut many, Path::new("/new"));
        assert_eq!(many.len(), MAX_ENTRIES);
        assert_eq!(many[0].path, PathBuf::from("/new"));
        assert!(!many.iter().any(|e| e.path == Path::new("/vault19")));
    }

    #[test]
    fn push_front_ignores_unwritable_paths() {
        let mut entries = Vec::new();
        push_front(&mut entries, Path::new("/v\tx"));
        push_front(&mut entries, Path::new(""));
        assert!(entries.is_empty());
    }

    #[test]
    fn set_view_is_in_place_only() {
        let mut entries = vec![
            entry("/a", None),
            entry("/b", Some("journals")),
            entry("/c", None),
        ];
        set_view(&mut entries, Path::new("/c"), Some("page:pages/C.md"));
        assert_eq!(
            entries,
            vec![
                entry("/a", None),
                entry("/b", Some("journals")),
                entry("/c", Some("page:pages/C.md")),
            ]
        );
        set_view(&mut entries, Path::new("/a"), Some("page:../x.md"));
        assert_eq!(entries[0].view, None);
        set_view(&mut entries, Path::new("/a"), None);
        set_view(&mut entries, Path::new("/missing"), Some("journals"));
        assert_eq!(entries.len(), 3);
    }

    #[test]
    fn remove_forgets_one_vault() {
        let mut entries = vec![entry("/a", None), entry("/b", Some("journals"))];
        remove(&mut entries, Path::new("/a"));
        assert_eq!(entries, vec![entry("/b", Some("journals"))]);
        remove(&mut entries, Path::new("/missing"));
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn update_writes_through_a_lock_and_temp_file() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("state/rike/vaults");
        assert!(update(&file, |entries| push_front(entries, Path::new("/a"))).unwrap());
        assert!(update(&file, |entries| push_front(entries, Path::new("/b"))).unwrap());
        let entries = parse(&fs::read_to_string(&file).unwrap());
        assert_eq!(entries, vec![entry("/b", None), entry("/a", None)]);
        assert!(file.with_file_name("vaults.lock").exists());
        let leftovers: Vec<_> = fs::read_dir(file.parent().unwrap())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp."))
            .collect();
        assert!(leftovers.is_empty());
    }

    #[test]
    fn unreadable_file_is_treated_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("vaults");
        fs::write(&file, b"\xff\xfe not utf-8").unwrap();
        assert!(update(&file, |entries| push_front(entries, Path::new("/a"))).unwrap());
        assert_eq!(
            parse(&fs::read_to_string(&file).unwrap()),
            vec![entry("/a", None)]
        );
    }

    #[test]
    fn stale_sequences_are_skipped() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("vaults");
        let writer = StateWriter::new(&file);
        let older = writer.reserve();
        let newer = writer.reserve();
        assert!(
            writer
                .apply(newer, |entries| push_front(entries, Path::new("/new")))
                .unwrap()
        );
        assert!(
            !writer
                .apply(older, |entries| push_front(entries, Path::new("/old")))
                .unwrap()
        );
        assert_eq!(
            parse(&fs::read_to_string(&file).unwrap()),
            vec![entry("/new", None)]
        );
        // Re-applying the same request is stale too.
        assert!(!writer.apply(newer, |_| {}).unwrap());
    }

    #[test]
    fn concurrent_updates_do_not_lose_entries() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("vaults");
        let threads: Vec<_> = (0..8)
            .map(|i| {
                let file = file.clone();
                thread::spawn(move || {
                    let path = PathBuf::from(format!("/vault{i}"));
                    update(&file, |entries| push_front(entries, &path)).unwrap()
                })
            })
            .collect();
        for thread in threads {
            assert!(thread.join().unwrap());
        }
        let entries = parse(&fs::read_to_string(&file).unwrap());
        assert_eq!(entries.len(), 8);
        for i in 0..8 {
            assert!(
                entries
                    .iter()
                    .any(|entry| entry.path == Path::new(&format!("/vault{i}"))),
                "vault{i} was lost"
            );
        }
    }
}
