//! The vault session: page state, `gen` tokens, editing, saving and the
//! resident pump task.
//!
//! This module is NSH-free. A `Session` is meant to live on one thread (the
//! engine thread's `thread_local`): methods borrow
//! the inner `RefCell` state and never hold the borrow across an `.await`.
//! Disk IO goes through the vault actor; the [`Session::pump`] future merges
//! watcher events, save requests and the save debounce and must be driven by
//! the host (NSH `spawn` on the engine thread, or a `tokio::task::LocalSet`
//! in tests).

use std::cell::RefCell;
use std::collections::HashMap;
use std::future::Future;
use std::io;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::rc::Rc;
use std::sync::{Arc, Mutex as StdMutex};
use std::task::{Context, Poll};
use std::time::Duration;

use rike_core::config::{VaultConfig, parse_config_edn};
use rike_core::journal::JournalDate;
use rike_core::names::{PageKey, title_to_file_stem};
use rike_core::ops;
use rike_core::parse::parse_page;
use rike_core::serialize::serialize_page;
use rike_core::task::Marker;
use rike_core::{Block, BlockId, Page};
use rike_index::resolve_title;
use rike_index::{BacklinkGroup, BlockInfo, Index, MemIndex, PageEntry, PageSource, SearchHit};
use rike_vault::{SaveOutcome, Vault, VaultError, VaultEvent, VaultHandle};
use tokio::runtime::Handle;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tokio::time::{Instant, Sleep};

use crate::doc::Doc;
use crate::md;
use crate::startup;

/// Typing is written back after this debounce; structure ops save at once.
const SAVE_DELAY: Duration = Duration::from_millis(300);
/// Index-change events are coalesced over this window.
const INDEX_DELAY: Duration = Duration::from_millis(300);
const UNDO_LIMIT: usize = 100;
const SEARCH_LIMIT: usize = 30;
/// Disarmed save timer: far enough to count as "never".
const NEVER: Duration = Duration::from_secs(24 * 3600);

/// Something the TS side should react to.
#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// A page's in-memory document was replaced by disk content.
    PageChanged {
        /// Vault-relative path.
        rel: String,
        /// New generation token; drafts and ops carrying an older one are rejected.
        r#gen: u64,
    },
    /// A toast or a page-level conflict banner.
    Notice(Notice),
    /// The index changed; re-fetch visible pages and rebuild rows whose `rev` changed.
    IndexVer(u64),
    /// The page list changed (a file appeared or disappeared).
    Pages,
}

/// Where events go. The api layer wraps a TS callback in one of these.
pub type EventSink = Arc<dyn Fn(Event) + Send + Sync>;

#[derive(Clone, Debug, PartialEq)]
pub struct Notice {
    pub id: u64,
    pub rel: String,
    pub message: String,
    pub kind: NoticeKind,
}

#[derive(Clone, Debug, PartialEq)]
pub enum NoticeKind {
    Info,
    Error,
    /// The file changed on disk while the page has unsaved edits.
    External {
        /// Disk content, for display only; `accept_disk` always re-reads the file.
        content: String,
    },
    /// A save found the file changed on disk; the local text is in a conflict copy.
    Conflict {
        /// Current disk content, or `None` when the file was deleted externally.
        disk: Option<String>,
        /// Vault-relative path of the conflict copy.
        copy: String,
    },
}

/// What the UI shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum View {
    Journals,
    Page(String),
}

/// The block being edited in source mode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    pub rel: String,
    pub id: BlockId,
}

/// One visible block in the flat list, with everything the row needs to draw.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub id: BlockId,
    pub depth: usize,
    pub text: String,
    /// `text` rendered as Markdown, `((uuid))` already resolved.
    pub markdown: String,
    /// Hash of the source text and the resolved reference text: a row is
    /// rebuilt only when this changes.
    pub rev: u64,
    pub task: Option<Marker>,
    pub collapsed: bool,
    pub has_children: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct PageView {
    pub rel: String,
    pub title: String,
    pub r#gen: u64,
    pub read_only: bool,
    pub stale: bool,
    /// Latest notice for this page, so a re-fetched view keeps its banner.
    pub notice: Option<Notice>,
    pub rows: Vec<Row>,
}

/// Where the editor should go after a structure op.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Focus {
    pub id: BlockId,
    /// A split puts the caret at the start of the new block.
    pub caret_start: bool,
    /// An absolute caret for the rebuilt editor (UTF-16 units), for ops that
    /// rewrite the block's text in place.
    pub caret: Option<usize>,
}

/// Result of a structure op: the model changed and the page was saved (or
/// queued for saving) inside the same call.
#[derive(Clone, Debug, PartialEq)]
pub struct OpResult {
    pub focus: Option<Focus>,
    pub page: PageView,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JournalEntry {
    pub rel: String,
    pub title: String,
    pub date: Option<JournalDate>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct JournalList {
    pub entries: Vec<JournalEntry>,
    pub more: bool,
}

/// `leave` either quits the app or switches to another vault.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaveVerb {
    Quit,
    Switch,
}

impl LeaveVerb {
    fn word(self) -> &'static str {
        match self {
            LeaveVerb::Quit => "退出",
            LeaveVerb::Switch => "切换",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LeaveResult {
    pub ok: bool,
    /// Why leaving was refused.
    pub notice: Option<String>,
}

#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("page not loaded: {0}")]
    NotLoaded(String),
    #[error("stale r#gen for {rel}: the page was replaced from disk")]
    StaleGen { rel: String },
    #[error("page is read-only: {0}")]
    ReadOnly(String),
    #[error("no block {id:?} in {rel}")]
    NoBlock { rel: String, id: BlockId },
    #[error("an edit is still open")]
    Editing,
    #[error("invalid text range [{start}, {end})")]
    InvalidRange { start: usize, end: usize },
    #[error("the block text changed since the completion opened")]
    StaleText,
    #[error("nothing to undo or redo for {0}")]
    NoHistory(String),
    #[error(transparent)]
    Vault(#[from] VaultError),
}

#[derive(Debug, thiserror::Error)]
pub enum OpenError {
    #[error("boot data is not installed yet")]
    NoBoot,
    #[error("refused: {0}")]
    Refused(String),
    #[error(transparent)]
    Vault(#[from] VaultError),
    #[error("open task failed: {0}")]
    Task(String),
}

/// Restarts the process into another vault.
pub type RelaunchHook = Arc<dyn Fn(&Path) -> io::Result<()> + Send + Sync>;

/// Process-level effects `leave` cannot perform itself.
#[derive(Clone)]
pub struct Hooks {
    pub relaunch: RelaunchHook,
    pub close_window: Arc<dyn Fn() + Send + Sync>,
}

impl Hooks {
    pub fn new(relaunch: RelaunchHook, close_window: Arc<dyn Fn() + Send + Sync>) -> Self {
        Self {
            relaunch,
            close_window,
        }
    }
}

impl Default for Hooks {
    fn default() -> Self {
        Self::new(Arc::new(|_| Ok(())), Arc::new(|| {}))
    }
}

/// Process-level hand-over from `main` to whatever needs the vault: cloned,
/// never taken, so `boot()` works after hot reloads and full reloads too.
#[derive(Clone)]
pub struct Boot {
    pub vault: Option<VaultHandle>,
    /// Shared so a rebuilt pump can re-attach to the same watcher stream.
    pub events: Option<Arc<tokio::sync::Mutex<UnboundedReceiver<VaultEvent>>>>,
    pub today: JournalDate,
    pub handle: Handle,
    pub view: View,
    pub notice: Option<String>,
    pub recent: Vec<String>,
    pub relaunch: RelaunchHook,
    /// Recent-vaults state file; `open` pushes the opened vault here.
    pub state_file: Option<PathBuf>,
}

static BOOT: StdMutex<Option<Boot>> = StdMutex::new(None);

/// Installs the boot data; called once by the host at start-up and again by
/// [`open`] when a vault is opened in place.
pub fn set_boot(boot: Boot) {
    *BOOT.lock().unwrap_or_else(|e| e.into_inner()) = Some(boot);
}

/// A clone of the boot data. Repeatable by design.
pub fn boot() -> Option<Boot> {
    BOOT.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// [`boot`], or, when the host installed nothing, a boot for the vault named by
/// `RIKE_DEV_VAULT`. That lets `nsh dev` and `nsh test` run the UI without the Rust host.
/// The vault runs on a runtime of its own that lives for the rest of the process.
pub fn boot_or_dev() -> Option<Boot> {
    if let Some(boot) = boot() {
        return Some(boot);
    }
    let dir = dev_root()?;
    dev_boot(&dir)
}

/// `RIKE_DEV_VAULT`, when set.
pub fn dev_root() -> Option<PathBuf> {
    std::env::var_os("RIKE_DEV_VAULT")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

/// Refuses the dev hooks when a host booted a vault outside the dev root.
pub fn dev_guard(base: &Path) -> Result<(), String> {
    match boot().and_then(|b| b.vault) {
        Some(current) if !current.root().starts_with(base) => {
            Err("a host booted a vault".to_owned())
        }
        _ => Ok(()),
    }
}

fn dev_runtime() -> Option<&'static tokio::runtime::Runtime> {
    static RUNTIME: std::sync::OnceLock<Option<tokio::runtime::Runtime>> =
        std::sync::OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .ok()
        })
        .as_ref()
}

fn dev_boot(dir: &Path) -> Option<Boot> {
    let runtime = dev_runtime()?;
    let (vault, events) = match runtime.block_on(Vault::open(dir.to_path_buf())) {
        Ok(opened) => opened,
        Err(e) => {
            log::error!("cannot open the dev vault: {e}");
            return None;
        }
    };
    set_boot(Boot {
        vault: Some(vault),
        events: Some(Arc::new(tokio::sync::Mutex::new(events))),
        today: dev_today(),
        handle: runtime.handle().clone(),
        view: View::Journals,
        notice: None,
        recent: Vec::new(),
        relaunch: Arc::new(|_| Ok(())),
        state_file: None,
    });
    boot()
}

/// Test hook: boots a fresh, empty vault (a new directory under `RIKE_DEV_VAULT`)
/// holding `files`, so every UI test starts from its own notes. Only for dev runs:
/// it refuses when the host booted a vault of its own.
pub fn dev_reset(seed: Option<&str>, files: &[(String, String)]) -> Result<PathBuf, String> {
    static SEQ: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let base = dev_root().ok_or("not a dev run")?;
    dev_guard(&base)?;
    let dir = base.join(format!(
        "t{}",
        SEQ.fetch_add(1, std::sync::atomic::Ordering::SeqCst)
    ));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    if let Some(seed) = seed {
        let seeds = std::env::var_os("RIKE_DEV_SEED").ok_or("RIKE_DEV_SEED is not set")?;
        copy_tree(&dev_join(Path::new(&seeds), seed)?, &dir).map_err(|e| e.to_string())?;
    }
    for (rel, text) in files {
        let file = dev_join(&dir, rel)?;
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
        std::fs::write(file, text).map_err(|e| e.to_string())?;
    }
    dev_boot(&dir).ok_or("cannot open the dev vault")?;
    Ok(dir)
}

fn copy_tree(from: &Path, to: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_tree(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// Test hook: boots with no vault (the welcome page), listing `recent`.
pub fn dev_welcome(recent: Vec<String>, notice: Option<String>) -> Result<(), String> {
    let base = dev_root().ok_or("not a dev run")?;
    dev_guard(&base)?;
    let runtime = dev_runtime().ok_or("no runtime")?;
    set_boot(Boot {
        vault: None,
        events: None,
        today: dev_today(),
        handle: runtime.handle().clone(),
        view: View::Journals,
        notice,
        recent,
        relaunch: Arc::new(|_| Ok(())),
        state_file: None,
    });
    Ok(())
}

/// Test hook: the view the next start restores.
pub fn dev_view(view: View) {
    if let Some(mut boot) = boot() {
        boot.view = view;
        set_boot(boot);
    }
}

/// `rel` inside `dir`, refusing anything but plain relative components.
pub fn dev_join(dir: &Path, rel: &str) -> Result<PathBuf, String> {
    let plain = Path::new(rel)
        .components()
        .all(|c| matches!(c, std::path::Component::Normal(_)));
    if plain {
        Ok(dir.join(rel))
    } else {
        Err(format!("bad path {rel}"))
    }
}

/// `RIKE_DEV_TODAY=yyyy-mm-dd` pins the date for UI tests; otherwise the local date.
fn dev_today() -> rike_core::journal::JournalDate {
    let pinned = std::env::var("RIKE_DEV_TODAY").ok().and_then(|text| {
        let mut parts = text.split('-').map(|p| p.parse::<i32>().ok());
        let (y, m, d) = (parts.next()??, parts.next()??, parts.next()??);
        rike_core::journal::JournalDate::new(y, u8::try_from(m).ok()?, u8::try_from(d).ok()?)
    });
    pinned.unwrap_or_else(crate::today::local_today)
}

/// What the switch-vault flow needs to know about `path` before opening it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inspect {
    pub canonical: Option<PathBuf>,
    /// Valid only when `reason` is `None`.
    pub kind: startup::VaultKind,
    /// `Some` when the path must not be opened.
    pub reason: Option<String>,
    pub is_current: bool,
}

/// Polls `future` with `handle`'s runtime entered, so tokio timers work even though the NSH
/// engine thread polls it. The guard lives only inside one poll: guards held across `.await`
/// points of tasks that interleave on one thread would be dropped out of order.
fn in_runtime<F: Future>(handle: Handle, future: F) -> impl Future<Output = F::Output> {
    struct InRuntime<F> {
        handle: Handle,
        future: Pin<Box<F>>,
    }
    impl<F: Future> Future for InRuntime<F> {
        type Output = F::Output;
        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<F::Output> {
            let _entered = self.handle.enter();
            self.future.as_mut().poll(cx)
        }
    }
    InRuntime {
        handle,
        future: Box::pin(future),
    }
}

/// One completion candidate: what the panel shows and what to insert
/// after the trigger (`[[` or `((`).
pub struct CompleteItem {
    pub kind: String,
    /// Text to insert in place of the query, closing the reference.
    pub insert: String,
    pub label: String,
    /// The source page's title for a block candidate, else empty.
    pub detail: String,
}

/// Messages from the session to its pump.
enum PumpMsg {
    SaveNow(String),
    SaveDebounced(String),
    CancelSaves,
    IndexDirty,
    Shutdown,
}

/// Async chores [`Session::on_vault_event`] cannot do synchronously; the pump
/// runs them right after.
enum FollowUp {
    SetReadOnly { rel: String, read_only: bool },
    IndexFromDisk { rel: String },
    Rescan,
}

/// An opened page and its generation token.
struct PageState {
    doc: Doc,
    /// Bumped only when the document is replaced by disk content (load, clean
    /// reload, `accept_disk`) — never on Conflict/External detection or
    /// `keep_mine`.
    r#gen: u64,
}

struct Inner {
    vault: VaultHandle,
    events: Arc<tokio::sync::Mutex<UnboundedReceiver<VaultEvent>>>,
    handle: Handle,
    hooks: Hooks,
    today: JournalDate,
    config: VaultConfig,
    index: MemIndex,
    docs: HashMap<String, PageState>,
    editing: Option<Edit>,
    undo: Vec<(String, Page)>,
    redo: Vec<(String, Page)>,
    notices: Vec<Notice>,
    next_notice: u64,
    quitting: bool,
    /// Once `leave` starts the pump stops scheduling new saves.
    leaving: bool,
    index_ver: u64,
    index_dirty: bool,
    sink: Option<EventSink>,
    /// Events coalesced while no sink is registered; replayed on registration.
    backlog: Vec<Event>,
    pump_tx: UnboundedSender<PumpMsg>,
    pump_rx: Option<UnboundedReceiver<PumpMsg>>,
}

/// An open vault session. Clone it freely; clones share the state.
#[derive(Clone)]
pub struct Session {
    inner: Rc<RefCell<Inner>>,
}

impl Session {
    pub fn new(
        vault: VaultHandle,
        events: Arc<tokio::sync::Mutex<UnboundedReceiver<VaultEvent>>>,
        today: JournalDate,
        handle: Handle,
        hooks: Hooks,
    ) -> Session {
        let (pump_tx, pump_rx) = tokio::sync::mpsc::unbounded_channel();
        Session {
            inner: Rc::new(RefCell::new(Inner {
                vault,
                events,
                handle,
                hooks,
                today,
                config: VaultConfig::default(),
                index: MemIndex::new(),
                docs: HashMap::new(),
                editing: None,
                undo: Vec::new(),
                redo: Vec::new(),
                notices: Vec::new(),
                next_notice: 1,
                quitting: false,
                leaving: false,
                index_ver: 0,
                index_dirty: false,
                sink: None,
                backlog: Vec::new(),
                pump_tx,
                pump_rx: Some(pump_rx),
            })),
        }
    }

    /// Builds a session from the boot data; `None` when no vault is open
    /// (the welcome page).
    pub fn from_boot(boot: &Boot, hooks: Hooks) -> Option<Session> {
        Some(Session::new(
            boot.vault.clone()?,
            boot.events.clone()?,
            boot.today,
            boot.handle.clone(),
            hooks,
        ))
    }

    /// Registers the event sink. Events that arrived while no sink was
    /// registered are replayed (coalesced) to the new one.
    pub fn set_event_sink(&self, sink: Option<EventSink>) {
        let backlog = {
            let mut i = self.inner.borrow_mut();
            i.sink = sink;
            if i.sink.is_some() {
                std::mem::take(&mut i.backlog)
            } else {
                Vec::new()
            }
        };
        let sink = self.inner.borrow().sink.clone();
        if let Some(sink) = sink {
            for event in backlog {
                sink(event);
            }
        }
    }

    /// Reads the vault config and builds the index. Awaited once after the
    /// pump is running.
    pub async fn start(&self) {
        match self.vault().read_config_edn().await {
            Ok(Some(edn)) => {
                let config = parse_config_edn(&edn);
                for warning in config.warnings() {
                    self.notify("", warning, NoticeKind::Info);
                }
                self.inner.borrow_mut().config = config;
            }
            Ok(None) => {}
            Err(e) => self.notify("", format!("读取 config.edn 失败：{e}"), NoticeKind::Error),
        }
        self.scan().await;
    }

    /// Stops the pump; the host calls this before dropping a session it
    /// replaces (an in-place `open`).
    pub fn shutdown(&self) {
        self.send(PumpMsg::Shutdown);
    }

    /// The vault root; tells the sessions of two vaults apart.
    pub fn root(&self) -> PathBuf {
        self.vault().root().to_path_buf()
    }

    /// The resident task: merges watcher events, save requests and the save
    /// debounce. One per session; the host drives it (engine-thread `spawn`,
    /// or `LocalSet` in tests). Ends when the vault event stream closes or
    /// [`shutdown`](Self::shutdown) is called.
    pub async fn pump(self) {
        let (rx, events, handle) = {
            let mut i = self.inner.borrow_mut();
            let rx = i.pump_rx.take().expect("one pump per session");
            (rx, i.events.clone(), i.handle.clone())
        };
        in_runtime(handle, self.run_pump(rx, events)).await;
    }

    async fn run_pump(
        self,
        mut rx: UnboundedReceiver<PumpMsg>,
        events: Arc<tokio::sync::Mutex<UnboundedReceiver<VaultEvent>>>,
    ) {
        let mut events = events.lock().await;
        let mut deadlines: HashMap<String, Instant> = HashMap::new();
        let mut save_timer: Pin<Box<Sleep>> = Box::pin(tokio::time::sleep(NEVER));
        let mut index_timer: Option<Pin<Box<Sleep>>> = None;

        loop {
            tokio::select! {
                event = events.recv() => {
                    let Some(event) = event else { break };
                    for follow_up in self.on_vault_event(event) {
                        match follow_up {
                            FollowUp::SetReadOnly { rel, read_only } => {
                                let _ = self.vault().set_read_only(&rel, read_only).await;
                            }
                            FollowUp::IndexFromDisk { rel } => self.index_from_disk(&rel, true).await,
                            FollowUp::Rescan => self.scan().await,
                        }
                    }
                }
                msg = rx.recv() => {
                    let Some(msg) = msg else { break };
                    match msg {
                        PumpMsg::SaveNow(rel) => {
                            deadlines.remove(&rel);
                            if !self.is_leaving() {
                                self.save_now(&rel).await;
                            }
                        }
                        PumpMsg::SaveDebounced(rel) => {
                            if !self.is_leaving() {
                                deadlines.insert(rel, Instant::now() + SAVE_DELAY);
                            }
                        }
                        PumpMsg::CancelSaves => deadlines.clear(),
                        PumpMsg::Shutdown => break,
                        PumpMsg::IndexDirty => {
                            if index_timer.is_none() {
                                index_timer = Some(Box::pin(tokio::time::sleep(INDEX_DELAY)));
                            }
                        }
                    }
                    rearm_save_timer(&mut save_timer, &deadlines);
                }
                () = &mut save_timer => {
                    let now = Instant::now();
                    let due: Vec<String> = deadlines
                        .iter()
                        .filter(|(_, at)| **at <= now)
                        .map(|(rel, _)| rel.clone())
                        .collect();
                    for rel in &due {
                        deadlines.remove(rel);
                    }
                    rearm_save_timer(&mut save_timer, &deadlines);
                    for rel in due {
                        if !self.is_leaving() {
                            self.save_now(&rel).await;
                        }
                    }
                }
                () = async {
                    match &mut index_timer {
                        Some(timer) => timer.await,
                        None => std::future::pending().await,
                    }
                } => {
                    index_timer = None;
                    self.emit_index_ver();
                }
            }
        }
    }

    // ---- events -----------------------------------------------------------

    fn emit(&self, event: Event) {
        let mut i = self.inner.borrow_mut();
        if let Some(sink) = i.sink.clone() {
            drop(i);
            sink(event);
        } else {
            coalesce(&mut i.backlog, event);
        }
    }

    fn send(&self, msg: PumpMsg) {
        let _ = self.inner.borrow().pump_tx.send(msg);
    }

    fn mark_index_dirty(&self) {
        self.inner.borrow_mut().index_dirty = true;
        self.send(PumpMsg::IndexDirty);
    }

    fn emit_index_ver(&self) {
        let event = {
            let mut i = self.inner.borrow_mut();
            if !i.index_dirty {
                return;
            }
            i.index_dirty = false;
            i.index_ver += 1;
            Event::IndexVer(i.index_ver)
        };
        self.emit(event);
    }

    /// Pushes a notice, replacing an unanswered one of the same page and kind.
    pub fn notify(&self, rel: &str, message: impl Into<String>, kind: NoticeKind) {
        let notice = {
            let mut i = self.inner.borrow_mut();
            let id = i.next_notice;
            i.next_notice += 1;
            let notice = Notice {
                id,
                rel: rel.to_owned(),
                message: message.into(),
                kind,
            };
            i.notices.retain(|n| {
                n.rel != notice.rel
                    || std::mem::discriminant(&n.kind) != std::mem::discriminant(&notice.kind)
            });
            i.notices.push(notice.clone());
            notice
        };
        self.emit(Event::Notice(notice));
    }

    /// Test hook: marks a loaded page read-only, as the losslessness
    /// self-check would. The self-check is nearly impossible to trigger with
    /// real files, so integration tests fake its outcome this way.
    #[doc(hidden)]
    pub fn force_read_only(&self, rel: &str) {
        if let Some(ps) = self.inner.borrow_mut().docs.get_mut(rel) {
            ps.doc.read_only = true;
        }
    }

    pub fn dismiss_notice(&self, id: u64) {
        self.inner.borrow_mut().notices.retain(|n| n.id != id);
    }

    // ---- documents --------------------------------------------------------

    /// Loads a page (from disk, or virtual when the file does not exist yet)
    /// and returns its view. Loading sets `gen` to 1.
    pub async fn page(&self, rel: &str) -> Result<PageView, SessionError> {
        if let Some(view) = self.page_view(rel) {
            return Ok(view);
        }
        let vault = self.vault();
        match vault.load(rel).await {
            Ok(loaded) => {
                // A reload that landed while this load was in flight is newer.
                if let Some(view) = self.page_view(rel) {
                    return Ok(view);
                }
                let parsed = parse_page(&loaded.content);
                let read_only = !parsed.lossless;
                let notice = {
                    let mut i = self.inner.borrow_mut();
                    let (title, journal) = resolve_title(rel, &parsed.page, &i.config);
                    let notice =
                        read_only.then(|| format!("{title} 含有无法无损往返的内容，已设为只读"));
                    index_parsed(&mut i, rel, &parsed.page);
                    i.docs.insert(
                        rel.to_owned(),
                        PageState {
                            doc: Doc::new(
                                rel.to_owned(),
                                title,
                                journal,
                                parsed.page,
                                true,
                                read_only,
                            ),
                            r#gen: 1,
                        },
                    );
                    notice
                };
                let _ = vault.set_read_only(rel, read_only).await;
                self.mark_index_dirty();
                if let Some(message) = notice {
                    self.notify(rel, message, NoticeKind::Info);
                }
                self.page_view(rel)
                    .ok_or_else(|| SessionError::NotLoaded(rel.to_owned()))
            }
            Err(VaultError::NotFound(_)) => {
                // A page without a file: virtual until the first non-empty save.
                let mut i = self.inner.borrow_mut();
                let page = Page::default();
                let (title, journal) = resolve_title(rel, &page, &i.config);
                i.docs.insert(
                    rel.to_owned(),
                    PageState {
                        doc: Doc::new(rel.to_owned(), title, journal, page, false, false),
                        r#gen: 1,
                    },
                );
                drop(i);
                self.page_view(rel)
                    .ok_or_else(|| SessionError::NotLoaded(rel.to_owned()))
            }
            Err(e) => {
                self.notify(rel, format!("无法打开 {rel}：{e}"), NoticeKind::Error);
                Err(SessionError::Vault(e))
            }
        }
    }

    /// The view of an already loaded page; what the TS side re-fetches on
    /// `IndexVer` and `PageChanged`.
    pub fn page_view(&self, rel: &str) -> Option<PageView> {
        let i = self.inner.borrow();
        let ps = i.docs.get(rel)?;
        let notice = i.notices.iter().rev().find(|n| n.rel == rel).cloned();
        let rows = ps
            .doc
            .rows
            .iter()
            .map(|row| {
                let markdown = md::render(&row.text, &|id| i.index.block(id).map(|b| b.text));
                Row {
                    id: row.id,
                    depth: row.depth,
                    text: row.text.clone(),
                    rev: row_rev(&row.text, &markdown),
                    markdown,
                    task: md::task_of(&row.text),
                    collapsed: row.collapsed,
                    has_children: row.has_children,
                }
            })
            .collect();
        Some(PageView {
            rel: rel.to_owned(),
            title: ps.doc.title.clone(),
            r#gen: ps.r#gen,
            read_only: ps.doc.read_only,
            stale: ps.doc.stale,
            notice,
            rows,
        })
    }

    pub fn editing(&self) -> Option<Edit> {
        self.inner.borrow().editing.clone()
    }

    /// Registers the block being edited. External changes to a page that is
    /// dirty or being edited become an External notice instead of a reload.
    pub fn set_editing(&self, rel: &str, r#gen: u64, id: BlockId) -> Result<(), SessionError> {
        {
            let mut i = self.inner.borrow_mut();
            let ps = i
                .docs
                .get(rel)
                .ok_or_else(|| SessionError::NotLoaded(rel.to_owned()))?;
            if ps.r#gen != r#gen {
                return Err(SessionError::StaleGen {
                    rel: rel.to_owned(),
                });
            }
            if ps.doc.read_only {
                return Err(SessionError::ReadOnly(rel.to_owned()));
            }
            if ps.doc.page.find(id).is_none() {
                return Err(SessionError::NoBlock {
                    rel: rel.to_owned(),
                    id,
                });
            }
            i.editing = Some(Edit {
                rel: rel.to_owned(),
                id,
            });
        }
        Ok(())
    }

    pub fn clear_editing(&self) {
        self.inner.borrow_mut().editing = None;
    }

    /// Moves a draft into the model and queues a debounced save. `false` when
    /// the gen is stale, the page is read-only, or the block is gone.
    pub fn set_draft(&self, rel: &str, r#gen: u64, id: BlockId, text: &str) -> bool {
        enum Draft {
            Applied,
            Unchanged,
            Rejected,
        }
        let outcome = {
            let mut i = self.inner.borrow_mut();
            let Some(ps) = i.docs.get_mut(rel) else {
                return false;
            };
            if ps.r#gen != r#gen || ps.doc.read_only || ps.doc.page.find(id).is_none() {
                Draft::Rejected
            } else if ops::set_text(&mut ps.doc.page, id, text.to_owned()) {
                ps.doc.refresh();
                ps.doc.dirty = true;
                Draft::Applied
            } else {
                Draft::Unchanged
            }
        };
        match outcome {
            Draft::Rejected => false,
            Draft::Unchanged => true,
            Draft::Applied => {
                {
                    let mut i = self.inner.borrow_mut();
                    index_page(&mut i, rel);
                }
                self.send(PumpMsg::SaveDebounced(rel.to_owned()));
                self.mark_index_dirty();
                true
            }
        }
    }

    // ---- structure ops ----------------------------------------------------

    /// Enter: splits the block at `caret` (UTF-16 units into `text`). On an
    /// empty indented block this outdents instead.
    pub fn split(
        &self,
        rel: &str,
        r#gen: u64,
        id: BlockId,
        text: String,
        caret: Option<u32>,
    ) -> Result<OpResult, SessionError> {
        let outdent_instead = {
            let i = self.inner.borrow();
            text.trim().is_empty()
                && i.docs
                    .get(rel)
                    .and_then(|ps| ps.doc.row(id))
                    .is_some_and(|row| row.depth > 0)
        };
        if outdent_instead {
            return self.structure_op(rel, r#gen, id, None, |page, id| {
                let changed = ops::outdent(page, id);
                if changed {
                    reveal(page, id);
                }
                (
                    changed,
                    changed.then_some(Focus {
                        id,
                        caret_start: false,
                        caret: None,
                    }),
                )
            });
        }
        let at = utf16_to_byte(&text, caret.unwrap_or(0) as usize);
        self.structure_op(
            rel,
            r#gen,
            id,
            Some(text),
            |page, id| match ops::split_block(page, id, at) {
                Some(new_id) => (
                    true,
                    Some(Focus {
                        id: new_id,
                        caret_start: true,
                        caret: None,
                    }),
                ),
                None => (false, None),
            },
        )
    }

    pub fn indent(
        &self,
        rel: &str,
        r#gen: u64,
        id: BlockId,
        text: Option<String>,
    ) -> Result<OpResult, SessionError> {
        self.move_like(rel, r#gen, id, text, ops::indent)
    }

    pub fn outdent(
        &self,
        rel: &str,
        r#gen: u64,
        id: BlockId,
        text: Option<String>,
    ) -> Result<OpResult, SessionError> {
        self.move_like(rel, r#gen, id, text, ops::outdent)
    }

    pub fn move_up(
        &self,
        rel: &str,
        r#gen: u64,
        id: BlockId,
        text: Option<String>,
    ) -> Result<OpResult, SessionError> {
        self.move_like(rel, r#gen, id, text, ops::move_up)
    }

    pub fn move_down(
        &self,
        rel: &str,
        r#gen: u64,
        id: BlockId,
        text: Option<String>,
    ) -> Result<OpResult, SessionError> {
        self.move_like(rel, r#gen, id, text, ops::move_down)
    }

    fn move_like(
        &self,
        rel: &str,
        r#gen: u64,
        id: BlockId,
        text: Option<String>,
        op: fn(&mut Page, BlockId) -> bool,
    ) -> Result<OpResult, SessionError> {
        self.structure_op(rel, r#gen, id, text, |page, id| {
            let changed = op(page, id);
            if changed {
                reveal(page, id);
            }
            (
                changed,
                changed.then_some(Focus {
                    id,
                    caret_start: false,
                    caret: None,
                }),
            )
        })
    }

    /// Backspace in an empty block. Blocks with properties (a referenced
    /// `id::`, say) are content and are kept.
    pub fn remove(
        &self,
        rel: &str,
        r#gen: u64,
        id: BlockId,
        text: Option<String>,
    ) -> Result<OpResult, SessionError> {
        self.structure_op(rel, r#gen, id, text, |page, id| {
            if page.find(id).is_none_or(|b| !b.properties.is_empty()) {
                return (false, None);
            }
            match ops::delete_block(page, id) {
                Some(focus) => (
                    true,
                    Some(Focus {
                        id: focus,
                        caret_start: false,
                        caret: None,
                    }),
                ),
                None => (false, None),
            }
        })
    }

    pub fn toggle_collapse(
        &self,
        rel: &str,
        r#gen: u64,
        id: BlockId,
    ) -> Result<OpResult, SessionError> {
        self.structure_op(rel, r#gen, id, None, |page, id| {
            let collapsed = page.find(id).is_some_and(Block::is_collapsed);
            (ops::set_collapsed(page, id, !collapsed), None)
        })
    }

    pub fn toggle_todo(
        &self,
        rel: &str,
        r#gen: u64,
        id: BlockId,
        text: Option<String>,
    ) -> Result<OpResult, SessionError> {
        self.structure_op(rel, r#gen, id, text, |page, id| {
            let changed = ops::toggle_todo(page, id);
            if changed {
                reveal(page, id);
            }
            (
                changed,
                changed.then_some(Focus {
                    id,
                    caret_start: false,
                    caret: None,
                }),
            )
        })
    }

    /// A press on a task checkbox; the page stays in rendered mode.
    pub fn set_task_done(
        &self,
        rel: &str,
        r#gen: u64,
        id: BlockId,
        done: bool,
    ) -> Result<OpResult, SessionError> {
        self.structure_op(rel, r#gen, id, None, |page, id| {
            (ops::set_task_done(page, id, done), None)
        })
    }

    /// Replaces the UTF-16 range `[start, end)` of a block's text with
    /// `insert` (a completion acceptance, say). `expected` must be what the
    /// range currently holds: the panel's offsets were taken from an older
    /// snapshot, and a page that changed underneath must not be spliced.
    /// The editor is rebuilt with the caret after the inserted text.
    // The (start, end, expected, insert) shape mirrors the wire format; a
    // struct would move the noise, not remove it.
    #[allow(clippy::too_many_arguments)]
    pub fn replace_range(
        &self,
        rel: &str,
        r#gen: u64,
        id: BlockId,
        start: usize,
        end: usize,
        expected: &str,
        insert: &str,
    ) -> Result<OpResult, SessionError> {
        if start > end {
            return Err(SessionError::InvalidRange { start, end });
        }
        let text = self
            .inner
            .borrow()
            .docs
            .get(rel)
            .and_then(|ps| ps.doc.page.find(id))
            .map(|b| b.text.clone())
            .ok_or_else(|| SessionError::NoBlock {
                rel: rel.to_owned(),
                id,
            })?;
        let b_start = utf16_to_byte(&text, start);
        let b_end = utf16_to_byte(&text, end);
        if text.get(b_start..b_end) != Some(expected) {
            return Err(SessionError::StaleText);
        }
        let new_text = format!("{}{insert}{}", &text[..b_start], &text[b_end..]);
        let caret = start.saturating_add(insert.encode_utf16().count());
        self.structure_op(rel, r#gen, id, Some(new_text), move |_, id| {
            (
                false,
                Some(Focus {
                    id,
                    caret_start: false,
                    caret: Some(caret),
                }),
            )
        })
    }

    /// The shared core of every structure op: checks, optional draft text,
    /// undo snapshot, save request and the fresh view, all in one call.
    fn structure_op(
        &self,
        rel: &str,
        r#gen: u64,
        id: BlockId,
        text: Option<String>,
        op: impl FnOnce(&mut Page, BlockId) -> (bool, Option<Focus>),
    ) -> Result<OpResult, SessionError> {
        enum Save {
            Now,
            Debounced,
            No,
        }
        let (save, focus) = {
            let mut i = self.inner.borrow_mut();
            let (changed, text_changed, focus, before) = {
                let ps = i
                    .docs
                    .get_mut(rel)
                    .ok_or_else(|| SessionError::NotLoaded(rel.to_owned()))?;
                if ps.r#gen != r#gen {
                    return Err(SessionError::StaleGen {
                        rel: rel.to_owned(),
                    });
                }
                if ps.doc.read_only {
                    return Err(SessionError::ReadOnly(rel.to_owned()));
                }
                let text_changed = match text {
                    Some(text) => ops::set_text(&mut ps.doc.page, id, text),
                    None => false,
                };
                // The snapshot keeps the typed text but predates the op.
                let before = ps.doc.page.clone();
                let (changed, focus) = op(&mut ps.doc.page, id);
                if changed || text_changed {
                    ps.doc.refresh();
                    ps.doc.dirty = true;
                }
                (changed, text_changed, focus, before)
            };
            let save = if changed {
                push_undo(&mut i, rel, before);
                index_page(&mut i, rel);
                Save::Now
            } else if text_changed {
                index_page(&mut i, rel);
                Save::Debounced
            } else {
                Save::No
            };
            (save, focus)
        };
        match save {
            Save::Now => self.send(PumpMsg::SaveNow(rel.to_owned())),
            Save::Debounced => self.send(PumpMsg::SaveDebounced(rel.to_owned())),
            Save::No => {}
        }
        if !matches!(save, Save::No) {
            self.mark_index_dirty();
        }
        let page = self
            .page_view(rel)
            .ok_or_else(|| SessionError::NotLoaded(rel.to_owned()))?;
        Ok(OpResult { focus, page })
    }

    // ---- undo -------------------------------------------------------------

    pub fn undo(&self, rel: &str, r#gen: u64) -> Result<OpResult, SessionError> {
        self.step_history(rel, r#gen, true)
    }

    pub fn redo(&self, rel: &str, r#gen: u64) -> Result<OpResult, SessionError> {
        self.step_history(rel, r#gen, false)
    }

    /// Structural undo. While an input has the focus, Ctrl+Z belongs to it.
    fn step_history(&self, rel: &str, r#gen: u64, undo: bool) -> Result<OpResult, SessionError> {
        {
            let mut i = self.inner.borrow_mut();
            if i.editing.is_some() {
                return Err(SessionError::Editing);
            }
            {
                let ps = i
                    .docs
                    .get(rel)
                    .ok_or_else(|| SessionError::NotLoaded(rel.to_owned()))?;
                if ps.r#gen != r#gen {
                    return Err(SessionError::StaleGen {
                        rel: rel.to_owned(),
                    });
                }
            }
            let entry = if undo { i.undo.pop() } else { i.redo.pop() };
            let Some((entry_rel, page)) = entry else {
                return Err(SessionError::NoHistory(rel.to_owned()));
            };
            if entry_rel != rel {
                // The newest snapshot belongs to another page; leave it be.
                if undo {
                    i.undo.push((entry_rel, page));
                } else {
                    i.redo.push((entry_rel, page));
                }
                return Err(SessionError::NoHistory(rel.to_owned()));
            }
            let previous = {
                let ps = i.docs.get_mut(rel).expect("r#gen checked above");
                let previous = std::mem::replace(&mut ps.doc.page, page);
                ps.doc.refresh();
                ps.doc.dirty = true;
                previous
            };
            if undo {
                i.redo.push((rel.to_owned(), previous));
            } else {
                i.undo.push((rel.to_owned(), previous));
            }
            index_page(&mut i, rel);
        }
        self.send(PumpMsg::SaveDebounced(rel.to_owned()));
        self.mark_index_dirty();
        let page = self
            .page_view(rel)
            .ok_or_else(|| SessionError::NotLoaded(rel.to_owned()))?;
        Ok(OpResult { focus: None, page })
    }

    // ---- conflicts --------------------------------------------------------

    /// "Keep mine": saves the in-memory page over the file. Never bumps `gen`.
    pub fn keep_mine(&self, rel: &str) -> bool {
        {
            let mut i = self.inner.borrow_mut();
            resolve_notices(&mut i, rel);
            let Some(ps) = i.docs.get_mut(rel) else {
                return false;
            };
            ps.doc.dirty = true;
            ps.doc.stale = false;
        }
        self.send(PumpMsg::SaveNow(rel.to_owned()));
        true
    }

    /// "Load the version on disk": re-reads the file (never the cached notice
    /// content), clears the page's editing state and bumps `gen`.
    pub async fn accept_disk(&self, rel: &str) -> Result<PageView, SessionError> {
        {
            let mut i = self.inner.borrow_mut();
            resolve_notices(&mut i, rel);
        }
        let vault = self.vault();
        match vault.load(rel).await {
            Ok(loaded) => {
                let read_only = self.reload(rel, &loaded.content);
                let _ = vault.set_read_only(rel, read_only).await;
                self.page_view(rel)
                    .ok_or_else(|| SessionError::NotLoaded(rel.to_owned()))
            }
            Err(VaultError::NotFound(_)) => {
                self.notify(rel, "磁盘上的文件已被删除", NoticeKind::Info);
                Err(SessionError::Vault(VaultError::NotFound(rel.to_owned())))
            }
            Err(e) => {
                self.notify(rel, format!("无法读取 {rel}：{e}"), NoticeKind::Error);
                Err(SessionError::Vault(e))
            }
        }
    }

    /// Writes the page of `rel`. True when the file is as the page is, or
    /// there is nothing to write (a read-only page, an empty new one); false
    /// when the save did not happen or did not go through.
    pub async fn save_now(&self, rel: &str) -> bool {
        let snapshot = {
            let i = self.inner.borrow();
            i.docs.get(rel).map(|ps| {
                (
                    serialize_page(&ps.doc.page),
                    ps.doc.exists,
                    ps.doc.has_content(),
                    ps.doc.read_only,
                    ps.doc.stale,
                )
            })
        };
        let Some((content, exists, has_content, read_only, stale)) = snapshot else {
            return true;
        };
        // A page waiting for the user to pick a side must not be written.
        if stale {
            return false;
        }
        if read_only || (!exists && !has_content) {
            return true;
        }
        {
            let mut i = self.inner.borrow_mut();
            index_page(&mut i, rel);
        }
        self.mark_index_dirty();
        match self.vault().save(rel, content.clone()).await {
            Ok(SaveOutcome::Saved { .. }) => {
                let mut i = self.inner.borrow_mut();
                if let Some(ps) = i.docs.get_mut(rel) {
                    let current = serialize_page(&ps.doc.page);
                    if !ps.doc.exists || current == content {
                        ps.doc.exists = true;
                        if current == content {
                            ps.doc.dirty = false;
                        }
                    }
                }
                true
            }
            Ok(SaveOutcome::Conflict {
                disk_content,
                conflict_copy,
            }) => {
                let title = {
                    let mut i = self.inner.borrow_mut();
                    if let Some(ps) = i.docs.get_mut(rel) {
                        ps.doc.stale = true;
                    }
                    i.docs
                        .get(rel)
                        .map(|ps| ps.doc.title.clone())
                        .unwrap_or_else(|| rel.to_owned())
                };
                self.notify(
                    rel,
                    format!("{title} 在磁盘上被外部修改，你的版本已另存为 {conflict_copy}"),
                    NoticeKind::Conflict {
                        disk: disk_content,
                        copy: conflict_copy,
                    },
                );
                false
            }
            Ok(SaveOutcome::Rejected(_)) => {
                self.notify(rel, format!("{rel} 为只读，未保存"), NoticeKind::Error);
                false
            }
            Err(e) => {
                self.notify(rel, format!("保存 {rel} 失败：{e}"), NoticeKind::Error);
                false
            }
        }
    }

    // ---- leaving ----------------------------------------------------------

    /// Gets every page into its file, then (for `Switch`) relaunches into
    /// `path` and closes the window. Refused, with a notice, when a page
    /// waits for the user to pick a side, a save fails, or an edit is still
    /// open; a refusal resets the flags so it can be tried again.
    pub async fn leave(&self, verb: LeaveVerb, path: Option<PathBuf>) -> LeaveResult {
        {
            let mut i = self.inner.borrow_mut();
            if i.quitting {
                return LeaveResult {
                    ok: false,
                    notice: Some("正在退出，请稍候".to_owned()),
                };
            }
            i.quitting = true;
            i.leaving = true;
        }
        let result = self.try_leave(verb, path).await;
        if !result.ok {
            let mut i = self.inner.borrow_mut();
            i.quitting = false;
            i.leaving = false;
        }
        result
    }

    async fn try_leave(&self, verb: LeaveVerb, path: Option<PathBuf>) -> LeaveResult {
        let result = self.try_leave_inner(verb, path).await;
        match &result.notice {
            Some(notice) if !result.ok => log::info!("leave refused: {notice}"),
            _ => log::info!("leave: closing the window"),
        }
        result
    }

    async fn try_leave_inner(&self, verb: LeaveVerb, path: Option<PathBuf>) -> LeaveResult {
        self.send(PumpMsg::CancelSaves);
        let vault = self.vault();
        // A save already in flight may come back as a conflict; let it land
        // (and mark the page stale) before anything else is written.
        let _ = vault.flush().await;
        let handle = self.handle();
        in_runtime(handle, async {
            tokio::time::sleep(Duration::from_millis(20)).await
        })
        .await;
        if let Some((rel, title)) = self.first_stale_page() {
            let message = format!("{title}：有未处理的外部修改或冲突，处理后再{}", verb.word());
            self.notify(&rel, message.clone(), NoticeKind::Info);
            return LeaveResult {
                ok: false,
                notice: Some(message),
            };
        }
        for rel in self.loaded_rels() {
            if self.needs_save(&rel) && !self.save_now(&rel).await {
                return LeaveResult {
                    ok: false,
                    notice: self.last_notice_for(&rel),
                };
            }
        }
        let _ = vault.flush().await;
        // Typing during the saves above would leave something behind.
        if let Some((rel, title)) = self.first_unsettled_page() {
            let message = format!("{title}：还有没保存的修改，没有{}", verb.word());
            self.notify(&rel, message.clone(), NoticeKind::Info);
            return LeaveResult {
                ok: false,
                notice: Some(message),
            };
        }
        if self.editing().is_some() {
            let message = format!("还有没保存的修改，没有{}", verb.word());
            self.notify("", message.clone(), NoticeKind::Info);
            return LeaveResult {
                ok: false,
                notice: Some(message),
            };
        }
        if verb == LeaveVerb::Switch {
            let Some(path) = path else {
                return LeaveResult {
                    ok: false,
                    notice: Some("缺少目标库路径".to_owned()),
                };
            };
            let relaunch = self.inner.borrow().hooks.relaunch.clone();
            if let Err(e) = relaunch(&path) {
                let message = format!("无法重启到 {}：{e}", path.display());
                self.notify("", message.clone(), NoticeKind::Error);
                return LeaveResult {
                    ok: false,
                    notice: Some(message),
                };
            }
        }
        let close_window = self.inner.borrow().hooks.close_window.clone();
        close_window();
        LeaveResult {
            ok: true,
            notice: None,
        }
    }

    // ---- derived data -----------------------------------------------------

    /// Journal pages to show: today first, then older ones newest first.
    pub async fn journals(&self, count: usize) -> JournalList {
        let mut entries = {
            let i = self.inner.borrow();
            let today_rel = format!("journals/{}.md", i.config.journal.format_file_stem(i.today));
            let mut entries = vec![JournalEntry {
                rel: today_rel.clone(),
                title: i.config.journal.format_title(i.today),
                date: Some(i.today),
            }];
            for entry in i.index.journals() {
                if entry.rel_path != today_rel && entry.journal.is_some_and(|d| d < i.today) {
                    entries.push(JournalEntry {
                        rel: entry.rel_path,
                        title: entry.title,
                        date: entry.journal,
                    });
                }
            }
            entries
        };
        let more = entries.len() > count;
        entries.truncate(count);
        JournalList { entries, more }
    }

    pub fn search(&self, query: &str) -> Vec<SearchHit> {
        if query.trim().is_empty() {
            return Vec::new();
        }
        self.inner.borrow().index.search(query, SEARCH_LIMIT)
    }

    pub fn backlinks(&self, rel: &str) -> Vec<BacklinkGroup> {
        let i = self.inner.borrow();
        let Some(entry) = i.index.page_by_path(rel) else {
            return Vec::new();
        };
        i.index.backlinks(&entry.key)
    }

    /// Block text as Markdown for the viewer, `((uuid))` resolved against the index.
    pub fn render_markdown(&self, text: &str) -> String {
        let i = self.inner.borrow();
        md::render(text, &|id| i.index.block(id).map(|b| b.text))
    }

    pub fn block(&self, id: BlockId) -> Option<BlockInfo> {
        self.inner.borrow().index.block(id)
    }

    /// Completion candidates after a trigger: `kind` is `"page"` (for `[[`)
    /// or `"block"` (for `((`). Pages match the title by prefix first, then
    /// substring; blocks match their text as a substring.
    pub fn complete(&self, query: &str, kind: &str, limit: usize) -> Vec<CompleteItem> {
        let i = self.inner.borrow();
        let q = query.trim().to_lowercase();
        let mut out = Vec::new();
        if kind == "block" {
            if q.is_empty() {
                return out;
            }
            for hit in i.index.search_blocks(&q, limit) {
                let label: String = hit.text.chars().take(60).collect();
                out.push(CompleteItem {
                    kind: "block".into(),
                    insert: format!("{})) ", hit.block.0),
                    label,
                    detail: hit.page.title,
                });
            }
            return out;
        }
        let (mut starts, mut rest) = (Vec::new(), Vec::new());
        for page in i.index.pages() {
            let t = page.title.to_lowercase();
            if q.is_empty() || t.starts_with(&q) {
                starts.push(page);
            } else if t.contains(&q) {
                rest.push(page);
            }
        }
        starts.extend(rest);
        out.extend(starts.into_iter().take(limit).map(|page| CompleteItem {
            kind: "page".into(),
            insert: format!("{}]] ", page.title),
            label: page.title,
            detail: String::new(),
        }));
        out
    }

    /// Non-journal pages by title, for the sidebar.
    pub fn pages(&self) -> Vec<PageEntry> {
        self.inner
            .borrow()
            .index
            .pages()
            .into_iter()
            .filter(|p| p.journal.is_none())
            .collect()
    }

    /// The vault-relative path of a page title, following the journal format.
    pub fn rel_for_title(&self, title: &str) -> String {
        let i = self.inner.borrow();
        let title = title.trim();
        if let Some(entry) = i.index.page(&PageKey::new(title)) {
            return entry.rel_path;
        }
        if let Some(date) = i.config.journal.parse_title(title) {
            return format!("journals/{}.md", i.config.journal.format_file_stem(date));
        }
        format!("pages/{}.md", title_to_file_stem(title))
    }

    pub fn today(&self) -> JournalDate {
        self.inner.borrow().today
    }

    // ---- watcher events ---------------------------------------------------

    fn on_vault_event(&self, event: VaultEvent) -> Vec<FollowUp> {
        match event {
            VaultEvent::Changed { rel, content, .. } => self.external_change(&rel, &content),
            VaultEvent::Removed { rel } => {
                let loaded = {
                    let mut i = self.inner.borrow_mut();
                    i.index.remove_page(&rel);
                    match i.docs.get_mut(&rel) {
                        Some(ps) => {
                            ps.doc.exists = false;
                            true
                        }
                        None => false,
                    }
                };
                self.mark_index_dirty();
                self.emit(Event::Pages);
                if loaded {
                    self.notify(&rel, format!("{rel} 已在磁盘上被删除"), NoticeKind::Info);
                }
                Vec::new()
            }
            VaultEvent::Created { rel } => {
                let loaded = self.inner.borrow().docs.contains_key(&rel);
                if loaded {
                    Vec::new()
                } else {
                    vec![FollowUp::IndexFromDisk { rel }]
                }
            }
            VaultEvent::Rescan => vec![FollowUp::Rescan],
            VaultEvent::Error(message) => {
                self.notify("", format!("文件监视出错：{message}"), NoticeKind::Error);
                Vec::new()
            }
        }
    }

    fn external_change(&self, rel: &str, content: &str) -> Vec<FollowUp> {
        let busy = {
            let i = self.inner.borrow();
            i.docs
                .get(rel)
                .map(|ps| ps.doc.dirty || i.editing.as_ref().is_some_and(|e| e.rel == rel))
        };
        match busy {
            None => {
                // Nobody shows the page: just keep the index honest.
                let mut i = self.inner.borrow_mut();
                let parsed = parse_page(content);
                index_parsed(&mut i, rel, &parsed.page);
                drop(i);
                self.mark_index_dirty();
                Vec::new()
            }
            Some(true) => {
                let title = {
                    let mut i = self.inner.borrow_mut();
                    if let Some(ps) = i.docs.get_mut(rel) {
                        ps.doc.stale = true;
                    }
                    i.docs
                        .get(rel)
                        .map(|ps| ps.doc.title.clone())
                        .unwrap_or_else(|| rel.to_owned())
                };
                self.notify(
                    rel,
                    format!("{title} 被外部修改，而你有未保存的编辑"),
                    NoticeKind::External {
                        content: content.to_owned(),
                    },
                );
                Vec::new()
            }
            Some(false) => {
                let read_only = self.reload(rel, content);
                vec![FollowUp::SetReadOnly {
                    rel: rel.to_owned(),
                    read_only,
                }]
            }
        }
    }

    /// Replaces the page with the parsed `content`, bumps `gen` and forgets
    /// the page's editing state and undo history. Returns the read-only flag.
    fn reload(&self, rel: &str, content: &str) -> bool {
        let (read_only, r#gen) = {
            let mut i = self.inner.borrow_mut();
            let parsed = parse_page(content);
            let (title, journal) = resolve_title(rel, &parsed.page, &i.config);
            let read_only = !parsed.lossless;
            index_parsed(&mut i, rel, &parsed.page);
            if i.editing.as_ref().is_some_and(|e| e.rel == rel) {
                i.editing = None;
            }
            i.undo.retain(|(r, _)| r != rel);
            i.redo.retain(|(r, _)| r != rel);
            let doc = Doc::new(rel.to_owned(), title, journal, parsed.page, true, read_only);
            let r#gen = match i.docs.get_mut(rel) {
                Some(ps) => {
                    ps.r#gen += 1;
                    ps.doc = doc;
                    ps.r#gen
                }
                None => {
                    i.docs.insert(rel.to_owned(), PageState { doc, r#gen: 1 });
                    1
                }
            };
            (read_only, r#gen)
        };
        self.mark_index_dirty();
        self.emit(Event::PageChanged {
            rel: rel.to_owned(),
            r#gen,
        });
        read_only
    }

    // ---- indexing ---------------------------------------------------------

    async fn scan(&self) {
        let files = match self.vault().list_pages().await {
            Ok(files) => files,
            Err(e) => {
                self.notify("", format!("列出页面失败：{e}"), NoticeKind::Error);
                return;
            }
        };
        for file in files {
            if !self.inner.borrow().docs.contains_key(&file.rel) {
                self.index_from_disk(&file.rel, false).await;
            }
        }
        self.mark_index_dirty();
        self.emit(Event::Pages);
    }

    async fn index_from_disk(&self, rel: &str, announce: bool) {
        if let Ok(loaded) = self.vault().load(rel).await {
            {
                let mut i = self.inner.borrow_mut();
                let parsed = parse_page(&loaded.content);
                index_parsed(&mut i, rel, &parsed.page);
            }
            self.mark_index_dirty();
            if announce {
                self.emit(Event::Pages);
            }
        }
    }

    // ---- small helpers ----------------------------------------------------

    fn vault(&self) -> VaultHandle {
        self.inner.borrow().vault.clone()
    }

    fn handle(&self) -> Handle {
        self.inner.borrow().handle.clone()
    }

    fn is_leaving(&self) -> bool {
        self.inner.borrow().leaving
    }

    fn loaded_rels(&self) -> Vec<String> {
        self.inner.borrow().docs.keys().cloned().collect()
    }

    fn first_stale_page(&self) -> Option<(String, String)> {
        self.inner
            .borrow()
            .docs
            .values()
            .find(|ps| ps.doc.stale)
            .map(|ps| (ps.doc.rel.clone(), ps.doc.title.clone()))
    }

    /// Edits that a save would write.
    fn needs_save(&self, rel: &str) -> bool {
        self.inner.borrow().docs.get(rel).is_some_and(|ps| {
            !ps.doc.read_only && ps.doc.dirty && (ps.doc.exists || ps.doc.has_content())
        })
    }

    fn first_unsettled_page(&self) -> Option<(String, String)> {
        self.loaded_rels()
            .into_iter()
            .find(|rel| {
                self.needs_save(rel)
                    || self
                        .inner
                        .borrow()
                        .docs
                        .get(rel)
                        .is_some_and(|ps| ps.doc.stale)
            })
            .map(|rel| {
                let title = self
                    .inner
                    .borrow()
                    .docs
                    .get(&rel)
                    .map(|ps| ps.doc.title.clone())
                    .unwrap_or_else(|| rel.clone());
                (rel, title)
            })
    }

    fn last_notice_for(&self, rel: &str) -> Option<String> {
        self.inner
            .borrow()
            .notices
            .iter()
            .rev()
            .find(|n| n.rel == rel)
            .map(|n| n.message.clone())
    }
}

/// Opens `path` as the new current vault and replaces the boot data, so the
/// next [`Session::from_boot`] talks to it. The path is recorded in the
/// recent-vaults state file.
pub async fn open(path: &Path) -> Result<Boot, OpenError> {
    let prev = boot().ok_or(OpenError::NoBoot)?;
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if let Some(reason) = startup::is_refused(path, home.as_deref()) {
        return Err(OpenError::Refused(reason));
    }
    let handle = prev.handle.clone();
    let state_file = prev.state_file.clone();
    let path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    if let Some(reason) = startup::is_refused(&path, home.as_deref()) {
        return Err(OpenError::Refused(reason));
    }
    let (vault, events, recent, saved_view) = handle
        .spawn(async move {
            let (vault, events) = Vault::open(path).await?;
            let root = vault.root().to_path_buf();
            let (recent, saved_view) = remember_vault(&root, state_file.as_deref()).await;
            Ok::<_, VaultError>((vault, events, recent, saved_view))
        })
        .await
        .map_err(|e| OpenError::Task(e.to_string()))??;
    let view = startup::view_from_saved(saved_view.as_deref(), vault.root());
    let next = Boot {
        vault: Some(vault),
        events: Some(Arc::new(tokio::sync::Mutex::new(events))),
        today: prev.today,
        handle: prev.handle,
        view,
        notice: None,
        recent,
        relaunch: prev.relaunch,
        state_file: prev.state_file,
    };
    set_boot(next.clone());
    Ok(next)
}

/// Pushes `root` to the front of the recent-vaults file and returns the new
/// list and the view saved for `root`. A failure to write the file downgrades
/// to just the opened vault.
async fn remember_vault(root: &Path, state_file: Option<&Path>) -> (Vec<String>, Option<String>) {
    let only = || (vec![root.to_string_lossy().into_owned()], None);
    let Some(file) = state_file else {
        return only();
    };
    let root_owned = root.to_path_buf();
    let file = file.to_path_buf();
    let result = tokio::task::spawn_blocking(move || {
        crate::state::update(&file, |entries| {
            crate::state::push_front(entries, &root_owned)
        })?;
        let text = std::fs::read_to_string(&file).unwrap_or_default();
        Ok::<_, io::Error>(crate::state::parse(&text))
    })
    .await;
    match result {
        Ok(Ok(entries)) => {
            let view = entries
                .iter()
                .find(|e| e.path == root)
                .and_then(|e| e.view.clone());
            let recent = entries
                .iter()
                .map(|e| e.path.to_string_lossy().into_owned())
                .collect();
            (recent, view)
        }
        Ok(Err(e)) => {
            log::warn!("cannot update the recent-vaults file: {e}");
            only()
        }
        Err(e) => {
            log::warn!("cannot update the recent-vaults file: {e}");
            only()
        }
    }
}

/// Classifies `path` for the switch-vault flow without opening it.
pub async fn inspect(path: &Path) -> Inspect {
    fn refused(reason: impl Into<String>) -> Inspect {
        Inspect {
            canonical: None,
            kind: startup::VaultKind::Other,
            reason: Some(reason.into()),
            is_current: false,
        }
    }
    let Some(boot) = boot() else {
        return refused("应用尚未启动");
    };
    let current = boot.vault.as_ref().map(|v| v.root().to_path_buf());
    let path = path.to_path_buf();
    let looked = boot
        .handle
        .spawn(async move {
            tokio::task::spawn_blocking(move || {
                let canonical = std::fs::canonicalize(&path).ok();
                let kind = canonical
                    .as_deref()
                    .filter(|p| p.is_dir())
                    .map(startup::looks_like_vault);
                (canonical, kind)
            })
            .await
        })
        .await;
    let (canonical, kind) = match looked {
        Ok(Ok(found)) => found,
        Ok(Err(e)) => return refused(format!("无法检查路径：{e}")),
        Err(e) => return refused(format!("无法检查路径：{e}")),
    };
    let Some(canonical) = canonical else {
        return refused("路径不存在或不可读");
    };
    let Some(kind) = kind else {
        return refused("所选路径不是文件夹");
    };
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if let Some(reason) = startup::is_refused(&canonical, home.as_deref()) {
        return Inspect {
            canonical: Some(canonical),
            kind,
            reason: Some(reason),
            is_current: false,
        };
    }
    Inspect {
        is_current: current.as_deref() == Some(canonical.as_path()),
        canonical: Some(canonical),
        kind,
        reason: None,
    }
}

/// The latest save deadline, or disarmed when nothing is due.
fn rearm_save_timer(timer: &mut Pin<Box<Sleep>>, deadlines: &HashMap<String, Instant>) {
    let next = deadlines.values().min().copied();
    timer
        .as_mut()
        .reset(next.unwrap_or_else(|| Instant::now() + NEVER));
}

/// Backlog coalescing: one PageChanged per page, one IndexVer, one Pages;
/// notices all survive. The single source of truth: the api layer replays
/// its own listener backlog through the same function.
pub(crate) fn coalesce(backlog: &mut Vec<Event>, event: Event) {
    match &event {
        Event::PageChanged { rel, .. } => {
            backlog.retain(|e| !matches!(e, Event::PageChanged { rel: r, .. } if r == rel));
        }
        Event::IndexVer(_) => backlog.retain(|e| !matches!(e, Event::IndexVer(_))),
        Event::Pages => backlog.retain(|e| !matches!(e, Event::Pages)),
        Event::Notice(_) => {}
    }
    backlog.push(event);
}

fn index_page(i: &mut Inner, rel: &str) {
    let Some(ps) = i.docs.get(rel) else {
        return;
    };
    i.index.upsert_page(PageSource {
        rel_path: rel,
        page: &ps.doc.page,
        config: &i.config,
    });
}

fn index_parsed(i: &mut Inner, rel: &str, page: &Page) {
    i.index.upsert_page(PageSource {
        rel_path: rel,
        page,
        config: &i.config,
    });
}

fn push_undo(i: &mut Inner, rel: &str, before: Page) {
    i.undo.push((rel.to_owned(), before));
    if i.undo.len() > UNDO_LIMIT {
        i.undo.remove(0);
    }
    i.redo.clear();
}

/// Drops the external-change and conflict notices of `rel`: once the user
/// picks a side, the others describe a state that no longer exists.
fn resolve_notices(i: &mut Inner, rel: &str) {
    i.notices.retain(|n| {
        n.rel != rel
            || !matches!(
                n.kind,
                NoticeKind::External { .. } | NoticeKind::Conflict { .. }
            )
    });
}

/// A row revision: the source text plus the Markdown, which already contains
/// the resolved reference text.
fn row_rev(text: &str, markdown: &str) -> u64 {
    let mut hasher = blake3::Hasher::new();
    hasher.update(text.as_bytes());
    hasher.update(&[0xff]);
    hasher.update(markdown.as_bytes());
    let hash = hasher.finalize();
    u64::from_le_bytes(hash.as_bytes()[..8].try_into().expect("blake3 is 32 bytes"))
}

/// Byte offset of UTF-16 offset `units` in `text` (clamped, on a char boundary).
fn utf16_to_byte(text: &str, units: usize) -> usize {
    let mut seen = 0;
    for (byte, ch) in text.char_indices() {
        if seen >= units {
            return byte;
        }
        seen += ch.len_utf16();
    }
    text.len()
}

/// Expands the collapsed ancestors of `id` so the block stays visible.
fn reveal(page: &mut Page, id: BlockId) {
    fn path(blocks: &[Block], id: BlockId, out: &mut Vec<BlockId>) -> bool {
        for b in blocks {
            if b.id == id {
                return true;
            }
            out.push(b.id);
            if path(&b.children, id, out) {
                return true;
            }
            out.pop();
        }
        false
    }
    let mut ancestors = Vec::new();
    let top: Vec<Block> = page.blocks().cloned().collect();
    if path(&top, id, &mut ancestors) {
        for ancestor in ancestors {
            ops::set_collapsed(page, ancestor, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utf16_offsets_map_to_bytes() {
        assert_eq!(utf16_to_byte("abc", 2), 2);
        assert_eq!(utf16_to_byte("你好", 1), 3);
        assert_eq!(utf16_to_byte("a😀b", 3), 5);
        assert_eq!(utf16_to_byte("ab", 9), 2);
    }

    #[test]
    fn reveal_opens_collapsed_parents() {
        let mut page = parse_page("- a\n\tcollapsed:: true\n\t- b\n").page;
        let child = page.blocks().next().unwrap().children[0].id;
        reveal(&mut page, child);
        assert!(!page.blocks().next().unwrap().is_collapsed());
    }

    #[test]
    fn rev_tracks_text_and_markdown() {
        assert_ne!(row_rev("a", "a"), row_rev("b", "a"));
        assert_ne!(row_rev("a", "a"), row_rev("a", "b"));
        assert_eq!(row_rev("a", "a"), row_rev("a", "a"));
    }

    #[test]
    fn backlog_coalesces_by_kind() {
        let mut backlog = Vec::new();
        coalesce(&mut backlog, Event::IndexVer(1));
        coalesce(&mut backlog, Event::IndexVer(2));
        coalesce(
            &mut backlog,
            Event::PageChanged {
                rel: "pages/a.md".into(),
                r#gen: 2,
            },
        );
        coalesce(
            &mut backlog,
            Event::PageChanged {
                rel: "pages/a.md".into(),
                r#gen: 3,
            },
        );
        coalesce(&mut backlog, Event::Pages);
        coalesce(&mut backlog, Event::Pages);
        assert_eq!(
            backlog,
            vec![
                Event::IndexVer(2),
                Event::PageChanged {
                    rel: "pages/a.md".into(),
                    r#gen: 3
                },
                Event::Pages,
            ]
        );
    }
}
