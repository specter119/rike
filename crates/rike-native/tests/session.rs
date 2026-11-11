//! Integration tests for the vault session: real vaults in temporary
//! directories, the pump running on a `LocalSet`, wall-clock debounces.
//!
//! The session is `!Send` (engine-thread design), so every test drives it
//! inside `LocalSet::run_until` on a current-thread runtime.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use rike_core::BlockId;
use rike_core::journal::JournalDate;
use rike_core::names::PageKey;
use rike_native::session::{
    self, Boot, Event, EventSink, Hooks, LeaveVerb, NoticeKind, Session, SessionError, View,
};
use rike_native::startup::VaultKind;
use rike_vault::{Vault, VaultOptions};
use tempfile::TempDir;
use tokio::runtime::Handle;
use tokio::task::LocalSet;

const FAST: Duration = Duration::from_millis(50);
/// Longer than the pump's 300 ms save debounce: watcher events of this vault
/// arrive only after a debounced save has fired, so saves hit real conflicts.
const SLOW: Duration = Duration::from_millis(900);

fn today() -> JournalDate {
    JournalDate::new(2026, 11, 1).unwrap()
}

fn today_rel() -> String {
    "journals/2026_11_01.md".to_owned()
}

struct Fixture {
    dir: TempDir,
    session: Session,
    events: Arc<Mutex<Vec<Event>>>,
    vault: rike_vault::VaultHandle,
    pump: tokio::task::JoinHandle<()>,
}

impl Fixture {
    fn file(&self, rel: &str) -> PathBuf {
        self.dir.path().join(rel)
    }

    fn read(&self, rel: &str) -> Option<String> {
        std::fs::read_to_string(self.file(rel)).ok()
    }
}

fn copy_dir(from: &Path, to: &Path) {
    std::fs::create_dir_all(to).unwrap();
    for entry in std::fs::read_dir(from).unwrap() {
        let entry = entry.unwrap();
        let target = to.join(entry.file_name());
        if entry.file_type().unwrap().is_dir() {
            copy_dir(&entry.path(), &target);
        } else {
            std::fs::copy(entry.path(), target).unwrap();
        }
    }
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/vaults/basic")
}

fn seed_basic(dir: &Path) {
    copy_dir(&fixture_root(), dir);
}

fn seed_page(dir: &Path) {
    std::fs::create_dir_all(dir.join("pages")).unwrap();
    std::fs::write(dir.join("pages/x.md"), "- original\n").unwrap();
}

async fn start(setup: impl FnOnce(&Path), debounce: Duration, hooks: Hooks) -> Fixture {
    let dir = TempDir::new().unwrap();
    setup(dir.path());
    let (vault, events) = Vault::open_with(dir.path(), VaultOptions::new().with_debounce(debounce))
        .await
        .unwrap();
    let collected: Arc<Mutex<Vec<Event>>> = Arc::new(Mutex::new(Vec::new()));
    let sink: EventSink = {
        let collected = collected.clone();
        Arc::new(move |event| collected.lock().unwrap().push(event))
    };
    let session = Session::new(
        vault.clone(),
        Arc::new(tokio::sync::Mutex::new(events)),
        today(),
        Handle::current(),
        hooks,
    );
    session.set_event_sink(Some(sink));
    let pump = tokio::task::spawn_local(session.clone().pump());
    session.start().await;
    Fixture {
        dir,
        session,
        events: collected,
        vault,
        pump,
    }
}

/// Counting hooks for `leave` tests.
#[derive(Clone, Default)]
struct Spy {
    relaunches: Arc<Mutex<Vec<PathBuf>>>,
    closes: Arc<AtomicUsize>,
    fail_relaunch: bool,
}

impl Spy {
    fn hooks(&self) -> Hooks {
        let relaunches = self.relaunches.clone();
        let closes = self.closes.clone();
        let fail = self.fail_relaunch;
        Hooks::new(
            Arc::new(move |path: &Path| {
                relaunches.lock().unwrap().push(path.to_path_buf());
                if fail {
                    return Err(std::io::Error::other("relaunch failed"));
                }
                Ok(())
            }),
            Arc::new(move || {
                closes.fetch_add(1, Ordering::SeqCst);
            }),
        )
    }

    fn closes(&self) -> usize {
        self.closes.load(Ordering::SeqCst)
    }
}

fn has_event(events: &Arc<Mutex<Vec<Event>>>, pred: impl Fn(&Event) -> bool) -> bool {
    events.lock().unwrap().iter().any(pred)
}

async fn until(what: &str, mut check: impl FnMut() -> bool) {
    let started = Instant::now();
    while started.elapsed() < Duration::from_secs(5) {
        if check() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("timed out waiting for {what}");
}

async fn until_event(events: &Arc<Mutex<Vec<Event>>>, what: &str, pred: impl Fn(&Event) -> bool) {
    let events = events.clone();
    until(what, move || has_event(&events, &pred)).await;
}

/// The boot data is process-global: tests that set it must not overlap.
static BOOT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn run(test: impl Future<Output = ()>) {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let local = LocalSet::new();
        local.run_until(test).await;
    });
}

use std::future::Future;

// ---- gen rules ------------------------------------------------------------

#[test]
fn external_keep_mine_reload_and_gen_rules() {
    run(async {
        let f = start(seed_page, FAST, Hooks::default()).await;
        let s = &f.session;
        let rel = "pages/x.md";

        let view = s.page(rel).await.unwrap();
        assert_eq!(view.r#gen, 1);
        let id = view.rows[0].id;

        // Editing while the file changes: External notice, no reload, same gen.
        s.set_editing(rel, 1, id).unwrap();
        assert!(s.set_draft(rel, 1, id, "mine"));
        std::fs::write(f.file(rel), "- theirs\n").unwrap();
        until("the page to go stale", || {
            s.page_view(rel).is_some_and(|v| v.stale)
        })
        .await;
        // The debounced save that fires now must not overwrite the disk version.
        tokio::time::sleep(Duration::from_millis(450)).await;
        let view = s.page_view(rel).unwrap();
        assert_eq!(view.r#gen, 1, "External detection must not bump gen");
        assert!(view.stale);
        assert_eq!(view.rows[0].text, "mine");
        assert_eq!(f.read(rel).as_deref(), Some("- theirs\n"));
        assert!(matches!(
            view.notice.as_ref().map(|n| &n.kind),
            Some(NoticeKind::External { .. })
        ));

        // keep_mine writes the local version over the disk one, same gen.
        assert!(s.keep_mine(rel));
        let path = f.file(rel);
        until("the file to be overwritten", || {
            std::fs::read_to_string(&path).is_ok_and(|c| c == "- mine\n")
        })
        .await;
        let view = s.page_view(rel).unwrap();
        assert_eq!(view.r#gen, 1, "keep_mine must not bump gen");
        assert!(!view.stale);

        // A clean idle page reloads with gen + 1.
        s.clear_editing();
        std::fs::write(f.file(rel), "- v3\n").unwrap();
        until_event(
            &f.events,
            "the reload",
            |e| matches!(e, Event::PageChanged { rel: r, r#gen: 2 } if r == rel),
        )
        .await;
        let view = s.page_view(rel).unwrap();
        assert_eq!(view.r#gen, 2);
        assert_eq!(view.rows[0].text, "v3");

        // Old gens are rejected; the current one is accepted.
        let id = view.rows[0].id;
        assert!(!s.set_draft(rel, 1, id, "old gen"));
        assert!(matches!(
            s.set_editing(rel, 1, id),
            Err(SessionError::StaleGen { .. })
        ));
        assert!(matches!(
            s.toggle_todo(rel, 1, id, None),
            Err(SessionError::StaleGen { .. })
        ));
        assert!(s.set_draft(rel, 2, id, "current"));
        f.pump.abort();
    });
}

#[test]
fn conflict_marks_stale_and_accept_disk_reloads() {
    run(async {
        let f = start(seed_page, SLOW, Hooks::default()).await;
        let s = &f.session;
        let rel = "pages/x.md";

        let view = s.page(rel).await.unwrap();
        let id = view.rows[0].id;
        s.set_editing(rel, 1, id).unwrap();
        // The debounced save (300 ms) fires before the watcher event (900 ms),
        // so the actor still has the old base hash and reports a conflict.
        std::fs::write(f.file(rel), "- theirs\n").unwrap();
        assert!(s.set_draft(rel, 1, id, "mine"));
        until_event(
            &f.events,
            "the conflict notice",
            |e| matches!(e, Event::Notice(n) if matches!(n.kind, NoticeKind::Conflict { .. })),
        )
        .await;
        let view = s.page_view(rel).unwrap();
        assert!(view.stale);
        assert_eq!(view.r#gen, 1, "a conflict must not bump gen");
        assert_eq!(f.read(rel).as_deref(), Some("- theirs\n"));
        let copies = std::fs::read_dir(f.file(".rike/conflicts/pages"))
            .unwrap()
            .count();
        assert_eq!(copies, 1, "the local version must be preserved");

        // accept_disk re-reads the file, bumps gen and clears the edit.
        let view = s.accept_disk(rel).await.unwrap();
        assert_eq!(view.r#gen, 2);
        assert!(!view.stale);
        assert_eq!(view.rows[0].text, "theirs");
        assert!(s.editing().is_none(), "accept_disk clears the edit");
        assert!(view.notice.is_none(), "the conflict notice is resolved");
        f.pump.abort();
    });
}

// ---- leave ----------------------------------------------------------------

#[test]
fn leave_refuses_while_editing_and_an_untouched_journal_stays_unwritten() {
    run(async {
        let spy = Spy::default();
        let f = start(|_| {}, FAST, spy.hooks()).await;
        let s = &f.session;
        let rel = today_rel();

        let view = s.page(&rel).await.unwrap();
        let id = view.rows[0].id;
        s.set_editing(&rel, view.r#gen, id).unwrap();
        let result = s.leave(LeaveVerb::Quit, None).await;
        assert!(!result.ok);
        assert_eq!(spy.closes(), 0);
        assert!(
            result
                .notice
                .as_deref()
                .is_some_and(|n| n.contains("没有退出")),
            "{:?}",
            result.notice
        );

        // The refusal resets the flags; the second attempt goes through.
        s.clear_editing();
        let result = s.leave(LeaveVerb::Quit, None).await;
        assert!(result.ok, "{:?}", result.notice);
        assert_eq!(spy.closes(), 1);
        assert!(
            !f.file(&rel).exists(),
            "a journal nobody typed into must not be written"
        );
        f.pump.abort();
    });
}

#[test]
fn leave_refuses_when_stale_and_does_not_relaunch() {
    run(async {
        let spy = Spy::default();
        let f = start(seed_page, FAST, spy.hooks()).await;
        let s = &f.session;
        let rel = "pages/x.md";
        let target = PathBuf::from("/tmp/rike-other-vault");

        let view = s.page(rel).await.unwrap();
        let id = view.rows[0].id;
        s.set_editing(rel, view.r#gen, id).unwrap();
        assert!(s.set_draft(rel, view.r#gen, id, "mine"));
        std::fs::write(f.file(rel), "- theirs\n").unwrap();
        until("the page to go stale", || {
            s.page_view(rel).is_some_and(|v| v.stale)
        })
        .await;

        let result = s.leave(LeaveVerb::Switch, Some(target.clone())).await;
        assert!(!result.ok);
        assert!(
            result
                .notice
                .as_deref()
                .is_some_and(|n| n.contains("处理后再切换")),
            "{:?}",
            result.notice
        );
        assert!(spy.relaunches.lock().unwrap().is_empty());
        assert_eq!(spy.closes(), 0);

        // After accepting the disk version, switching works.
        s.accept_disk(rel).await.unwrap();
        let result = s.leave(LeaveVerb::Switch, Some(target.clone())).await;
        assert!(result.ok, "{:?}", result.notice);
        assert_eq!(*spy.relaunches.lock().unwrap(), vec![target]);
        assert_eq!(spy.closes(), 1);
        f.pump.abort();
    });
}

#[test]
fn leave_saves_the_draft_and_relaunch_failure_stays() {
    run(async {
        let spy = Spy::default();
        let f = start(|_| {}, FAST, spy.hooks()).await;
        let s = &f.session;
        let rel = today_rel();

        let view = s.page(&rel).await.unwrap();
        let id = view.rows[0].id;
        s.set_editing(&rel, view.r#gen, id).unwrap();
        assert!(s.set_draft(&rel, view.r#gen, id, "last words"));
        s.clear_editing();
        // The 300 ms debounce has not fired; leave must persist the draft.
        let result = s.leave(LeaveVerb::Quit, None).await;
        assert!(result.ok, "{:?}", result.notice);
        assert_eq!(spy.closes(), 1);
        assert_eq!(f.read(&rel).as_deref(), Some("- last words\n"));
        f.pump.abort();
    });

    run(async {
        let spy = Spy {
            fail_relaunch: true,
            ..Spy::default()
        };
        let f = start(|_| {}, FAST, spy.hooks()).await;
        let s = &f.session;
        let result = s
            .leave(LeaveVerb::Switch, Some(PathBuf::from("/tmp/rike-other")))
            .await;
        assert!(!result.ok);
        assert!(
            result
                .notice
                .as_deref()
                .is_some_and(|n| n.contains("无法重启")),
            "{:?}",
            result.notice
        );
        assert_eq!(spy.closes(), 0, "a failed relaunch keeps the window");
        // The flags were reset: quitting can be tried again.
        let result = s.leave(LeaveVerb::Quit, None).await;
        assert!(result.ok, "{:?}", result.notice);
        f.pump.abort();
    });
}

// ---- events and index -----------------------------------------------------

/// The index-change events carry what actually moved, so consumers can
/// dirty-check instead of re-deriving everything on every keystroke.
#[test]
fn index_ver_carries_the_change_set() {
    run(async {
        let f = start(seed_basic, FAST, Hooks::default()).await;
        let s = &f.session;
        let rel = today_rel();

        // First wait out the scan's event, so the edits below get windows
        // of their own.
        until_event(&f.events, "the scan's IndexVer", |e| {
            matches!(e, Event::IndexVer { ver: 1, .. })
        })
        .await;

        // A plain-text edit dirties only the page itself: no backlink keys.
        let view = s.page(&rel).await.unwrap();
        let id = view.rows[0].id;
        s.set_editing(&rel, view.r#gen, id).unwrap();
        assert!(s.set_draft(&rel, view.r#gen, id, "plain text"));
        until_event(&f.events, "IndexVer without backlink keys", |e| {
            matches!(e, Event::IndexVer { ver, changes }
                if *ver == 2
                    && changes.changed == BTreeSet::from([rel.clone()])
                    && changes.backlink_keys.is_empty())
        })
        .await;

        // An edit that adds a link dirties the target's backlink group.
        assert!(s.set_draft(&rel, view.r#gen, id, "see [[Inbox]]"));
        until_event(&f.events, "IndexVer with the target's key", |e| {
            matches!(e, Event::IndexVer { ver, changes }
                if *ver == 3
                    && changes.changed == BTreeSet::from([rel.clone()])
                    && changes.backlink_keys.contains(&PageKey::new("Inbox")))
        })
        .await;
        f.pump.abort();
    });
}

#[test]
fn edits_emit_index_ver_and_save_after_the_debounce() {
    run(async {
        let f = start(|_| {}, FAST, Hooks::default()).await;
        let s = &f.session;
        let rel = today_rel();

        let view = s.page(&rel).await.unwrap();
        let id = view.rows[0].id;
        s.set_editing(&rel, view.r#gen, id).unwrap();
        assert!(s.set_draft(&rel, view.r#gen, id, "typed text"));
        until_event(
            &f.events,
            "IndexVer",
            |e| matches!(e, Event::IndexVer { ver, .. } if *ver >= 1),
        )
        .await;
        let path = f.file(&rel);
        until("the debounced save", || {
            std::fs::read_to_string(&path).is_ok_and(|c| c == "- typed text\n")
        })
        .await;
        f.pump.abort();
    });
}

#[test]
fn editing_a_referenced_block_changes_the_referencing_rows_rev() {
    run(async {
        let f = start(
            |dir| {
                std::fs::create_dir_all(dir.join("pages")).unwrap();
                std::fs::write(
                    dir.join("pages/a.md"),
                    "- target text\n  id:: 6356e902-3b7b-4cb2-8c3e-6a904c813c40\n",
                )
                .unwrap();
                std::fs::write(
                    dir.join("pages/b.md"),
                    "- ref ((6356e902-3b7b-4cb2-8c3e-6a904c813c40))\n",
                )
                .unwrap();
            },
            FAST,
            Hooks::default(),
        )
        .await;
        let s = &f.session;

        let before = s.page("pages/b.md").await.unwrap();
        assert_eq!(before.rows.len(), 1);
        assert!(
            before.rows[0].markdown.contains("target text"),
            "{:?}",
            before.rows[0].markdown
        );

        let a = s.page("pages/a.md").await.unwrap();
        let id = a.rows[0].id;
        assert!(s.set_draft("pages/a.md", a.r#gen, id, "changed text"));

        let after = s.page("pages/b.md").await.unwrap();
        assert!(
            after.rows[0].markdown.contains("changed text"),
            "{:?}",
            after.rows[0].markdown
        );
        assert_ne!(before.rows[0].rev, after.rows[0].rev);
        f.pump.abort();
    });
}

#[test]
fn search_backlinks_and_the_pages_event() {
    run(async {
        let f = start(
            |dir| {
                std::fs::create_dir_all(dir.join("pages")).unwrap();
                std::fs::create_dir_all(dir.join("journals")).unwrap();
                std::fs::write(dir.join("pages/Inbox.md"), "- one\n").unwrap();
                std::fs::write(dir.join("journals/2026_10_30.md"), "- see [[Inbox]]\n").unwrap();
            },
            FAST,
            Hooks::default(),
        )
        .await;
        let s = &f.session;

        // The scan announced the page list.
        assert!(has_event(&f.events, |e| matches!(e, Event::Pages)));
        let hits = s.search("inbox");
        assert!(!hits.is_empty(), "search should find the Inbox page");

        let groups = s.backlinks("pages/Inbox.md");
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].source.rel_path, "journals/2026_10_30.md");
        f.pump.abort();
    });
}

// ---- structure ops --------------------------------------------------------

#[test]
fn structure_ops_return_focus_and_page_and_save() {
    run(async {
        let f = start(|_| {}, FAST, Hooks::default()).await;
        let s = &f.session;
        let rel = today_rel();

        let view = s.page(&rel).await.unwrap();
        let id = view.rows[0].id;

        // Enter splits at the caret and focuses the new block.
        let result = s.split(&rel, 1, id, "parent".to_owned(), Some(6)).unwrap();
        assert_eq!(result.page.rows.len(), 2);
        assert_eq!(result.page.rows[0].text, "parent");
        assert_eq!(result.page.rows[1].text, "");
        let focus = result.focus.unwrap();
        assert!(focus.caret_start);
        assert_eq!(result.page.rows[1].id, focus.id);

        // Tab indents the new block under "parent".
        let result = s.indent(&rel, 1, focus.id, None).unwrap();
        assert_eq!(result.page.rows[1].depth, 1);
        assert_eq!(
            result.focus,
            Some(session::Focus {
                id: focus.id,
                caret_start: false,
                caret: None
            })
        );

        // Structure ops save immediately, without the typing debounce.
        let path = f.file(&rel);
        until("the structure save", || {
            std::fs::read_to_string(&path).is_ok_and(|c| c == "- parent\n\t-\n")
        })
        .await;

        // Collapse hides the children.
        assert!(result.page.rows[0].has_children);
        let result = s.toggle_collapse(&rel, 1, id).unwrap();
        assert!(result.page.rows[0].collapsed);
        assert_eq!(result.page.rows.len(), 1);
        let result = s.toggle_collapse(&rel, 1, id).unwrap();
        assert_eq!(result.page.rows.len(), 2);

        // Enter on an empty indented block outdents instead of splitting.
        let result = s.split(&rel, 1, focus.id, String::new(), Some(0)).unwrap();
        assert_eq!(result.page.rows.len(), 2);
        assert_eq!(result.page.rows[1].depth, 0);

        // Ctrl+Enter cycles the task marker; the checkbox sets it directly.
        let result = s.toggle_todo(&rel, 1, id, None).unwrap();
        assert_eq!(result.page.rows[0].text, "TODO parent");
        assert_eq!(
            result.page.rows[0].task,
            Some(rike_core::task::Marker::Todo)
        );
        let result = s.set_task_done(&rel, 1, id, true).unwrap();
        assert_eq!(result.page.rows[0].text, "DONE parent");
        let result = s.set_task_done(&rel, 1, id, false).unwrap();
        assert_eq!(result.page.rows[0].text, "TODO parent");

        // Removing a childless block focuses the previous visible one.
        let result = s.remove(&rel, 1, focus.id, None).unwrap();
        assert_eq!(result.page.rows.len(), 1);
        assert_eq!(
            result.focus,
            Some(session::Focus {
                id,
                caret_start: false,
                caret: None
            })
        );
        f.pump.abort();
    });
}

#[test]
fn undo_and_redo_restore_structure_snapshots() {
    run(async {
        let f = start(|_| {}, FAST, Hooks::default()).await;
        let s = &f.session;
        let rel = today_rel();

        let view = s.page(&rel).await.unwrap();
        let id = view.rows[0].id;
        let result = s.split(&rel, 1, id, "a".to_owned(), Some(1)).unwrap();
        let second = result.focus.unwrap().id;
        s.indent(&rel, 1, second, None).unwrap();

        let result = s.undo(&rel, 1).unwrap();
        assert_eq!(result.page.rows.len(), 2);
        assert_eq!(result.page.rows[1].depth, 0, "undo of the indent");
        let result = s.undo(&rel, 1).unwrap();
        assert_eq!(result.page.rows.len(), 1);
        assert_eq!(result.page.rows[0].text, "a", "undo of the split");
        let result = s.redo(&rel, 1).unwrap();
        assert_eq!(result.page.rows.len(), 2);
        let result = s.redo(&rel, 1).unwrap();
        assert_eq!(result.page.rows[1].depth, 1);

        // While an input owns Ctrl+Z, structural undo refuses.
        s.set_editing(&rel, 1, result.page.rows[0].id).unwrap();
        assert!(matches!(s.undo(&rel, 1), Err(SessionError::Editing)));
        s.clear_editing();
        f.pump.abort();
    });
}

// ---- read-only ------------------------------------------------------------

#[test]
fn read_only_pages_reject_edits() {
    run(async {
        let f = start(seed_page, FAST, Hooks::default()).await;
        let s = &f.session;
        let rel = "pages/x.md";

        let view = s.page(rel).await.unwrap();
        let id = view.rows[0].id;
        s.force_read_only(rel);
        let view = s.page_view(rel).unwrap();
        assert!(view.read_only);

        assert!(!s.set_draft(rel, view.r#gen, id, "nope"));
        assert!(matches!(
            s.set_editing(rel, view.r#gen, id),
            Err(SessionError::ReadOnly(_))
        ));
        assert!(matches!(
            s.toggle_todo(rel, view.r#gen, id, None),
            Err(SessionError::ReadOnly(_))
        ));
        assert!(matches!(
            s.indent(rel, view.r#gen, id, None),
            Err(SessionError::ReadOnly(_))
        ));
        // A read-only page never blocks leaving.
        let result = s.leave(LeaveVerb::Quit, None).await;
        assert!(result.ok, "{:?}", result.notice);
        assert_eq!(f.read(rel).as_deref(), Some("- original\n"));
        f.pump.abort();
    });
}

#[test]
fn the_vault_actor_rejects_writes_of_read_only_pages() {
    run(async {
        let f = start(seed_page, FAST, Hooks::default()).await;
        let s = &f.session;
        let rel = "pages/x.md";

        let view = s.page(rel).await.unwrap();
        let id = view.rows[0].id;
        // The session loaded the page as writable; the actor disagrees.
        f.vault.set_read_only(rel, true).await.unwrap();
        assert!(s.set_draft(rel, view.r#gen, id, "mine"));
        until_event(
            &f.events,
            "the read-only notice",
            |e| matches!(e, Event::Notice(n) if n.message.contains("为只读")),
        )
        .await;
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(f.read(rel).as_deref(), Some("- original\n"));
        f.pump.abort();
    });
}

// ---- journals -------------------------------------------------------------

#[test]
fn journals_load_more_and_today_comes_first() {
    run(async {
        let f = start(
            |dir| {
                std::fs::create_dir_all(dir.join("journals")).unwrap();
                std::fs::write(dir.join("journals/2026_10_30.md"), "- two days ago\n").unwrap();
                std::fs::write(dir.join("journals/2026_10_31.md"), "- yesterday\n").unwrap();
            },
            FAST,
            Hooks::default(),
        )
        .await;
        let s = &f.session;

        let one = s.journals(1).await;
        assert_eq!(one.entries.len(), 1);
        assert!(one.more);
        assert_eq!(one.entries[0].rel, today_rel());
        assert_eq!(one.entries[0].title, "Nov 1st, 2026");

        let all = s.journals(10).await;
        assert!(!all.more);
        assert_eq!(all.entries.len(), 3);
        assert_eq!(all.entries[1].rel, "journals/2026_10_31.md");
        assert_eq!(all.entries[2].rel, "journals/2026_10_30.md");
        f.pump.abort();
    });
}

// ---- boot, open, inspect ----------------------------------------------------

fn test_boot(handle: Handle, state_file: Option<PathBuf>) -> Boot {
    Boot {
        vault: None,
        events: None,
        today: today(),
        handle,
        view: View::Journals,
        notice: None,
        recent: Vec::new(),
        relaunch: Arc::new(|_| Ok(())),
        state_file,
    }
}

#[test]
fn boot_is_repeatable_and_open_replaces_it() {
    let _boot = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    run(async {
        let state_dir = TempDir::new().unwrap();
        let state_file = state_dir.path().join("vaults");
        session::set_boot(test_boot(Handle::current(), Some(state_file.clone())));

        // boot() clones and never takes.
        assert!(session::boot().is_some());
        assert!(session::boot().is_some());

        let dir = TempDir::new().unwrap();
        let booted = session::open(dir.path()).await.unwrap();
        let root = booted.vault.as_ref().unwrap().root().to_path_buf();
        assert_eq!(root, dir.path().canonicalize().unwrap());
        // Repeatable after open too, and the recent list was updated.
        let again = session::boot().unwrap();
        assert!(again.vault.is_some());
        assert!(
            booted
                .recent
                .iter()
                .any(|p| p == root.to_string_lossy().as_ref())
        );
        // The state file is written off-thread.
        let mut on_disk = String::new();
        for _ in 0..100 {
            on_disk = std::fs::read_to_string(&state_file).unwrap_or_default();
            if on_disk.contains(root.to_str().unwrap()) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        assert!(on_disk.contains(root.to_str().unwrap()));

        // A session can be built from the boot data.
        let s = Session::from_boot(&again, Hooks::default()).unwrap();
        assert_eq!(s.today(), today());
    });
}

#[test]
fn inspect_classifies_and_refuses() {
    let _boot = BOOT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    run(async {
        session::set_boot(test_boot(Handle::current(), None));

        let dir = TempDir::new().unwrap();
        let empty = session::inspect(dir.path()).await;
        assert_eq!(empty.kind, VaultKind::Empty);
        assert_eq!(empty.reason, None);
        assert!(!empty.is_current);
        assert_eq!(
            empty.canonical.as_deref(),
            Some(dir.path().canonicalize().unwrap().as_path())
        );

        std::fs::create_dir(dir.path().join("journals")).unwrap();
        let vault = session::inspect(dir.path()).await;
        assert_eq!(vault.kind, VaultKind::Vault);

        std::fs::remove_dir(dir.path().join("journals")).unwrap();
        std::fs::write(dir.path().join("notes.txt"), "hi").unwrap();
        let other = session::inspect(dir.path()).await;
        assert_eq!(other.kind, VaultKind::Other);

        let home = std::env::var_os("HOME").map(PathBuf::from);
        if let Some(home) = home {
            let refused = session::inspect(&home).await;
            assert!(refused.reason.is_some(), "HOME must be refused");
        }
        let root_refused = session::inspect(Path::new("/")).await;
        assert!(root_refused.reason.is_some(), "/ must be refused");

        let file = dir.path().join("notes.txt");
        let not_a_dir = session::inspect(&file).await;
        assert!(not_a_dir.reason.is_some(), "a plain file is refused");
    });
}

// ---- fixture-based smoke ----------------------------------------------------

#[test]
fn the_basic_fixture_indexes_pages_and_resolves_refs() {
    run(async {
        let f = start(seed_basic, FAST, Hooks::default()).await;
        let s = &f.session;

        let pages = s.pages();
        assert!(pages.iter().any(|p| p.title == "项目/日课"), "{pages:?}");
        let view = s.page("pages/项目___日课.md").await.unwrap();
        assert_eq!(view.title, "项目/日课");
        // The collapsed block hides its child; the code fence stays one block.
        let code = view.rows.iter().find(|r| r.text.starts_with("代码示例"));
        assert!(code.is_some(), "{:?}", view.rows);
        let hits = s.search("meeting");
        assert!(!hits.is_empty());
        f.pump.abort();
    });
}

// BlockId is part of the public API surface; keep the import used.
#[allow(unused)]
fn _api_surface(id: BlockId) {}
