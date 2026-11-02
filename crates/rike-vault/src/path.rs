//! Validation and normalization of vault-relative paths.

use std::path::{Component, Path};

use crate::VaultError;

/// Directory names the vault never reads, lists, or writes.
const IGNORED_DIRS: [&str; 3] = ["logseq", ".git", ".rike"];

/// Validates a vault-relative page path and returns it unchanged.
pub(crate) fn validate(rel: &str) -> Result<&str, VaultError> {
    let invalid = || VaultError::InvalidPath(rel.to_string());

    if rel.is_empty()
        || rel.contains('\\')
        || rel.starts_with('/')
        || !rel.ends_with(".md")
        || rel == ".md"
    {
        return Err(invalid());
    }
    for component in rel.split('/') {
        if component.is_empty()
            || component == "."
            || component == ".."
            || is_ignored_dir(component)
        {
            return Err(invalid());
        }
    }
    Ok(rel)
}

/// Whether `rel` names a readable page file.
pub(crate) fn is_page(rel: &str) -> bool {
    validate(rel).is_ok()
}

/// Whether any component of `rel` is a directory the vault must not touch.
pub(crate) fn is_ignored(rel: &str) -> bool {
    rel.split('/').any(is_ignored_dir)
}

fn is_ignored_dir(component: &str) -> bool {
    IGNORED_DIRS.contains(&component)
}

/// Converts an absolute filesystem path into a vault-relative path.
///
/// Returns `None` for paths outside the root and for names that are not UTF-8.
pub(crate) fn rel_from_path(root: &Path, path: &Path) -> Option<String> {
    let rel = path.strip_prefix(root).ok()?;
    let mut parts = Vec::new();
    for component in rel.components() {
        match component {
            Component::Normal(part) => parts.push(part.to_str()?),
            _ => return None,
        }
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join("/"))
    }
}

/// Whether `rel` equals `prefix` or lives below it.
pub(crate) fn is_under(rel: &str, prefix: &str) -> bool {
    let prefix = prefix.trim_end_matches('/');
    if rel == prefix {
        return true;
    }
    if !rel.starts_with(prefix) {
        return false;
    }
    matches!(rel.as_bytes().get(prefix.len()), Some(b'/'))
}
