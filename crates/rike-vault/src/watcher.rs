//! Filesystem watcher feeding raw path events into the vault actor.

use std::path::{Path, PathBuf};
use std::time::Duration;

use notify_debouncer_full::notify::{EventKind, RecommendedWatcher, RecursiveMode};
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer};
use tokio::sync::mpsc;

use crate::{VaultError, path};

/// A filesystem change reduced to what the actor needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RawPathEvent {
    pub(crate) rel: String,
    pub(crate) kind: RawPathKind,
}

/// Kind of change reported by the OS watcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum RawPathKind {
    Created,
    Removed,
    Changed,
}

/// A watcher event: either a subtree the actor should rescan, or a path change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RawEvent {
    Path(RawPathEvent),
    RescanDir(String),
    Rescan,
    Error(String),
}

/// Keeps the OS watcher alive. Dropping it stops the watcher thread.
pub(crate) struct WatchGuard {
    _debouncer: Debouncer<RecommendedWatcher, RecommendedCache>,
}

/// Starts a recursive watcher on `root`, forwarding events into `tx`.
pub(crate) fn start(
    root: &Path,
    debounce: Duration,
    tx: mpsc::UnboundedSender<RawEvent>,
) -> Result<WatchGuard, VaultError> {
    let canonical_root = root.to_path_buf();
    let mut debouncer =
        new_debouncer(
            debounce,
            None,
            move |result: DebounceEventResult| match result {
                Ok(events) => {
                    for event in events {
                        if event.need_rescan() {
                            // The OS dropped events; the caller must re-list the vault.
                            let _ = tx.send(RawEvent::Rescan);
                            continue;
                        }
                        translate(&canonical_root, &event.kind, &event.paths, &tx);
                    }
                }
                Err(errors) => {
                    for error in errors {
                        let _ = tx.send(RawEvent::Error(error.to_string()));
                    }
                }
            },
        )
        .map_err(|err| VaultError::Watcher(err.to_string()))?;

    debouncer
        .watch(root, RecursiveMode::Recursive)
        .map_err(|err| VaultError::Watcher(err.to_string()))?;

    Ok(WatchGuard {
        _debouncer: debouncer,
    })
}

fn translate(
    root: &Path,
    kind: &EventKind,
    paths: &[PathBuf],
    tx: &mpsc::UnboundedSender<RawEvent>,
) {
    if matches!(kind, EventKind::Access(_)) {
        return;
    }

    for path in paths {
        // Directory events carry no content, so rescan the subtree instead. This
        // also covers files that appeared before the watch was registered.
        if path.is_dir() {
            if matches!(kind, EventKind::Create(_) | EventKind::Modify(_))
                && let Some(rel) = path::rel_from_path(root, path)
                && !path::is_ignored(&rel)
            {
                let _ = tx.send(RawEvent::RescanDir(rel));
            }
            continue;
        }

        let Some(rel) = path::rel_from_path(root, path) else {
            continue;
        };
        if path::is_ignored(&rel) {
            continue;
        }

        let kind = match kind {
            EventKind::Create(_) => RawPathKind::Created,
            EventKind::Remove(_) => RawPathKind::Removed,
            EventKind::Modify(_) => RawPathKind::Changed,
            _ => continue,
        };
        let _ = tx.send(RawEvent::Path(RawPathEvent { rel, kind }));
    }
}
