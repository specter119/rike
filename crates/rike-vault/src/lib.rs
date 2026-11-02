//! Vault actor for a local note vault.
//!
//! Every read, write, backup, and watcher event for a vault is handled by a
//! single tokio task. Callers hold a [`VaultHandle`], which forwards commands to
//! that task and awaits the reply; filesystem changes made by other programs
//! arrive on a separate channel as [`VaultEvent`]s.
//!
//! The actor never depends on the page model: pages are plain `String`s here so
//! that parsing and serialization stay in `rike-core`.

mod actor;
mod path;
mod watcher;

use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use opendal::Operator;
use opendal::services::Fs;
use tokio::sync::{mpsc, oneshot};

use crate::actor::{Actor, Command, Reply};

/// Default debounce window for the file watcher.
const DEFAULT_DEBOUNCE: Duration = Duration::from_millis(250);
/// Default minimum gap between two backups of the same page.
const DEFAULT_BACKUP_INTERVAL: Duration = Duration::from_secs(10 * 60);
/// Default number of backups kept per page.
const DEFAULT_BACKUP_LIMIT: usize = 20;
/// Default number of self-write hashes remembered per page.
const DEFAULT_SELF_WRITE_LIMIT: usize = 8;

/// Content hash of a page, wrapping blake3.
#[derive(Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ContentHash([u8; 32]);

impl ContentHash {
    /// Hashes `bytes`.
    pub fn of(bytes: impl AsRef<[u8]>) -> Self {
        Self(*blake3::hash(bytes.as_ref()).as_bytes())
    }

    /// Raw digest bytes.
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Lowercase hex digest, handy for logs.
    pub fn to_hex(self) -> String {
        use std::fmt::Write as _;

        let mut out = String::with_capacity(64);
        for byte in self.0 {
            let _ = write!(out, "{byte:02x}");
        }
        out
    }
}

impl fmt::Debug for ContentHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.to_hex())
    }
}

/// Source of wall-clock time.
///
/// Injected through [`VaultOptions`] so tests can drive backup throttling and
/// conflict-copy timestamps without sleeping.
pub trait Clock: Send + Sync + 'static {
    /// Current wall-clock time.
    fn now(&self) -> SystemTime;
}

/// Wall-clock time taken from the operating system.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

/// Tuning knobs for an open vault. Defaults match the shipping save rules.
#[derive(Clone)]
pub struct VaultOptions {
    pub(crate) backup_interval: Duration,
    pub(crate) backup_limit: usize,
    pub(crate) debounce: Duration,
    pub(crate) self_write_limit: usize,
    clock: Arc<dyn Clock>,
}

impl Default for VaultOptions {
    fn default() -> Self {
        Self::new()
    }
}

impl VaultOptions {
    /// Default options: 10 minute backup interval, 20 backups per page, 250 ms debounce.
    pub fn new() -> Self {
        Self {
            backup_interval: DEFAULT_BACKUP_INTERVAL,
            backup_limit: DEFAULT_BACKUP_LIMIT,
            debounce: DEFAULT_DEBOUNCE,
            self_write_limit: DEFAULT_SELF_WRITE_LIMIT,
            clock: Arc::new(SystemClock),
        }
    }

    /// Minimum gap between two backups of the same page.
    pub fn with_backup_interval(mut self, interval: Duration) -> Self {
        self.backup_interval = interval;
        self
    }

    /// Number of backups kept per page; the oldest ones are pruned.
    pub fn with_backup_limit(mut self, limit: usize) -> Self {
        self.backup_limit = limit;
        self
    }

    /// Debounce window for the file watcher.
    pub fn with_debounce(mut self, debounce: Duration) -> Self {
        self.debounce = debounce;
        self
    }

    /// Clock used for backup throttling and timestamped file names.
    pub fn with_clock(mut self, clock: Arc<dyn Clock>) -> Self {
        self.clock = clock;
        self
    }

    pub(crate) fn now(&self) -> SystemTime {
        self.clock.now()
    }
}

/// Everything that can go wrong when talking to a vault.
#[derive(Debug, thiserror::Error)]
pub enum VaultError {
    /// The relative path is absolute, contains `..`, a backslash, a non-`.md`
    /// suffix, or points into an ignored directory.
    #[error("invalid vault path: {0}")]
    InvalidPath(String),
    /// The page file does not exist on disk.
    #[error("page not found: {0}")]
    NotFound(String),
    /// The file exists but is not valid UTF-8.
    #[error("invalid UTF-8 content in {0}")]
    InvalidUtf8(String),
    /// The vault root path cannot be expressed as UTF-8.
    #[error("vault root is not valid UTF-8: {0:?}")]
    NonUtf8Root(PathBuf),
    /// The actor task exited because every handle was dropped.
    #[error("vault actor is closed")]
    ActorClosed,
    /// The file watcher could not be started or reported an error.
    #[error("file watcher error: {0}")]
    Watcher(String),
    /// Local directory setup failed.
    #[error("vault io error: {0}")]
    Io(#[from] std::io::Error),
    /// A storage operation failed.
    #[error("storage error: {0}")]
    Storage(String),
}

impl From<opendal::Error> for VaultError {
    fn from(err: opendal::Error) -> Self {
        Self::Storage(err.to_string())
    }
}

/// Why a save was rejected before touching the disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectReason {
    /// The page is marked read-only because its self-check failed.
    ReadOnly,
}

/// Result of [`VaultHandle::save`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SaveOutcome {
    /// The content was written (or already matched the file).
    Saved {
        /// Hash of the content that is now on disk.
        hash: ContentHash,
    },
    /// The file changed on disk since it was loaded; the caller's content was
    /// preserved in a conflict copy instead of overwriting the disk version.
    Conflict {
        /// Current disk content, or `None` when the file was deleted externally.
        disk_content: Option<String>,
        /// Vault-relative path of the conflict copy holding the caller's content.
        conflict_copy: String,
    },
    /// The save was refused without any disk access.
    Rejected(RejectReason),
}

/// Filesystem notifications for an open vault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VaultEvent {
    /// A tracked page changed on disk (content differs from what the actor knew).
    Changed {
        /// Vault-relative path.
        rel: String,
        /// New content read from disk.
        content: String,
        /// Hash of `content`.
        hash: ContentHash,
    },
    /// A tracked page was deleted on disk.
    Removed {
        /// Vault-relative path.
        rel: String,
    },
    /// A page appeared on disk that the actor had never seen.
    Created {
        /// Vault-relative path.
        rel: String,
    },
    /// The watcher lost events (for example after a buffer overflow); callers
    /// should re-list the vault.
    Rescan,
    /// A watcher or refresh error that does not stop the actor.
    Error(String),
}

/// One markdown file found under `journals/` or `pages/`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageFile {
    /// Vault-relative path with `/` separators.
    pub rel: String,
    /// File size in bytes.
    pub len: u64,
    /// Last modification time, when the filesystem reports one.
    pub modified: Option<SystemTime>,
}

/// Content of a page read from disk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loaded {
    /// Page content as UTF-8 text.
    pub content: String,
    /// Hash of the bytes that were read.
    pub hash: ContentHash,
}

/// Handle to an open vault.
///
/// Cloning is cheap; all clones talk to the same actor task. Dropping the last
/// handle lets the actor finish queued work and then exit.
#[derive(Clone)]
pub struct VaultHandle {
    inner: Arc<HandleInner>,
}

struct HandleInner {
    root: PathBuf,
    commands: mpsc::UnboundedSender<Command>,
}

impl VaultHandle {
    /// Canonicalized vault root.
    pub fn root(&self) -> &Path {
        &self.inner.root
    }

    /// All `*.md` files under `journals/` and `pages/`.
    pub async fn list_pages(&self) -> Result<Vec<PageFile>, VaultError> {
        self.request(|reply| Command::ListPages { reply }).await
    }

    /// Reads a page and records its hash as the base hash for later saves.
    pub async fn load(&self, rel: &str) -> Result<Loaded, VaultError> {
        self.request(|reply| Command::Load {
            rel: rel.to_string(),
            reply,
        })
        .await
    }

    /// Reads `logseq/config.edn` if it exists. The file is not tracked as a page.
    pub async fn read_config_edn(&self) -> Result<Option<String>, VaultError> {
        self.request(|reply| Command::ReadConfig { reply }).await
    }

    /// Marks a page read-only (or writable again). Saves of read-only pages are rejected.
    pub async fn set_read_only(&self, rel: &str, read_only: bool) -> Result<(), VaultError> {
        self.request(|reply| Command::SetReadOnly {
            rel: rel.to_string(),
            read_only,
            reply,
        })
        .await
    }

    /// Saves a page, detecting external modifications and backing up the old content.
    pub async fn save(&self, rel: &str, content: String) -> Result<SaveOutcome, VaultError> {
        self.request(|reply| Command::Save {
            rel: rel.to_string(),
            content,
            reply,
        })
        .await
    }

    /// Writes the caller's version into `.rike/conflicts/` and returns the
    /// vault-relative path. The write is complete when this returns.
    pub async fn save_conflict_copy(
        &self,
        rel: &str,
        content: String,
    ) -> Result<String, VaultError> {
        self.request(|reply| Command::SaveConflictCopy {
            rel: rel.to_string(),
            content,
            reply,
        })
        .await
    }

    /// Waits until every command sent before this call has completed.
    pub async fn flush(&self) -> Result<(), VaultError> {
        self.request(|reply| Command::Flush { reply }).await
    }

    async fn request<T>(&self, command: impl FnOnce(Reply<T>) -> Command) -> Result<T, VaultError> {
        let (reply, response) = oneshot::channel();
        self.inner
            .commands
            .send(command(reply))
            .map_err(|_| VaultError::ActorClosed)?;
        response.await.map_err(|_| VaultError::ActorClosed)?
    }
}

/// Entry points for opening a vault.
pub struct Vault;

impl Vault {
    /// Opens (creating if needed) `root`, starts the actor task and the watcher.
    ///
    /// Must be called from inside a tokio runtime.
    pub async fn open(
        root: impl Into<PathBuf>,
    ) -> Result<(VaultHandle, mpsc::UnboundedReceiver<VaultEvent>), VaultError> {
        Self::open_with(root, VaultOptions::default()).await
    }

    /// Same as [`Vault::open`] with explicit options.
    pub async fn open_with(
        root: impl Into<PathBuf>,
        options: VaultOptions,
    ) -> Result<(VaultHandle, mpsc::UnboundedReceiver<VaultEvent>), VaultError> {
        let root = root.into();
        tokio::fs::create_dir_all(&root).await?;
        // Canonicalize before anything else so watcher paths compare equal with
        // the paths reported by the OS.
        let root = tokio::fs::canonicalize(&root).await?;

        let rike = root.join(".rike");
        let mut tmp = rike.join("tmp");
        for sub in ["tmp", "backups", "conflicts"] {
            let dir = rike.join(sub);
            tokio::fs::create_dir_all(&dir).await?;
            if sub == "tmp" {
                tmp = dir;
            }
        }

        let root_str = root
            .to_str()
            .ok_or_else(|| VaultError::NonUtf8Root(root.clone()))?
            .to_string();
        let tmp_str = tmp
            .to_str()
            .ok_or_else(|| VaultError::NonUtf8Root(tmp.clone()))?
            .to_string();

        let operator = Operator::new(Fs::default().root(&root_str).atomic_write_dir(&tmp_str))?;

        let (commands, command_rx) = mpsc::unbounded_channel();
        let (events, event_rx) = mpsc::unbounded_channel();
        let (raw_events, raw_event_rx) = mpsc::unbounded_channel();

        let watcher = watcher::start(&root, options.debounce, raw_events)?;
        let actor = Actor::new(root.clone(), operator, options, events);
        tokio::spawn(actor.run(command_rx, raw_event_rx, watcher));

        let handle = VaultHandle {
            inner: Arc::new(HandleInner { root, commands }),
        };
        Ok((handle, event_rx))
    }
}
