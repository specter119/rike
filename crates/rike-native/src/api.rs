//! The `rike` native module: what TS calls. Thin: DTO conversion, the
//! engine-thread `Session`, and the event callback. Behavior lives in
//! [`crate::session`].
//!
//! Sync members only touch memory and return in the same call. Anything that
//! reads the disk or opens a dialog is `async`. No `RefCell` borrow is held
//! across an `.await`: async members clone the `Session` handle out of the
//! thread-local first.

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex, OnceLock};

use nsh_core::file_dialog::OpenDialog;
use nsh_core::prelude::*;
use rike_core::BlockId;
use rike_index::{BacklinkGroup, BlockInfo, PageEntry, SearchHit};

/// How many completion candidates one panel shows.
const COMPLETE_LIMIT: usize = 8;
use tokio::sync::Notify;
use uuid::Uuid;

use crate::session::{
    self, Event, Focus, Hooks, Inspect, JournalList, LeaveResult, LeaveVerb, Notice, NoticeKind,
    OpResult, PageView, Row, Session, View,
};
use crate::startup::VaultKind;
use crate::state::{self, StateWriter};
use crate::{math, md};

// ---- DTOs ------------------------------------------------------------------

/// A file a UI test seeds into its vault.
#[derive(Debug, Clone, JsObject)]
pub struct DevFileDto {
    /// Vault-relative path.
    pub path: String,
    pub text: String,
}

/// Where the UI is.
#[derive(Debug, Clone, JsObject)]
pub struct ViewDto {
    /// `"journals"` or `"page"`.
    pub kind: String,
    /// The page's vault-relative path when `kind` is `"page"`.
    pub rel: Option<String>,
}

/// What the app starts with.
#[derive(Debug, Clone, JsObject)]
pub struct BootDto {
    /// `"vault"` when a vault is open, `"welcome"` otherwise.
    pub mode: String,
    /// A startup message for the welcome page.
    pub notice: Option<String>,
    /// Recently opened vaults, newest first.
    pub recent: Vec<String>,
    /// The open vault's root.
    pub root: Option<String>,
    pub view: ViewDto,
    /// Debug builds with `RIKE_DEBUG_EDIT_FIRST` set: start editing the first block
    /// (the IME smoke test has no pointer to click with).
    pub debug_edit_first: bool,
}

/// A toast or a page-level banner.
#[derive(Debug, Clone, JsObject)]
pub struct NoticeDto {
    pub id: u64,
    pub rel: String,
    pub message: String,
    /// `"info"`, `"error"`, `"external"` or `"conflict"`.
    pub kind: String,
    /// The disk version for `"external"`, for display only.
    pub content: Option<String>,
    /// The conflict copy's vault-relative path for `"conflict"`.
    pub copy: Option<String>,
}

/// One visible block.
#[derive(Debug, Clone, JsObject)]
pub struct RowDto {
    pub id: String,
    pub depth: u32,
    /// The source text, what the editor shows.
    pub text: String,
    /// The text as Markdown for the viewer, `((uuid))` resolved.
    pub markdown: String,
    /// Changes when `text` or a resolved reference changes; rebuild the row then.
    pub rev: String,
    /// The task keyword (`"TODO"`, `"DONE"`, ...) when the block is a task.
    pub task: Option<String>,
    pub task_done: bool,
    pub collapsed: bool,
    pub has_children: bool,
}

#[derive(Debug, Clone, JsObject)]
pub struct PageViewDto {
    pub rel: String,
    pub title: String,
    /// Drafts and operations must carry this; it grows when the page is
    /// replaced from disk.
    pub generation: u64,
    pub read_only: bool,
    pub stale: bool,
    pub notice: Option<NoticeDto>,
    pub rows: Vec<RowDto>,
}

#[derive(Debug, Clone, JsObject)]
pub struct FocusDto {
    pub id: String,
    pub caret_start: bool,
    /// An absolute caret for the rebuilt editor (UTF-16 units), for ops that
    /// rewrite the block's text in place.
    pub caret: Option<u32>,
}

/// The result of a structure operation.
#[derive(Debug, Clone, JsObject)]
pub struct OpResultDto {
    pub focus: Option<FocusDto>,
    pub page: PageViewDto,
}

#[derive(Debug, Clone, JsObject)]
pub struct JournalEntryDto {
    pub rel: String,
    pub title: String,
}

#[derive(Debug, Clone, JsObject)]
pub struct JournalListDto {
    pub entries: Vec<JournalEntryDto>,
    pub more: bool,
}

#[derive(Debug, Clone, JsObject)]
pub struct PageEntryDto {
    pub rel: String,
    pub title: String,
}

#[derive(Debug, Clone, JsObject)]
pub struct SearchHitDto {
    /// `"page"` or `"block"`.
    pub kind: String,
    pub rel: String,
    pub title: String,
    pub block: Option<String>,
    pub text: Option<String>,
}

#[derive(Debug, Clone, JsObject)]
pub struct BacklinkItemDto {
    pub block: String,
    pub text: String,
    /// `text` as Markdown for the viewer.
    pub markdown: String,
    pub ancestors: Vec<String>,
}

#[derive(Debug, Clone, JsObject)]
pub struct BacklinkGroupDto {
    pub rel: String,
    pub title: String,
    pub items: Vec<BacklinkItemDto>,
}

/// One completion candidate: `kind` is `"page"` or `"block"`; `insert` is
/// the text to put in place of the query (closing the reference), `label`
/// and `detail` what the panel shows.
#[derive(Debug, Clone, JsObject)]
pub struct CompleteItemDto {
    pub kind: String,
    pub insert: String,
    pub label: String,
    pub detail: String,
}

#[derive(Debug, Clone, JsObject)]
pub struct BlockInfoDto {
    pub id: String,
    pub text: String,
    pub rel: String,
    pub title: String,
}

/// A rendered formula: an image URI and where it sits in the text line.
#[derive(Debug, Clone, JsObject)]
pub struct FormulaDto {
    pub uri: String,
    pub width: f64,
    pub height: f64,
    pub baseline: f64,
}

/// What a `sx-page:` / `sx-block:` link points at.
#[derive(Debug, Clone, JsObject)]
pub struct LinkDto {
    /// `"page"` or `"block"`.
    pub kind: String,
    /// The page title or the block id.
    pub target: String,
}

#[derive(Debug, Clone, JsObject)]
pub struct InspectDto {
    pub canonical: Option<String>,
    /// `"vault"`, `"empty"` or `"other"`; meaningful when `reason` is empty.
    pub kind: String,
    /// Why the folder must not be opened.
    pub reason: Option<String>,
    pub is_current: bool,
}

#[derive(Debug, Clone, JsObject)]
pub struct LeaveResultDto {
    pub ok: bool,
    pub notice: Option<String>,
}

/// Something the UI should react to.
#[derive(Debug, Clone, JsObject)]
pub struct EventDto {
    /// `"page"`, `"notice"`, `"index"`, `"pages"` or `"indexProgress"`.
    pub kind: String,
    pub rel: Option<String>,
    pub generation: Option<u64>,
    pub ver: Option<u64>,
    pub notice: Option<NoticeDto>,
    /// `kind == "index"`: pages upserted since the previous index event.
    pub changed: Vec<String>,
    /// `kind == "index"`: pages removed since the previous index event.
    pub removed: Vec<String>,
    /// `kind == "index"`: page keys whose backlink groups changed.
    pub backlink_keys: Vec<String>,
    /// `kind == "indexProgress"`: pages parsed, pages to parse, and whether
    /// the background index is still running.
    pub done: Option<u32>,
    pub total: Option<u32>,
    pub indexing: Option<bool>,
}

// ---- conversions -----------------------------------------------------------

fn block_id(text: &str) -> Result<BlockId, String> {
    Uuid::parse_str(text)
        .map(BlockId)
        .map_err(|_| format!("not a block id: {text}"))
}

fn id_text(id: BlockId) -> String {
    id.0.to_string()
}

fn path_text(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn view_dto(view: &View) -> ViewDto {
    match view {
        View::Journals => ViewDto {
            kind: "journals".into(),
            rel: None,
        },
        View::Page(rel) => ViewDto {
            kind: "page".into(),
            rel: Some(rel.clone()),
        },
    }
}

fn view_of(dto: &ViewDto) -> View {
    match (dto.kind.as_str(), &dto.rel) {
        ("page", Some(rel)) => View::Page(rel.clone()),
        _ => View::Journals,
    }
}

fn boot_dto(boot: &session::Boot) -> BootDto {
    BootDto {
        mode: if boot.vault.is_some() {
            "vault"
        } else {
            "welcome"
        }
        .into(),
        notice: boot.notice.clone(),
        recent: boot.recent.clone(),
        root: boot.vault.as_ref().map(|v| path_text(v.root())),
        view: view_dto(&boot.view),
        debug_edit_first: cfg!(debug_assertions)
            && std::env::var_os("RIKE_DEBUG_EDIT_FIRST").is_some(),
    }
}

fn notice_dto(n: Notice) -> NoticeDto {
    let (kind, content, copy) = match n.kind {
        NoticeKind::Info => ("info", None, None),
        NoticeKind::Error => ("error", None, None),
        NoticeKind::External { content } => ("external", Some(content), None),
        NoticeKind::Conflict { disk, copy } => ("conflict", disk, Some(copy)),
    };
    NoticeDto {
        id: n.id,
        rel: n.rel,
        message: n.message,
        kind: kind.into(),
        content,
        copy,
    }
}

fn row_dto(row: Row) -> RowDto {
    RowDto {
        id: id_text(row.id),
        depth: row.depth as u32,
        text: row.text,
        markdown: row.markdown,
        rev: format!("{:x}", row.rev),
        task_done: row.task.is_some_and(|m| !m.is_open()),
        task: row.task.map(|m| m.keyword().to_owned()),
        collapsed: row.collapsed,
        has_children: row.has_children,
    }
}

fn page_dto(page: PageView) -> PageViewDto {
    PageViewDto {
        rel: page.rel,
        title: page.title,
        generation: page.r#gen,
        read_only: page.read_only,
        stale: page.stale,
        notice: page.notice.map(notice_dto),
        rows: page.rows.into_iter().map(row_dto).collect(),
    }
}

fn op_dto(result: OpResult) -> OpResultDto {
    OpResultDto {
        focus: result.focus.map(
            |Focus {
                 id,
                 caret_start,
                 caret,
             }| FocusDto {
                id: id_text(id),
                caret_start,
                caret: caret.map(|c| u32::try_from(c).unwrap_or(u32::MAX)),
            },
        ),
        page: page_dto(result.page),
    }
}

fn entry_dto(entry: &PageEntry) -> PageEntryDto {
    PageEntryDto {
        rel: entry.rel_path.clone(),
        title: entry.title.clone(),
    }
}

fn journals_dto(list: JournalList) -> JournalListDto {
    JournalListDto {
        entries: list
            .entries
            .into_iter()
            .map(|e| JournalEntryDto {
                rel: e.rel,
                title: e.title,
            })
            .collect(),
        more: list.more,
    }
}

fn hit_dto(hit: SearchHit) -> SearchHitDto {
    match hit {
        SearchHit::Page(page) => SearchHitDto {
            kind: "page".into(),
            rel: page.rel_path,
            title: page.title,
            block: None,
            text: None,
        },
        SearchHit::Block { page, block, text } => SearchHitDto {
            kind: "block".into(),
            rel: page.rel_path,
            title: page.title,
            block: Some(id_text(block)),
            text: Some(text),
        },
    }
}

fn group_dto(session: &Session, group: BacklinkGroup) -> BacklinkGroupDto {
    BacklinkGroupDto {
        rel: group.source.rel_path,
        title: group.source.title,
        items: group
            .items
            .into_iter()
            .map(|item| BacklinkItemDto {
                block: id_text(item.block),
                markdown: session.render_markdown(&item.text),
                text: item.text,
                ancestors: item.ancestors,
            })
            .collect(),
    }
}

fn info_dto(info: BlockInfo) -> BlockInfoDto {
    BlockInfoDto {
        id: id_text(info.id),
        text: info.text,
        rel: info.page.rel_path,
        title: info.page.title,
    }
}

fn inspect_dto(inspect: Inspect) -> InspectDto {
    InspectDto {
        canonical: inspect.canonical.as_deref().map(path_text),
        kind: match inspect.kind {
            VaultKind::Vault => "vault",
            VaultKind::Empty => "empty",
            VaultKind::Other => "other",
        }
        .into(),
        reason: inspect.reason,
        is_current: inspect.is_current,
    }
}

fn event_dto(event: &Event) -> EventDto {
    let empty = EventDto {
        kind: String::new(),
        rel: None,
        generation: None,
        ver: None,
        notice: None,
        changed: Vec::new(),
        removed: Vec::new(),
        backlink_keys: Vec::new(),
        done: None,
        total: None,
        indexing: None,
    };
    match event {
        Event::PageChanged { rel, r#gen } => EventDto {
            kind: "page".into(),
            rel: Some(rel.clone()),
            generation: Some(*r#gen),
            ..empty
        },
        Event::Notice(notice) => EventDto {
            kind: "notice".into(),
            notice: Some(notice_dto(notice.clone())),
            ..empty
        },
        Event::IndexProgress {
            done,
            total,
            indexing,
        } => EventDto {
            kind: "indexProgress".into(),
            done: Some(*done as u32),
            total: Some(*total as u32),
            indexing: Some(*indexing),
            ..empty
        },
        Event::IndexVer { ver, changes } => EventDto {
            kind: "index".into(),
            ver: Some(*ver),
            changed: changes.changed.iter().cloned().collect(),
            removed: changes.removed.iter().cloned().collect(),
            backlink_keys: changes
                .backlink_keys
                .iter()
                .map(|key| key.as_str().to_owned())
                .collect(),
            ..empty
        },
        Event::Pages => EventDto {
            kind: "pages".into(),
            ..empty
        },
    }
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

// ---- engine-thread state ---------------------------------------------------

thread_local! {
    /// The open vault's session.
    static SESSION: RefCell<Option<Session>> = const { RefCell::new(None) };
    /// A session whose pump has not started yet.
    static PENDING: RefCell<Option<Session>> = const { RefCell::new(None) };
    static WAKE: Rc<Notify> = Rc::new(Notify::new());
    /// Newest `pump()` call; older resident loops stop at their next wake-up.
    static PUMP_EPOCH: Cell<u64> = const { Cell::new(0) };
    /// The event listener, with its registration id.
    static LISTENER: RefCell<Option<(u64, Callback<EventDto>)>> = const { RefCell::new(None) };
    static NEXT_LISTENER: Cell<u64> = const { Cell::new(0) };
    /// Events that arrived while nobody listened.
    static BACKLOG: RefCell<Vec<Event>> = const { RefCell::new(Vec::new()) };
}

fn session() -> Result<Session, String> {
    SESSION
        .with(|s| s.borrow().clone())
        .ok_or_else(|| "no vault is open".to_owned())
}

/// The session sink: posts to the listener, or keeps the event for the next one.
fn deliver(event: Event) {
    let listener = LISTENER.with(|l| l.borrow().as_ref().map(|(_, cb)| cb.clone()));
    match listener {
        Some(cb) => cb.post(event_dto(&event)),
        None => BACKLOG.with(|b| b.borrow_mut().push(event)),
    }
}

/// Keeps one `IndexVer`, one `Pages` and the newest `PageChanged` per page;
/// notices all stay. Delegates to `session::coalesce` so both backlog layers
/// share one rule set.
fn coalesce(events: Vec<Event>) -> Vec<Event> {
    let mut out: Vec<Event> = Vec::new();
    for event in events {
        session::coalesce(&mut out, event);
    }
    out
}

fn hooks() -> Result<Hooks, String> {
    let boot = session::boot_or_dev().ok_or("the app was started without boot data")?;
    Ok(Hooks::new(
        boot.relaunch.clone(),
        Arc::new(|| nsh_core::element::desktop::main_window().close()),
    ))
}

/// Writes the recent-vaults file off the engine thread, in request order.
fn persist_view(view: &View) {
    static WRITER: OnceLock<Mutex<Option<StateWriter>>> = OnceLock::new();
    let Some(boot) = session::boot() else { return };
    let (Some(file), Some(vault)) = (boot.state_file.clone(), boot.vault.as_ref()) else {
        return;
    };
    let root = vault.root().to_path_buf();
    let text = match view {
        View::Journals => "journals".to_owned(),
        View::Page(rel) => format!("page:{rel}"),
    };
    let writer = {
        let mut slot = WRITER
            .get_or_init(|| Mutex::new(None))
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        match &*slot {
            Some(w) if w.file() == file => w.clone(),
            _ => {
                let w = StateWriter::new(file);
                *slot = Some(w.clone());
                w
            }
        }
    };
    let seq = writer.reserve();
    boot.handle.spawn_blocking(move || {
        let result = writer.apply(seq, |entries| state::set_view(entries, &root, Some(&text)));
        if let Err(e) = result {
            log::warn!("cannot save the view: {e}");
        }
    });
}

// ---- the module ------------------------------------------------------------

/// Rike's vault session, for the TSX UI.
#[module]
pub struct Rike;

#[module]
impl Rike {
    // -- start-up (sync) --

    /// What the app starts with. Repeatable: it never consumes the boot data.
    pub fn boot() -> Result<BootDto, String> {
        session::boot_or_dev()
            .map(|b| boot_dto(&b))
            .ok_or_else(|| "the app was started without boot data".to_owned())
    }

    /// Listens to session events. Events that arrived with no listener are
    /// replayed (coalesced). The returned `Stop` only removes this registration.
    pub fn on_event(listener: Callback<EventDto>) -> Stop {
        let id = NEXT_LISTENER.with(|n| {
            n.set(n.get() + 1);
            n.get()
        });
        LISTENER.with(|l| *l.borrow_mut() = Some((id, listener.clone())));
        for event in coalesce(BACKLOG.with(|b| std::mem::take(&mut *b.borrow_mut()))) {
            listener.post(event_dto(&event));
        }
        Stop::new(move || {
            LISTENER.with(|l| {
                let mut slot = l.borrow_mut();
                if matches!(&*slot, Some((current, _)) if *current == id) {
                    *slot = None;
                }
            });
        })
    }

    // -- start-up (async) --

    /// Builds the session for the open vault and loads its index. Call after
    /// `pump()` is running. No vault (welcome page): does nothing.
    pub async fn start() -> Result<(), String> {
        let boot = session::boot().ok_or("the app was started without boot data")?;
        if let Some(old) = SESSION.with(|s| s.borrow_mut().take()) {
            old.shutdown();
        }
        if boot.vault.is_none() {
            return Ok(());
        }
        let new = Session::from_boot(&boot, hooks()?).ok_or("no vault is open")?;
        new.set_event_sink(Some(Arc::new(deliver)));
        SESSION.with(|s| *s.borrow_mut() = Some(new.clone()));
        PENDING.with(|p| *p.borrow_mut() = Some(new.clone()));
        WAKE.with(|w| w.notify_one());
        new.start().await;
        Ok(())
    }

    /// The resident task. Runs each session's pump as it appears; never
    /// returns on its own. A newer `pump()` call retires an older one.
    pub async fn pump() {
        let epoch = PUMP_EPOCH.with(|e| {
            e.set(e.get() + 1);
            e.get()
        });
        let wake = WAKE.with(Rc::clone);
        wake.notify_waiters();
        loop {
            if PUMP_EPOCH.with(Cell::get) != epoch {
                return;
            }
            match PENDING.with(|p| p.borrow_mut().take()) {
                Some(session) => session.pump().await,
                None => wake.notified().await,
            }
        }
    }

    // -- vault selection --

    /// Looks at a folder before it is opened.
    pub async fn inspect(path: String) -> InspectDto {
        inspect_dto(session::inspect(Path::new(&path)).await)
    }

    /// The system's folder dialog. `None` when the user cancels.
    pub async fn pick_folder(start: Option<String>) -> Result<Option<String>, String> {
        let mut dialog = OpenDialog::folders()
            .title("选择笔记库文件夹")
            .button_label("打开");
        if let Some(start) = start {
            dialog = dialog.directory(PathBuf::from(start));
        }
        let picked = pick_paths(dialog).await?;
        Ok(picked
            .and_then(|paths| paths.into_iter().next())
            .map(|p| path_text(&p)))
    }

    /// Opens a folder as the vault in place. Call `start()` afterwards.
    pub async fn open(path: String) -> Result<BootDto, String> {
        let boot = session::open(Path::new(&path)).await.map_err(err)?;
        Ok(boot_dto(&boot))
    }

    /// Saves everything, then quits (`"quit"`) or restarts into `path`
    /// (`"switch"`). Refused while a page waits for the user to pick a side
    /// or a draft is still open.
    pub async fn leave(verb: String, path: Option<String>) -> Result<LeaveResultDto, String> {
        let verb = match verb.as_str() {
            "quit" => LeaveVerb::Quit,
            "switch" => LeaveVerb::Switch,
            other => return Err(format!("unknown verb: {other}")),
        };
        let LeaveResult { ok, notice } = session()?.leave(verb, path.map(PathBuf::from)).await;
        Ok(LeaveResultDto { ok, notice })
    }

    // -- pages (async: they may read the disk) --

    pub async fn journals(count: u32) -> Result<JournalListDto, String> {
        Ok(journals_dto(session()?.journals(count as usize).await))
    }

    pub async fn page(rel: String) -> Result<PageViewDto, String> {
        session()?.page(&rel).await.map(page_dto).map_err(err)
    }

    /// "Load the version on disk": re-reads the file and bumps the generation.
    /// End the page's editing before calling it.
    pub async fn accept_disk(rel: String) -> Result<PageViewDto, String> {
        session()?
            .accept_disk(&rel)
            .await
            .map(page_dto)
            .map_err(err)
    }

    // -- editing (sync) --

    /// Registers the block being edited; the page is then not reloaded by
    /// external changes.
    pub fn set_editing(rel: String, generation: u64, id: String) -> Result<(), String> {
        session()?
            .set_editing(&rel, generation, block_id(&id)?)
            .map_err(err)
    }

    pub fn clear_editing() -> Result<(), String> {
        session()?.clear_editing();
        Ok(())
    }

    /// Moves the editor's text into the model. `false` when the page was
    /// replaced from disk meanwhile (the draft is dropped).
    pub fn set_draft(
        rel: String,
        generation: u64,
        id: String,
        text: String,
    ) -> Result<bool, String> {
        Ok(session()?.set_draft(&rel, generation, block_id(&id)?, &text))
    }

    /// Enter: split the block at `caret` (UTF-16 units).
    pub fn split(
        rel: String,
        generation: u64,
        id: String,
        text: String,
        caret: Option<u32>,
    ) -> Result<OpResultDto, String> {
        session()?
            .split(&rel, generation, block_id(&id)?, text, caret)
            .map(op_dto)
            .map_err(err)
    }

    /// Replaces the UTF-16 range `[start, end)` of a block's text with
    /// `insert`: how a completion candidate is accepted. `expected` must be
    /// what the range holds now, or the op is refused.
    pub fn replace(
        rel: String,
        generation: u64,
        id: String,
        start: u32,
        end: u32,
        expected: String,
        insert: String,
    ) -> Result<OpResultDto, String> {
        session()?
            .replace_range(
                &rel,
                generation,
                block_id(&id)?,
                start as usize,
                end as usize,
                &expected,
                &insert,
            )
            .map(op_dto)
            .map_err(err)
    }

    pub fn indent(
        rel: String,
        generation: u64,
        id: String,
        text: Option<String>,
    ) -> Result<OpResultDto, String> {
        session()?
            .indent(&rel, generation, block_id(&id)?, text)
            .map(op_dto)
            .map_err(err)
    }

    pub fn outdent(
        rel: String,
        generation: u64,
        id: String,
        text: Option<String>,
    ) -> Result<OpResultDto, String> {
        session()?
            .outdent(&rel, generation, block_id(&id)?, text)
            .map(op_dto)
            .map_err(err)
    }

    pub fn move_up(
        rel: String,
        generation: u64,
        id: String,
        text: Option<String>,
    ) -> Result<OpResultDto, String> {
        session()?
            .move_up(&rel, generation, block_id(&id)?, text)
            .map(op_dto)
            .map_err(err)
    }

    pub fn move_down(
        rel: String,
        generation: u64,
        id: String,
        text: Option<String>,
    ) -> Result<OpResultDto, String> {
        session()?
            .move_down(&rel, generation, block_id(&id)?, text)
            .map(op_dto)
            .map_err(err)
    }

    /// Backspace in an empty block.
    pub fn remove(
        rel: String,
        generation: u64,
        id: String,
        text: Option<String>,
    ) -> Result<OpResultDto, String> {
        session()?
            .remove(&rel, generation, block_id(&id)?, text)
            .map(op_dto)
            .map_err(err)
    }

    pub fn toggle_collapse(
        rel: String,
        generation: u64,
        id: String,
    ) -> Result<OpResultDto, String> {
        session()?
            .toggle_collapse(&rel, generation, block_id(&id)?)
            .map(op_dto)
            .map_err(err)
    }

    pub fn toggle_todo(
        rel: String,
        generation: u64,
        id: String,
        text: Option<String>,
    ) -> Result<OpResultDto, String> {
        session()?
            .toggle_todo(&rel, generation, block_id(&id)?, text)
            .map(op_dto)
            .map_err(err)
    }

    pub fn set_task_done(
        rel: String,
        generation: u64,
        id: String,
        done: bool,
    ) -> Result<OpResultDto, String> {
        session()?
            .set_task_done(&rel, generation, block_id(&id)?, done)
            .map(op_dto)
            .map_err(err)
    }

    pub fn undo(rel: String, generation: u64) -> Result<OpResultDto, String> {
        session()?.undo(&rel, generation).map(op_dto).map_err(err)
    }

    pub fn redo(rel: String, generation: u64) -> Result<OpResultDto, String> {
        session()?.redo(&rel, generation).map(op_dto).map_err(err)
    }

    /// "Overwrite the disk version": keep the page as it is in memory.
    pub fn keep_mine(rel: String) -> Result<bool, String> {
        Ok(session()?.keep_mine(&rel))
    }

    pub fn dismiss_notice(id: u64) -> Result<(), String> {
        session()?.dismiss_notice(id);
        Ok(())
    }

    // -- derived data (sync) --

    pub fn search(query: String) -> Result<Vec<SearchHitDto>, String> {
        Ok(session()?.search(&query).into_iter().map(hit_dto).collect())
    }

    /// Completion candidates after a trigger: `kind` is `"page"` (for `[[`)
    /// or `"block"` (for `((`).
    pub fn complete(query: String, kind: String) -> Result<Vec<CompleteItemDto>, String> {
        Ok(session()?
            .complete(&query, &kind, COMPLETE_LIMIT)
            .into_iter()
            .map(|c| CompleteItemDto {
                kind: c.kind,
                insert: c.insert,
                label: c.label,
                detail: c.detail,
            })
            .collect())
    }

    pub fn backlinks(rel: String) -> Result<Vec<BacklinkGroupDto>, String> {
        let session = session()?;
        Ok(session
            .backlinks(&rel)
            .into_iter()
            .map(|group| group_dto(&session, group))
            .collect())
    }

    pub fn block(id: String) -> Result<Option<BlockInfoDto>, String> {
        Ok(session()?.block(block_id(&id)?).map(info_dto))
    }

    /// Non-journal pages, for the sidebar.
    pub fn pages() -> Result<Vec<PageEntryDto>, String> {
        Ok(session()?.pages().iter().map(entry_dto).collect())
    }

    /// The vault-relative path of a page title.
    pub fn rel_for_title(title: String) -> Result<String, String> {
        Ok(session()?.rel_for_title(&title))
    }

    /// What a `sx-page:` / `sx-block:` link from a block's Markdown points at.
    pub fn link(href: String) -> Option<LinkDto> {
        if let Some(rest) = href.strip_prefix(md::PAGE_SCHEME) {
            return Some(LinkDto {
                kind: "page".into(),
                target: md::decode(rest),
            });
        }
        href.strip_prefix(md::BLOCK_SCHEME).map(|rest| LinkDto {
            kind: "block".into(),
            target: md::decode(rest),
        })
    }

    /// A formula as an image, or `None` when it does not compile (show the
    /// TeX source then).
    pub fn render_math(tex: String, display: bool, dark: bool) -> Option<FormulaDto> {
        math::render(&tex, display, dark).map(|f| FormulaDto {
            uri: f.uri.clone(),
            width: f64::from(f.width),
            height: f64::from(f.height),
            baseline: f64::from(f.baseline),
        })
    }

    /// Remembers where the user is, for the next start. The session does not
    /// track the view: it is UI state, persisted here for the next boot.
    pub fn set_view(view: ViewDto) -> Result<(), String> {
        let view = view_of(&view);
        session()?;
        persist_view(&view);
        Ok(())
    }

    /// Test hook: boots a fresh vault seeded with the fixture `seed` (a folder
    /// under `RIKE_DEV_SEED`) and `files`. Call before the app starts.
    pub fn dev_reset(
        seed: Option<String>,
        files: Vec<DevFileDto>,
        view: Option<ViewDto>,
    ) -> Result<(), String> {
        if let Some(old) = SESSION.with(|s| s.borrow_mut().take()) {
            old.shutdown();
        }
        let files: Vec<(String, String)> = files.into_iter().map(|f| (f.path, f.text)).collect();
        session::dev_reset(seed.as_deref(), &files)?;
        if let Some(view) = view {
            session::dev_view(view_of(&view));
        }
        Ok(())
    }

    /// Test hook: a folder `name` with `files` inside the dev root; returns its path.
    pub fn dev_folder(name: String, files: Vec<DevFileDto>) -> Result<String, String> {
        let base = session::dev_root().ok_or("not a dev run")?;
        session::dev_guard(&base)?;
        let dir = session::dev_join(&base, &format!("f-{name}"))?;
        std::fs::create_dir_all(&dir).map_err(err)?;
        for f in files {
            let file = session::dev_join(&dir, &f.path)?;
            if let Some(parent) = file.parent() {
                std::fs::create_dir_all(parent).map_err(err)?;
            }
            std::fs::write(file, f.text).map_err(err)?;
        }
        Ok(path_text(&dir))
    }

    /// Test hook: boots with no vault, as a start with nothing to open does.
    pub fn dev_welcome(recent: Vec<String>, notice: Option<String>) -> Result<(), String> {
        if let Some(old) = SESSION.with(|s| s.borrow_mut().take()) {
            old.shutdown();
        }
        session::dev_welcome(recent, notice)
    }

    /// Test hook: a file of the dev vault, `None` when absent.
    pub fn dev_read(rel: String) -> Result<Option<String>, String> {
        match std::fs::read_to_string(dev_file(&rel)?) {
            Ok(text) => Ok(Some(text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }

    /// Test hook: writes a file of the dev vault behind the app's back.
    pub fn dev_write(rel: String, text: String) -> Result<(), String> {
        let file = dev_file(&rel)?;
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(file, text).map_err(|e| e.to_string())
    }
}

/// The path of `rel` inside the dev vault. Only works for a vault under
/// `RIKE_DEV_VAULT` (no host booted), so a real run can never reach these hooks.
fn dev_file(rel: &str) -> Result<PathBuf, String> {
    let base = session::dev_root().ok_or("not a dev run")?;
    let root = session::boot()
        .and_then(|b| b.vault)
        .map(|v| v.root().to_path_buf())
        .filter(|r| r.starts_with(&base))
        .ok_or("not the dev vault")?;
    session::dev_join(&root, rel)
}
