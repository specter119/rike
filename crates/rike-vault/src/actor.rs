//! The vault actor: one task that owns all vault state and performs all disk IO.

use std::collections::{HashMap, VecDeque};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use opendal::{ErrorKind, Operator};
use tokio::sync::{mpsc, oneshot};

use crate::watcher::{RawEvent, RawPathEvent, RawPathKind, WatchGuard};
use crate::{
    ContentHash, Loaded, PageFile, RejectReason, SaveOutcome, VaultError, VaultEvent, VaultOptions,
    path,
};

pub(crate) type Reply<T> = oneshot::Sender<Result<T, VaultError>>;

/// Commands accepted from [`crate::VaultHandle`].
pub(crate) enum Command {
    ListPages {
        reply: Reply<Vec<PageFile>>,
    },
    Load {
        rel: String,
        reply: Reply<Loaded>,
    },
    ReadConfig {
        reply: Reply<Option<String>>,
    },
    SetReadOnly {
        rel: String,
        read_only: bool,
        reply: Reply<()>,
    },
    Save {
        rel: String,
        content: String,
        reply: Reply<SaveOutcome>,
    },
    SaveConflictCopy {
        rel: String,
        content: String,
        reply: Reply<String>,
    },
    Flush {
        reply: Reply<()>,
    },
}

/// Bookkeeping for one page, created lazily on first load, save, or event.
#[derive(Default)]
struct PageState {
    /// Hash of the content last loaded, saved, or accepted from disk.
    /// `None` means the file did not exist at that point.
    base_hash: Option<ContentHash>,
    read_only: bool,
    last_backup_at: Option<SystemTime>,
    loaded_since_backup: bool,
    /// Hashes of our own recent writes; watcher events matching one are ignored.
    self_writes: VecDeque<ContentHash>,
}

impl PageState {
    /// Consumes a self-write mark matching `hash`, reporting whether it existed.
    fn consume_self_write(&mut self, hash: ContentHash) -> bool {
        if let Some(index) = self.self_writes.iter().position(|mark| *mark == hash) {
            self.self_writes.remove(index);
            true
        } else {
            false
        }
    }
}

pub(crate) struct Actor {
    root: PathBuf,
    op: Operator,
    options: VaultOptions,
    events: mpsc::UnboundedSender<VaultEvent>,
    pages: HashMap<String, PageState>,
}

impl Actor {
    pub(crate) fn new(
        root: PathBuf,
        op: Operator,
        options: VaultOptions,
        events: mpsc::UnboundedSender<VaultEvent>,
    ) -> Self {
        Self {
            root,
            op,
            options,
            events,
            pages: HashMap::new(),
        }
    }

    /// Runs until every handle is dropped, then exits.
    ///
    /// `_watcher` is owned here so the OS watcher lives exactly as long as the
    /// actor task does.
    pub(crate) async fn run(
        mut self,
        mut commands: mpsc::UnboundedReceiver<Command>,
        mut raw_events: mpsc::UnboundedReceiver<RawEvent>,
        _watcher: WatchGuard,
    ) {
        let mut watcher_open = true;
        loop {
            tokio::select! {
                command = commands.recv() => match command {
                    Some(command) => self.handle_command(command).await,
                    None => break,
                },
                event = raw_events.recv(), if watcher_open => match event {
                    Some(event) => self.handle_raw_event(event).await,
                    None => watcher_open = false,
                },
            }
        }
    }

    async fn handle_command(&mut self, command: Command) {
        match command {
            Command::ListPages { reply } => {
                let _ = reply.send(self.list_pages().await);
            }
            Command::Load { rel, reply } => {
                let _ = reply.send(self.load(&rel).await);
            }
            Command::ReadConfig { reply } => {
                let _ = reply.send(self.read_config().await);
            }
            Command::SetReadOnly {
                rel,
                read_only,
                reply,
            } => {
                let result = path::validate(&rel).map(|rel| {
                    self.pages.entry(rel.to_string()).or_default().read_only = read_only;
                });
                let _ = reply.send(result);
            }
            Command::Save {
                rel,
                content,
                reply,
            } => {
                let _ = reply.send(self.save(&rel, content).await);
            }
            Command::SaveConflictCopy {
                rel,
                content,
                reply,
            } => {
                let result = async {
                    let rel = path::validate(&rel)?;
                    self.write_conflict_copy(rel, &content).await
                }
                .await;
                let _ = reply.send(result);
            }
            Command::Flush { reply } => {
                let _ = reply.send(Ok(()));
            }
        }
    }

    async fn handle_raw_event(&mut self, event: RawEvent) {
        match event {
            RawEvent::Path(event) => self.handle_path_event(event).await,
            RawEvent::RescanDir(dir) => self.rescan_dir(&dir).await,
            RawEvent::Rescan => {
                let _ = self.events.send(VaultEvent::Rescan);
            }
            RawEvent::Error(message) => {
                let _ = self.events.send(VaultEvent::Error(message));
            }
        }
    }

    async fn list_pages(&self) -> Result<Vec<PageFile>, VaultError> {
        let mut pages = Vec::new();
        for dir in ["journals", "pages"] {
            let entries = self.op.list_with(dir).recursive(true).await?;
            for entry in entries {
                // `list` yields the directory itself (as a dir entry) plus children.
                if !entry.metadata().is_file() {
                    continue;
                }
                let rel = entry.path().to_string();
                if !path::is_page(&rel) {
                    continue;
                }
                // The fs lister only reports entry mode; ask for size and mtime.
                let metadata = match self.op.stat(&rel).await {
                    Ok(metadata) if metadata.is_file() => metadata,
                    Ok(_) => continue,
                    Err(err) if err.kind() == ErrorKind::NotFound => continue,
                    Err(err) => return Err(err.into()),
                };
                pages.push(PageFile {
                    rel,
                    len: metadata.content_length(),
                    modified: metadata.last_modified().map(SystemTime::from),
                });
            }
        }
        pages.sort_by(|a, b| a.rel.cmp(&b.rel));
        pages.dedup_by(|a, b| a.rel == b.rel);
        Ok(pages)
    }

    async fn load(&mut self, rel: &str) -> Result<Loaded, VaultError> {
        let rel = path::validate(rel)?;
        let Some((content, hash)) = self.read_optional(rel).await? else {
            return Err(VaultError::NotFound(rel.to_string()));
        };
        let state = self.pages.entry(rel.to_string()).or_default();
        state.base_hash = Some(hash);
        state.loaded_since_backup = false;
        Ok(Loaded { content, hash })
    }

    async fn read_config(&self) -> Result<Option<String>, VaultError> {
        match self.op.read("logseq/config.edn").await {
            Ok(buffer) => {
                let bytes = buffer.to_vec();
                String::from_utf8(bytes)
                    .map(Some)
                    .map_err(|_| VaultError::InvalidUtf8("logseq/config.edn".to_string()))
            }
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    async fn save(&mut self, rel: &str, content: String) -> Result<SaveOutcome, VaultError> {
        let rel = path::validate(rel)?;
        if self.pages.get(rel).is_some_and(|state| state.read_only) {
            return Ok(SaveOutcome::Rejected(RejectReason::ReadOnly));
        }

        let disk = self.read_optional(rel).await?;
        let disk_hash = disk.as_ref().map(|(_, hash)| *hash);
        let base_hash = self.pages.get(rel).and_then(|state| state.base_hash);
        let new_hash = ContentHash::of(content.as_bytes());

        if disk_hash != base_hash {
            // Someone else changed or deleted the file since we last agreed on it.
            let conflict_copy = self.write_conflict_copy(rel, &content).await?;
            self.pages.entry(rel.to_string()).or_default().base_hash = disk_hash;
            return Ok(SaveOutcome::Conflict {
                disk_content: disk.map(|(content, _)| content),
                conflict_copy,
            });
        }

        if disk_hash == Some(new_hash) {
            let state = self.pages.entry(rel.to_string()).or_default();
            state.base_hash = Some(new_hash);
            return Ok(SaveOutcome::Saved { hash: new_hash });
        }

        let old_content = disk.map(|(content, _)| content);
        self.write_page(rel, &content, old_content.as_deref())
            .await?;
        Ok(SaveOutcome::Saved { hash: new_hash })
    }

    /// Backs up the previous content when due, then writes `content` atomically.
    async fn write_page(
        &mut self,
        rel: &str,
        content: &str,
        old_content: Option<&str>,
    ) -> Result<(), VaultError> {
        let now = self.options.now();
        let interval = self.options.backup_interval;
        let due = match self.pages.get(rel) {
            Some(state) => {
                old_content.is_some()
                    && (!state.loaded_since_backup
                        || state.last_backup_at.is_none_or(|last| {
                            now.duration_since(last)
                                .map_or(true, |elapsed| elapsed >= interval)
                        }))
            }
            None => old_content.is_some(),
        };

        if due && let Some(old) = old_content {
            self.write_backup(rel, old).await?;
            self.pages
                .entry(rel.to_string())
                .or_default()
                .last_backup_at = Some(now);
        }

        self.op.write(rel, content.as_bytes().to_vec()).await?;

        let hash = ContentHash::of(content.as_bytes());
        let limit = self.options.self_write_limit;
        let state = self.pages.entry(rel.to_string()).or_default();
        state.base_hash = Some(hash);
        state.loaded_since_backup = true;
        state.self_writes.push_back(hash);
        while state.self_writes.len() > limit {
            state.self_writes.pop_front();
        }
        Ok(())
    }

    async fn write_backup(&self, rel: &str, content: &str) -> Result<(), VaultError> {
        let dir = format!(".rike/backups/{rel}");
        let path = self
            .next_timestamped_path(&dir, self.unix_seconds())
            .await?;
        self.op.write(&path, content.as_bytes().to_vec()).await?;
        self.prune_backups(&dir).await
    }

    /// Keeps only the newest `backup_limit` backups of one page.
    async fn prune_backups(&self, dir: &str) -> Result<(), VaultError> {
        let limit = self.options.backup_limit;
        let entries = self.op.list_with(dir).recursive(true).await?;
        let mut files: Vec<(u64, String)> = entries
            .iter()
            .filter(|entry| entry.metadata().is_file())
            .filter_map(|entry| {
                let path = entry.path().to_string();
                let stem = path.rsplit('/').next()?.strip_suffix(".md")?;
                Some((stem.parse::<u64>().ok()?, path))
            })
            .collect();
        if files.len() <= limit {
            return Ok(());
        }
        files.sort_by_key(|(ts, _)| *ts);
        let excess = files.len() - limit;
        for (_, path) in files.into_iter().take(excess) {
            self.op.delete(&path).await?;
        }
        Ok(())
    }

    async fn write_conflict_copy(&self, rel: &str, content: &str) -> Result<String, VaultError> {
        let mut ts = self.unix_seconds();
        loop {
            let candidate = format!(".rike/conflicts/{rel}.{ts}.md");
            match self.op.stat(&candidate).await {
                Ok(_) => ts += 1,
                Err(err) if err.kind() == ErrorKind::NotFound => {
                    self.op
                        .write(&candidate, content.as_bytes().to_vec())
                        .await?;
                    return Ok(candidate);
                }
                Err(err) => return Err(err.into()),
            }
        }
    }

    /// First free `<dir>/<ts>.md`, bumping the timestamp on collision so backup
    /// names stay parseable as numbers.
    async fn next_timestamped_path(&self, dir: &str, mut ts: u64) -> Result<String, VaultError> {
        loop {
            let candidate = format!("{dir}/{ts}.md");
            match self.op.stat(&candidate).await {
                Ok(_) => ts += 1,
                Err(err) if err.kind() == ErrorKind::NotFound => return Ok(candidate),
                Err(err) => return Err(err.into()),
            }
        }
    }

    async fn read_optional(&self, rel: &str) -> Result<Option<(String, ContentHash)>, VaultError> {
        match self.op.read(rel).await {
            Ok(buffer) => {
                let bytes = buffer.to_vec();
                let hash = ContentHash::of(&bytes);
                let content = String::from_utf8(bytes)
                    .map_err(|_| VaultError::InvalidUtf8(rel.to_string()))?;
                Ok(Some((content, hash)))
            }
            Err(err) if err.kind() == ErrorKind::NotFound => Ok(None),
            Err(err) => Err(err.into()),
        }
    }

    async fn handle_path_event(&mut self, event: RawPathEvent) {
        let rel = event.rel;
        if !path::is_page(&rel) {
            return;
        }
        match event.kind {
            RawPathKind::Removed => self.page_disappeared(&rel).await,
            RawPathKind::Created | RawPathKind::Changed => self.page_appeared(&rel).await,
        }
    }

    async fn page_appeared(&mut self, rel: &str) {
        self.refresh_page(rel, true).await;
    }

    async fn page_disappeared(&mut self, rel: &str) {
        self.refresh_page(rel, false).await;
    }

    /// Reconciles actor state with the current disk content of `rel`.
    ///
    /// `announce_creation` lets a first-seen file emit `Created` before `Changed`;
    /// rescan passes keep `Created` for genuinely new files only.
    async fn refresh_page(&mut self, rel: &str, announce_creation: bool) {
        let known = self
            .pages
            .get(rel)
            .is_some_and(|state| state.base_hash.is_some());
        let read = self.read_optional(rel).await;

        match read {
            Ok(Some((content, hash))) => {
                if let Some(state) = self.pages.get_mut(rel)
                    && (state.base_hash == Some(hash) || state.consume_self_write(hash))
                {
                    return;
                }

                let first_seen = !known;
                let state = self.pages.entry(rel.to_string()).or_default();
                state.base_hash = Some(hash);
                state.loaded_since_backup = false;
                state.self_writes.clear();

                if first_seen && announce_creation {
                    let _ = self.events.send(VaultEvent::Created {
                        rel: rel.to_string(),
                    });
                }
                let _ = self.events.send(VaultEvent::Changed {
                    rel: rel.to_string(),
                    content,
                    hash,
                });
            }
            Ok(None) => {
                if let Some(state) = self.pages.get_mut(rel)
                    && state.base_hash.is_some()
                {
                    state.base_hash = None;
                    state.loaded_since_backup = false;
                    state.self_writes.clear();
                    let _ = self.events.send(VaultEvent::Removed {
                        rel: rel.to_string(),
                    });
                }
            }
            Err(err) => {
                let _ = self.events.send(VaultEvent::Error(format!("{rel}: {err}")));
            }
        }
    }

    /// Rescans one subtree after a directory event; handles files that appeared
    /// before the watch was registered.
    async fn rescan_dir(&mut self, dir: &str) {
        let entries = match self.op.list_with(dir).recursive(true).await {
            Ok(entries) => entries,
            Err(err) if err.kind() == ErrorKind::NotFound => {
                self.forget_subtree(dir);
                return;
            }
            Err(err) => {
                let _ = self.events.send(VaultEvent::Error(format!("{dir}: {err}")));
                return;
            }
        };

        let mut seen = Vec::new();
        for entry in entries {
            if !entry.metadata().is_file() {
                continue;
            }
            let rel = entry.path().to_string();
            if !path::is_page(&rel) {
                continue;
            }
            seen.push(rel.clone());
            // Rescans never announce creations: a file that appeared inside a
            // directory before the watch existed may have shown up as `Created`
            // already, and tests poll for `Changed` regardless.
            self.refresh_page(&rel, false).await;
        }

        let lost: Vec<String> = self
            .pages
            .iter()
            .filter(|(rel, state)| {
                state.base_hash.is_some() && path::is_under(rel, dir) && !seen.contains(rel)
            })
            .map(|(rel, _)| rel.clone())
            .collect();
        for rel in lost {
            self.page_disappeared(&rel).await;
        }
    }

    /// Drops state for pages below `dir` (the directory itself is gone).
    fn forget_subtree(&mut self, dir: &str) {
        let lost: Vec<String> = self
            .pages
            .iter()
            .filter(|(rel, state)| state.base_hash.is_some() && path::is_under(rel, dir))
            .map(|(rel, _)| rel.clone())
            .collect();
        for rel in lost {
            if let Some(state) = self.pages.get_mut(&rel) {
                state.base_hash = None;
                state.loaded_since_backup = false;
                state.self_writes.clear();
            }
            let _ = self.events.send(VaultEvent::Removed { rel });
        }
    }

    fn unix_seconds(&self) -> u64 {
        self.options
            .now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| elapsed.as_secs())
            .unwrap_or(0)
    }
}

impl std::fmt::Debug for Actor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Actor")
            .field("root", &self.root)
            .field("pages", &self.pages.len())
            .finish()
    }
}
