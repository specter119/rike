//! Integration tests for the vault actor, driven through the public API.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime};

use rike_vault::{
    Clock, ContentHash, RejectReason, SaveOutcome, Vault, VaultError, VaultEvent, VaultHandle,
    VaultOptions,
};
use tempfile::TempDir;
use tokio::sync::mpsc;

/// Debounce used by tests; kept small so event tests stay fast.
const TEST_DEBOUNCE: Duration = Duration::from_millis(120);
/// Generous upper bound for a watcher round trip.
const EVENT_TIMEOUT: Duration = Duration::from_secs(10);
/// Window in which no watcher event may appear (self-write suppression).
const SETTLE: Duration = Duration::from_secs(1);

struct TestVault {
    // Held so the temporary directory outlives the vault.
    _dir: TempDir,
    handle: VaultHandle,
    events: mpsc::UnboundedReceiver<VaultEvent>,
}

async fn open_vault() -> TestVault {
    open_with(VaultOptions::default().with_debounce(TEST_DEBOUNCE)).await
}

async fn open_with(options: VaultOptions) -> TestVault {
    let dir = TempDir::new().expect("temp dir");
    let root = dir.path().join("vault");
    let (handle, events) = Vault::open_with(&root, options).await.expect("open vault");
    TestVault {
        _dir: dir,
        handle,
        events,
    }
}

impl TestVault {
    fn path(&self, rel: &str) -> PathBuf {
        self.handle.root().join(rel)
    }

    fn read(&self, rel: &str) -> String {
        std::fs::read_to_string(self.path(rel)).expect("read file")
    }

    fn write(&self, rel: &str, content: &str) {
        let path = self.path(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).expect("create parent");
        }
        std::fs::write(path, content).expect("write file");
    }

    fn backups(&self, rel: &str) -> Vec<String> {
        let dir = self.path(".rike/backups").join(rel);
        let Ok(entries) = std::fs::read_dir(&dir) else {
            return Vec::new();
        };
        let mut backups: Vec<String> = entries
            .map(|entry| entry.expect("backup entry").path())
            .map(|path| std::fs::read_to_string(path).expect("read backup"))
            .collect();
        backups.sort();
        backups
    }

    async fn wait(&mut self, pred: impl Fn(&VaultEvent) -> bool) -> VaultEvent {
        tokio::time::timeout(EVENT_TIMEOUT, async {
            loop {
                let event = self
                    .events
                    .recv()
                    .await
                    .expect("vault event channel closed");
                if pred(&event) {
                    return event;
                }
            }
        })
        .await
        .expect("timed out waiting for a vault event")
    }

    async fn expect_quiet(&mut self, window: Duration) {
        if let Ok(Some(event)) = tokio::time::timeout(window, self.events.recv()).await {
            panic!("unexpected vault event: {event:?}");
        }
    }
}

/// Clock under test control, used to drive backup throttling.
struct ManualClock {
    now: Mutex<SystemTime>,
}

impl ManualClock {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            now: Mutex::new(SystemTime::UNIX_EPOCH + Duration::from_secs(1_700_000_000)),
        })
    }

    fn advance(&self, by: Duration) {
        let mut now = self.now.lock().expect("clock lock");
        *now += by;
    }
}

impl Clock for ManualClock {
    fn now(&self) -> SystemTime {
        *self.now.lock().expect("clock lock")
    }
}

fn change_of(event: &VaultEvent, rel: &str) -> Option<ContentHash> {
    match event {
        VaultEvent::Changed {
            rel: changed, hash, ..
        } if changed == rel => Some(*hash),
        _ => None,
    }
}

#[tokio::test]
async fn open_creates_vault_structure() {
    let vault = open_vault().await;
    let root = vault.handle.root().to_path_buf();
    assert!(root.is_absolute());
    assert_eq!(
        root,
        std::fs::canonicalize(&root).expect("canonical root"),
        "root must be canonicalized"
    );
    for sub in ["tmp", "backups", "conflicts"] {
        assert!(root.join(".rike").join(sub).is_dir(), "missing .rike/{sub}");
    }
}

#[tokio::test]
async fn open_creates_missing_root_directories() {
    let dir = TempDir::new().expect("temp dir");
    let root = dir.path().join("nested/vault");
    let (_handle, _events) = Vault::open(&root).await.expect("open nested vault");
    assert!(root.join(".rike/tmp").is_dir());
}

#[tokio::test]
async fn list_pages_ignores_irrelevant_paths() {
    let vault = open_vault().await;
    vault.write("pages/a.md", "- a\n");
    vault.write("pages/sub/c.md", "- c\n");
    vault.write("journals/2024_01_01.md", "- j\n");
    vault.write("pages/notes.txt", "ignored");
    vault.write("logseq/pages.md", "ignored");
    vault.write(".git/pages.md", "ignored");
    vault.write(".rike/pages.md", "ignored");
    vault.write(".rike/extra/pages.md", "ignored");
    vault.write("other.md", "ignored");

    let pages = vault.handle.list_pages().await.expect("list pages");
    let rels: Vec<&str> = pages.iter().map(|page| page.rel.as_str()).collect();
    assert_eq!(
        rels,
        ["journals/2024_01_01.md", "pages/a.md", "pages/sub/c.md"]
    );

    let page = pages
        .iter()
        .find(|page| page.rel == "pages/a.md")
        .expect("page entry");
    assert_eq!(page.len, 4);
    assert!(page.modified.is_some());
}

#[tokio::test]
async fn load_and_save_roundtrip() {
    let vault = open_vault().await;
    let content = "- one\n- two\n\t- nested\n".to_string();

    let outcome = vault
        .handle
        .save("pages/a.md", content.clone())
        .await
        .expect("save");
    assert_eq!(
        outcome,
        SaveOutcome::Saved {
            hash: ContentHash::of(&content)
        }
    );
    assert_eq!(vault.read("pages/a.md"), content);

    let loaded = vault.handle.load("pages/a.md").await.expect("load");
    assert_eq!(loaded.content, content);
    assert_eq!(loaded.hash, ContentHash::of(&content));

    let missing = vault.handle.load("pages/missing.md").await;
    assert!(matches!(missing, Err(VaultError::NotFound(_))));
}

#[tokio::test]
async fn invalid_utf8_is_reported() {
    let vault = open_vault().await;
    let path = vault.path("pages/bad.md");
    std::fs::create_dir_all(path.parent().expect("parent")).expect("create pages");
    std::fs::write(&path, [0xff, 0xfe, 0xfd]).expect("write bytes");

    let result = vault.handle.load("pages/bad.md").await;
    assert!(matches!(result, Err(VaultError::InvalidUtf8(_))));
}

#[tokio::test]
async fn atomic_write_leaves_no_temporary_files() {
    let vault = open_vault().await;
    vault
        .handle
        .save("pages/deep/a.md", "- a\n".to_string())
        .await
        .expect("save");
    vault
        .handle
        .save("pages/deep/a.md", "- b\n".to_string())
        .await
        .expect("save again");

    let leftovers: Vec<PathBuf> = std::fs::read_dir(vault.path(".rike/tmp"))
        .expect("read tmp dir")
        .map(|entry| entry.expect("entry").path())
        .collect();
    assert!(leftovers.is_empty(), "leftover temp files: {leftovers:?}");
}

#[cfg(unix)]
#[tokio::test]
async fn unchanged_content_takes_the_fast_path() {
    use std::os::unix::fs::MetadataExt;

    let vault = open_vault().await;
    let content = "- same\n".to_string();
    vault
        .handle
        .save("pages/a.md", content.clone())
        .await
        .expect("save");
    let inode_before = std::fs::metadata(vault.path("pages/a.md"))
        .expect("metadata")
        .ino();

    let outcome = vault
        .handle
        .save("pages/a.md", content.clone())
        .await
        .expect("save");
    assert_eq!(
        outcome,
        SaveOutcome::Saved {
            hash: ContentHash::of(&content)
        }
    );

    let inode_after = std::fs::metadata(vault.path("pages/a.md"))
        .expect("metadata")
        .ino();
    assert_eq!(
        inode_before, inode_after,
        "unchanged content must not rewrite the file"
    );
    assert!(vault.backups("pages/a.md").is_empty());
}

#[tokio::test]
async fn external_modification_conflicts_and_preserves_the_local_version() {
    let vault = open_vault().await;
    vault
        .handle
        .save("pages/a.md", "- local v1\n".to_string())
        .await
        .expect("save");
    vault.handle.load("pages/a.md").await.expect("load");

    vault.write("pages/a.md", "- external\n");
    let outcome = vault
        .handle
        .save("pages/a.md", "- local v2\n".to_string())
        .await
        .expect("save");

    let SaveOutcome::Conflict {
        disk_content,
        conflict_copy,
    } = outcome
    else {
        panic!("expected conflict, got {outcome:?}");
    };
    assert_eq!(disk_content.as_deref(), Some("- external\n"));
    assert!(
        conflict_copy.starts_with(".rike/conflicts/pages/a.md."),
        "unexpected conflict path {conflict_copy}"
    );
    assert_eq!(vault.read(&conflict_copy), "- local v2\n");
    // The disk version wins; the caller's file is untouched.
    assert_eq!(vault.read("pages/a.md"), "- external\n");

    // Base hash now matches the disk, so the next save goes through.
    let outcome = vault
        .handle
        .save("pages/a.md", "- local v2\n".to_string())
        .await
        .expect("save");
    assert!(matches!(outcome, SaveOutcome::Saved { .. }));
    assert_eq!(vault.read("pages/a.md"), "- local v2\n");
}

#[tokio::test]
async fn untracked_existing_file_conflicts_on_first_save() {
    let vault = open_vault().await;
    vault.write("pages/a.md", "- on disk\n");

    let outcome = vault
        .handle
        .save("pages/a.md", "- from caller\n".to_string())
        .await
        .expect("save");
    let SaveOutcome::Conflict {
        disk_content,
        conflict_copy,
    } = outcome
    else {
        panic!("expected conflict, got {outcome:?}");
    };
    assert_eq!(disk_content.as_deref(), Some("- on disk\n"));
    assert_eq!(vault.read(&conflict_copy), "- from caller\n");
}

#[tokio::test]
async fn external_delete_conflicts_without_disk_content() {
    let vault = open_vault().await;
    vault
        .handle
        .save("pages/a.md", "- v1\n".to_string())
        .await
        .expect("save");
    vault.handle.load("pages/a.md").await.expect("load");
    std::fs::remove_file(vault.path("pages/a.md")).expect("remove page");

    let outcome = vault
        .handle
        .save("pages/a.md", "- v2\n".to_string())
        .await
        .expect("save");
    let SaveOutcome::Conflict {
        disk_content,
        conflict_copy,
    } = outcome
    else {
        panic!("expected conflict, got {outcome:?}");
    };
    assert_eq!(disk_content, None);
    assert_eq!(vault.read(&conflict_copy), "- v2\n");

    // The page is now considered absent, so a retry recreates it.
    let outcome = vault
        .handle
        .save("pages/a.md", "- v2\n".to_string())
        .await
        .expect("save");
    assert!(matches!(outcome, SaveOutcome::Saved { .. }));
    assert_eq!(vault.read("pages/a.md"), "- v2\n");
}

#[tokio::test]
async fn backups_are_throttled_and_pruned() {
    let clock = ManualClock::new();
    let vault = open_with(
        VaultOptions::default()
            .with_backup_limit(3)
            .with_backup_interval(Duration::from_secs(600))
            .with_clock(clock.clone())
            .with_debounce(TEST_DEBOUNCE),
    )
    .await;

    vault
        .handle
        .save("pages/a.md", "- v1\n".to_string())
        .await
        .expect("save v1");
    vault.handle.load("pages/a.md").await.expect("load");
    assert!(vault.backups("pages/a.md").is_empty(), "no backup yet");

    vault
        .handle
        .save("pages/a.md", "- v2\n".to_string())
        .await
        .expect("save v2");
    assert_eq!(
        vault.backups("pages/a.md"),
        ["- v1\n"],
        "first overwrite after load backs up"
    );

    vault
        .handle
        .save("pages/a.md", "- v3\n".to_string())
        .await
        .expect("save v3");
    assert_eq!(
        vault.backups("pages/a.md"),
        ["- v1\n"],
        "a rapid second overwrite must not back up"
    );

    clock.advance(Duration::from_secs(601));
    vault
        .handle
        .save("pages/a.md", "- v4\n".to_string())
        .await
        .expect("save v4");
    assert_eq!(vault.backups("pages/a.md"), ["- v1\n", "- v3\n"]);

    clock.advance(Duration::from_secs(601));
    vault
        .handle
        .save("pages/a.md", "- v5\n".to_string())
        .await
        .expect("save v5");
    assert_eq!(vault.backups("pages/a.md"), ["- v1\n", "- v3\n", "- v4\n"]);

    clock.advance(Duration::from_secs(601));
    vault
        .handle
        .save("pages/a.md", "- v6\n".to_string())
        .await
        .expect("save v6");
    assert_eq!(
        vault.backups("pages/a.md"),
        ["- v3\n", "- v4\n", "- v5\n"],
        "oldest backups are pruned"
    );
}

#[tokio::test]
async fn read_only_pages_reject_saves() {
    let vault = open_vault().await;
    vault
        .handle
        .set_read_only("pages/a.md", true)
        .await
        .expect("set read only");

    let outcome = vault
        .handle
        .save("pages/a.md", "- blocked\n".to_string())
        .await
        .expect("save");
    assert_eq!(outcome, SaveOutcome::Rejected(RejectReason::ReadOnly));
    assert!(!vault.path("pages/a.md").exists());

    vault
        .handle
        .set_read_only("pages/a.md", false)
        .await
        .expect("clear read only");
    let outcome = vault
        .handle
        .save("pages/a.md", "- allowed\n".to_string())
        .await
        .expect("save");
    assert!(matches!(outcome, SaveOutcome::Saved { .. }));
    assert_eq!(vault.read("pages/a.md"), "- allowed\n");
}

#[tokio::test]
async fn own_saves_do_not_emit_events() {
    let mut vault = open_vault().await;
    vault
        .handle
        .save("pages/a.md", "- v1\n".to_string())
        .await
        .expect("save");
    vault.expect_quiet(SETTLE).await;

    vault
        .handle
        .save("pages/a.md", "- v2\n".to_string())
        .await
        .expect("save");
    vault.expect_quiet(SETTLE).await;

    // A brand new page in a brand new directory also stays silent.
    vault
        .handle
        .save("pages/fresh/dir/b.md", "- b\n".to_string())
        .await
        .expect("save");
    vault.expect_quiet(SETTLE).await;
}

#[tokio::test]
async fn external_edit_emits_changed_with_content() {
    let mut vault = open_vault().await;
    vault.write("pages/a.md", "- v1\n");

    let event = vault
        .wait(|event| change_of(event, "pages/a.md") == Some(ContentHash::of("- v1\n")))
        .await;
    let VaultEvent::Changed { content, .. } = event else {
        unreachable!("predicate only matches changes");
    };
    assert_eq!(content, "- v1\n");

    vault.write("pages/a.md", "- v2\n");
    let event = vault
        .wait(|event| change_of(event, "pages/a.md") == Some(ContentHash::of("- v2\n")))
        .await;
    let VaultEvent::Changed { content, .. } = event else {
        unreachable!("predicate only matches changes");
    };
    assert_eq!(content, "- v2\n");
}

#[tokio::test]
async fn external_delete_emits_removed() {
    let mut vault = open_vault().await;
    vault.write("pages/a.md", "- v1\n");
    vault.handle.load("pages/a.md").await.expect("load");
    // The watcher event for the write above may arrive before or after the
    // load and is ignored in the latter case, so wait for it to drain instead
    // of expecting a specific event.
    tokio::time::sleep(SETTLE).await;

    std::fs::remove_file(vault.path("pages/a.md")).expect("remove page");
    let event = vault
        .wait(|event| matches!(event, VaultEvent::Removed { rel } if rel == "pages/a.md"))
        .await;
    assert_eq!(
        event,
        VaultEvent::Removed {
            rel: "pages/a.md".to_string()
        }
    );
}

#[tokio::test]
async fn file_in_new_subdirectory_emits_created_and_changed() {
    let mut vault = open_vault().await;
    vault.write("pages/new/probe.md", "- probe\n");

    // Observing the probe proves the freshly created directory is watched.
    vault
        .wait(|event| match event {
            VaultEvent::Created { rel } | VaultEvent::Changed { rel, .. } => {
                rel == "pages/new/probe.md"
            }
            _ => false,
        })
        .await;

    vault.write("pages/new/c.md", "- child\n");
    vault
        .wait(|event| matches!(event, VaultEvent::Created { rel } if rel == "pages/new/c.md"))
        .await;
    let event = vault
        .wait(|event| change_of(event, "pages/new/c.md") == Some(ContentHash::of("- child\n")))
        .await;
    let VaultEvent::Changed { content, .. } = event else {
        unreachable!("predicate only matches changes");
    };
    assert_eq!(content, "- child\n");
}

#[tokio::test]
async fn cjk_page_names_work_end_to_end() {
    let mut vault = open_vault().await;
    let rel = "pages/中文 页面 🎋.md";
    let content = "- 第一行\n\t- 子块\n".to_string();
    vault
        .handle
        .save(rel, content.clone())
        .await
        .expect("save cjk page");

    let pages = vault.handle.list_pages().await.expect("list pages");
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].rel, rel);

    let loaded = vault.handle.load(rel).await.expect("load cjk page");
    assert_eq!(loaded.content, content);

    vault.write(rel, "- 外部修改\n");
    vault
        .wait(|event| change_of(event, rel) == Some(ContentHash::of("- 外部修改\n")))
        .await;
}

#[tokio::test]
async fn invalid_paths_are_rejected() {
    let vault = open_vault().await;
    let invalid = [
        "",
        "/abs/page.md",
        "../escape.md",
        "pages/../../escape.md",
        "pages\\win.md",
        "logseq/x.md",
        ".git/x.md",
        ".rike/x.md",
        "pages/notes.txt",
        "pages//x.md",
    ];
    for rel in invalid {
        let err = vault
            .handle
            .save(rel, "- x\n".to_string())
            .await
            .expect_err("save must fail");
        assert!(
            matches!(err, VaultError::InvalidPath(_)),
            "save({rel:?}) => {err:?}"
        );

        let err = vault.handle.load(rel).await.expect_err("load must fail");
        assert!(
            matches!(err, VaultError::InvalidPath(_)),
            "load({rel:?}) => {err:?}"
        );
    }
}

#[tokio::test]
async fn flush_covers_commands_sent_before_it() {
    let vault = open_vault().await;
    let handle = vault.handle.clone();

    let first = {
        let handle = handle.clone();
        async move { handle.save("pages/p1.md", "- 1\n".to_string()).await }
    };
    let second = {
        let handle = handle.clone();
        async move { handle.save("pages/p2.md", "- 2\n".to_string()).await }
    };
    let third = {
        let handle = handle.clone();
        async move { handle.save("pages/p3.md", "- 3\n".to_string()).await }
    };
    // `join!` polls the saves before `flush`, so `flush` is queued behind them.
    let (first, second, third, flushed) = tokio::join!(first, second, third, handle.flush());
    first.expect("save 1");
    second.expect("save 2");
    third.expect("save 3");
    flushed.expect("flush");

    for (rel, content) in [
        ("pages/p1.md", "- 1\n"),
        ("pages/p2.md", "- 2\n"),
        ("pages/p3.md", "- 3\n"),
    ] {
        assert_eq!(
            vault.read(rel),
            content,
            "{rel} must be on disk after flush"
        );
    }
}

#[tokio::test]
async fn config_edn_is_read_when_present() {
    let vault = open_vault().await;
    assert_eq!(
        vault.handle.read_config_edn().await.expect("read config"),
        None
    );

    vault.write(
        "logseq/config.edn",
        "{:journal/file-name-format \"yyyy_MM_dd\"}\n",
    );
    assert_eq!(
        vault.handle.read_config_edn().await.expect("read config"),
        Some("{:journal/file-name-format \"yyyy_MM_dd\"}\n".to_string())
    );
    // The config file is never treated as a page.
    assert!(vault.handle.list_pages().await.expect("list").is_empty());
}

#[tokio::test]
async fn conflict_copies_can_be_written_directly() {
    let vault = open_vault().await;
    let rel = vault
        .handle
        .save_conflict_copy("pages/a.md", "- mine\n".to_string())
        .await
        .expect("save conflict copy");
    assert!(rel.starts_with(".rike/conflicts/pages/a.md."));
    assert_eq!(vault.read(&rel), "- mine\n");
    assert!(vault.handle.list_pages().await.expect("list").is_empty());
}

#[tokio::test]
async fn save_after_external_change_is_visible_to_load() {
    let vault = open_vault().await;
    vault
        .handle
        .save("pages/a.md", "- v1\n".to_string())
        .await
        .expect("save");
    vault.write("pages/a.md", "- external\n");

    let loaded = vault.handle.load("pages/a.md").await.expect("load");
    assert_eq!(loaded.content, "- external\n");
    let outcome = vault
        .handle
        .save("pages/a.md", "- mine\n".to_string())
        .await
        .expect("save");
    assert!(matches!(outcome, SaveOutcome::Saved { .. }));
    assert_eq!(vault.read("pages/a.md"), "- mine\n");
}
