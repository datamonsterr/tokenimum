//! Claude's local deny list, with ownership supplied by the caller's store.

use super::{
    Update, atomic_update, checked_backup, invalid, read_optional, retry_update, safe_target, span,
    update_with_backup,
};
use crate::AdapterError;
use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

const RELATIVE: &str = ".claude/settings.local.json";
const EXCLUDE_LINE: &str = "/.claude/settings.local.json";

/// Result of reconciling a local deny list against a caller-owned set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DenyReport {
    /// Entries to persist as tokenimum-owned after this update.
    pub owned: BTreeSet<String>,
    /// Paths changed, or proposed for change during a dry run.
    pub changed_files: Vec<PathBuf>,
    /// Whether filesystem writes were disabled.
    pub dry_run: bool,
}

fn git_path(root: &Path, args: &[&str]) -> Result<PathBuf, AdapterError> {
    let output = Command::new("git").current_dir(root).args(args).output()?;
    if !output.status.success() {
        return Err(io::Error::other("git could not resolve repository metadata").into());
    }
    let text =
        String::from_utf8(output.stdout).map_err(|_| invalid("git metadata path is not UTF-8"))?;
    let path = PathBuf::from(text.trim_end_matches(['\r', '\n']));
    if path.as_os_str().is_empty() {
        return Err(invalid("git returned an empty metadata path"));
    }
    Ok(if path.is_absolute() {
        path
    } else {
        root.join(path)
    })
}

fn exclude_target(root: &Path) -> Result<PathBuf, AdapterError> {
    let path = git_path(root, &["rev-parse", "--git-path", "info/exclude"])?;
    let common = git_path(root, &["rev-parse", "--git-common-dir"])?;
    safe_target(&common, &path)
}

fn refuse_tracked(root: &Path, path: &Path) -> Result<(), AdapterError> {
    let output = Command::new("git")
        .current_dir(root)
        .args(["ls-files", "--error-unmatch", "--"])
        .arg(super::absolute(path)?)
        .output()?;
    if output.status.success() {
        return Err(AdapterError::UnsafeTarget {
            path: path.to_path_buf(),
            reason: "settings file is tracked by git".to_owned(),
        });
    }
    if output.status.code() != Some(1) {
        return Err(io::Error::other("git could not inspect tracked settings").into());
    }
    Ok(())
}

fn deny_entries(value: &Value) -> Result<Vec<String>, AdapterError> {
    if !value.is_object() {
        return Err(invalid("settings must be an object"));
    }
    let Some(permissions) = value.get("permissions") else {
        return Ok(Vec::new());
    };
    if !permissions.is_object() {
        return Err(invalid("permissions must be an object"));
    }
    let Some(deny) = permissions.get("deny") else {
        return Ok(Vec::new());
    };
    let Some(entries) = deny.as_array() else {
        return Err(invalid("permissions.deny must be an array"));
    };
    entries
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| invalid("deny entries must be strings"))
        })
        .collect()
}

fn ensure_deny(mut text: String) -> Result<String, AdapterError> {
    let root = span::parse(&text)?;
    if root.member("permissions").is_none() {
        text = span::append_member(&text, &root, "permissions", &json!({"deny":[]}));
    } else {
        let permissions = root
            .member("permissions")
            .ok_or_else(|| invalid("permissions missing"))?;
        if permissions.member("deny").is_none() {
            text = span::append_member(&text, permissions, "deny", &json!([]));
        }
    }
    Ok(text)
}

fn reconcile(
    text: &str,
    owned: &BTreeSet<String>,
    desired: &BTreeSet<String>,
) -> Result<(String, BTreeSet<String>), AdapterError> {
    let value: Value = serde_json::from_str(text)?;
    let entries = deny_entries(&value)?;
    let mut new_owned = BTreeSet::new();
    let additions: Vec<_> = desired
        .iter()
        .filter(|entry| !entries.contains(entry))
        .cloned()
        .collect();
    for entry in &entries {
        if owned.contains(entry) && desired.contains(entry) {
            new_owned.insert(entry.clone());
        }
    }
    let removals: Vec<_> = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| owned.contains(*entry) && !desired.contains(*entry))
        .map(|(index, _)| index)
        .collect();
    if additions.is_empty() && removals.is_empty() {
        return Ok((text.to_owned(), new_owned));
    }
    let mut changed = ensure_deny(text.to_owned())?;
    for index in removals.into_iter().rev() {
        let root = span::parse(&changed)?;
        let deny = root
            .member("permissions")
            .and_then(|permissions| permissions.member("deny"))
            .ok_or_else(|| invalid("deny list missing"))?;
        changed = span::remove_element(&changed, deny, index);
    }
    for entry in additions {
        let root = span::parse(&changed)?;
        let deny = root
            .member("permissions")
            .and_then(|permissions| permissions.member("deny"))
            .ok_or_else(|| invalid("deny list missing"))?;
        changed = span::append_element(&changed, deny, &Value::String(entry.clone()));
        new_owned.insert(entry);
    }
    Ok((changed, new_owned))
}

/// Reconciles `<repo>/.claude/settings.local.json` `permissions.deny`.
///
/// `owned` comes from the caller's store. Existing person-written entries are
/// never adopted, even when desired. Only owned entries no longer desired are
/// removed. The returned set replaces the store's ownership set after a write.
/// The local settings path is excluded through git's resolved `info/exclude`,
/// which also supports worktrees. Dry runs return proposed ownership and paths.
///
/// # Errors
/// Refuses tracked settings and escaping settings/backup/exclude symlinks.
/// Returns an adapter error for malformed settings, git failures, or I/O errors.
pub fn update_deny(
    repo: &Path,
    owned: &BTreeSet<String>,
    desired: &BTreeSet<String>,
    dry_run: bool,
) -> Result<DenyReport, AdapterError> {
    let path = repo.join(RELATIVE);
    let target = safe_target(repo, &path)?;
    refuse_tracked(repo, &path)?;
    refuse_tracked(repo, &target)?;
    let (backup, backup_target) = checked_backup(repo, &path, &target)?;
    let mut exclude_changed = false;
    retry_update(read_optional(&target)?, |original| {
        // Recheck the index too when retrying a concurrently changed document.
        refuse_tracked(repo, &path)?;
        refuse_tracked(repo, &target)?;
        let (changed, new_owned) = reconcile(original.unwrap_or("{}"), owned, desired)?;
        let settings_changed = original.unwrap_or("{}") != changed;
        let mut report = DenyReport {
            owned: new_owned,
            changed_files: Vec::new(),
            dry_run,
        };
        if original.is_none() && !settings_changed {
            return Ok(Update::Done(report));
        }
        let exclude = exclude_target(repo)?;
        let needs_backup = settings_changed && !backup_target.exists();
        if settings_changed {
            report.changed_files.push(path.clone());
        }
        if needs_backup {
            report.changed_files.push(backup.clone());
        }
        // Exclude before creating settings, so a partial write cannot expose it.
        exclude_changed |= update_exclude(&exclude, dry_run)?;
        if exclude_changed {
            report.changed_files.push(exclude);
        }
        if settings_changed && !dry_run {
            let backup_path = needs_backup.then_some(backup_target.as_path());
            match update_with_backup(&target, original, &changed, backup_path, || {})? {
                Update::Done(()) => {}
                Update::Retry(current) => return Ok(Update::Retry(current)),
            }
        }
        Ok(Update::Done(report))
    })
}

fn update_exclude(path: &Path, dry_run: bool) -> Result<bool, AdapterError> {
    retry_update(read_optional(path)?, |snapshot| {
        let previous = snapshot.unwrap_or_default();
        if previous.lines().any(|line| line == EXCLUDE_LINE) {
            return Ok(Update::Done(false));
        }
        let mut next = previous.to_owned();
        if !next.is_empty() && !next.ends_with('\n') {
            next.push('\n');
        }
        next.push_str(EXCLUDE_LINE);
        next.push('\n');
        if dry_run {
            return Ok(Update::Done(true));
        }
        match atomic_update(path, snapshot, Some(&next), || Ok(()))? {
            Update::Done(()) => Ok(Update::Done(true)),
            Update::Retry(current) => Ok(Update::Retry(current)),
        }
    })
}
