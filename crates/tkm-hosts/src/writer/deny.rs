//! Host-local deny lists, with ownership supplied by the caller's store.

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

use tkm_core::model::Host;

#[derive(Clone, Copy)]
enum Surface {
    Permissions,
    Opencode,
    Pi,
}

impl Surface {
    fn keys(self) -> &'static [&'static str] {
        match self {
            Self::Permissions => &["permissions", "deny"],
            Self::Opencode => &["permission"],
            Self::Pi => &["defaultTools"],
        }
    }

    fn node(self, text: &str) -> Result<span::Node, AdapterError> {
        let mut node = span::parse(text)?;
        for key in self.keys() {
            node = node
                .members
                .into_iter()
                .rev()
                .find(|member| member.key == *key)
                .ok_or_else(|| invalid("deny container missing"))?
                .value;
        }
        Ok(node)
    }
}

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

pub(crate) fn exclude_target(root: &Path) -> Result<PathBuf, AdapterError> {
    let path = git_path(root, &["rev-parse", "--git-path", "info/exclude"])?;
    let common = git_path(root, &["rev-parse", "--git-common-dir"])?;
    safe_target(&common, &path)
}

pub(crate) fn refuse_tracked(root: &Path, path: &Path) -> Result<(), AdapterError> {
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

fn entries(value: &Value, surface: Surface) -> Result<Vec<String>, AdapterError> {
    let mut container = value;
    for key in surface.keys() {
        if !container.is_object() {
            return Err(invalid("deny settings parent must be an object"));
        }
        let Some(child) = container.get(key) else {
            return Ok(Vec::new());
        };
        container = child;
    }
    if matches!(surface, Surface::Opencode) {
        let object = container
            .as_object()
            .ok_or_else(|| invalid("permission must be an object"))?;
        return Ok(object
            .iter()
            .filter(|(_, value)| *value == "deny")
            .map(|(key, _)| key.clone())
            .collect());
    }
    container
        .as_array()
        .ok_or_else(|| invalid("deny settings must be an array"))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| invalid("deny entries must be strings"))
        })
        .collect()
}

fn ensure_container(mut text: String, surface: Surface) -> Result<String, AdapterError> {
    let root = span::parse(&text)?;
    match surface {
        Surface::Permissions => {
            if let Some(permissions) = root.member("permissions") {
                if permissions.member("deny").is_none() {
                    text = span::append_member(&text, permissions, "deny", &json!([]));
                }
            } else {
                text = span::append_member(&text, &root, "permissions", &json!({"deny":[]}));
            }
        }
        Surface::Opencode | Surface::Pi => {
            let key = surface.keys()[0];
            if root.member(key).is_none() {
                let empty = if matches!(surface, Surface::Opencode) {
                    json!({})
                } else {
                    json!([])
                };
                text = span::append_member(&text, &root, key, &empty);
            }
        }
    }
    Ok(text)
}

fn reconcile(
    text: &str,
    owned: &BTreeSet<String>,
    desired: &BTreeSet<String>,
    surface: Surface,
) -> Result<(String, BTreeSet<String>), AdapterError> {
    let value: Value = serde_json::from_str(text)?;
    let entries = entries(&value, surface)?;
    // A foreign allow, ask, or granular opencode rule is never replaced or adopted.
    let additions: Vec<_> = desired
        .iter()
        .filter(|entry| {
            !entries.contains(entry)
                && (!matches!(surface, Surface::Opencode)
                    || value
                        .get("permission")
                        .and_then(|map| map.get(*entry))
                        .is_none())
        })
        .cloned()
        .collect();
    let mut new_owned: BTreeSet<_> = owned
        .intersection(desired)
        .filter(|entry| entries.contains(entry))
        .cloned()
        .collect();
    let removals: Vec<_> = entries
        .iter()
        .enumerate()
        .filter(|(_, entry)| owned.contains(*entry) && !desired.contains(*entry))
        .map(|(index, entry)| (index, entry.clone()))
        .collect();
    if additions.is_empty() && removals.is_empty() {
        return Ok((text.to_owned(), new_owned));
    }
    let mut changed = ensure_container(text.to_owned(), surface)?;
    for (index, entry) in removals.into_iter().rev() {
        let container = surface.node(&changed)?;
        changed = if matches!(surface, Surface::Opencode) {
            span::remove_member(&changed, &container, &entry)
        } else {
            span::remove_element(&changed, &container, index)
        };
    }
    for entry in additions {
        let container = surface.node(&changed)?;
        changed = if matches!(surface, Surface::Opencode) {
            span::append_member(&changed, &container, &entry, &json!("deny"))
        } else {
            span::append_element(&changed, &container, &Value::String(entry.clone()))
        };
        new_owned.insert(entry);
    }
    Ok((changed, new_owned))
}

fn token(host: Host, name: &str) -> String {
    match host {
        Host::Cursor => name
            .strip_prefix("mcp__")
            .and_then(|name| name.split_once("__"))
            .map_or_else(
                || name.to_owned(),
                |(server, tool)| format!("Mcp({server}:{tool})"),
            ),
        Host::Opencode => name
            .strip_prefix("mcp__")
            .and_then(|name| name.split_once("__"))
            .map_or_else(
                || name.to_owned(),
                |(server, tool)| format!("{server}_{tool}"),
            ),
        Host::Pi => format!("-{name}"),
        _ => name.to_owned(),
    }
}

// Restore only empty containers that were absent before tokenimum's first write.
// In particular, Pi's absent defaultTools inherits tools, but [] disables them.
fn restore_containers(
    mut text: String,
    backup: &str,
    surface: Surface,
) -> Result<String, AdapterError> {
    let baseline: Value = serde_json::from_str(if backup.is_empty() { "{}" } else { backup })?;
    let value: Value = serde_json::from_str(&text)?;
    match surface {
        Surface::Permissions => {
            if value
                .pointer("/permissions/deny")
                .and_then(Value::as_array)
                .is_some_and(Vec::is_empty)
                && baseline.pointer("/permissions/deny").is_none()
            {
                let root = span::parse(&text)?;
                if let Some(permissions) = root.member("permissions") {
                    text = span::remove_member(&text, permissions, "deny");
                }
            }
            let root = span::parse(&text)?;
            if let Some(permissions) = root.member("permissions")
                && permissions.members.is_empty()
                && baseline.get("permissions").is_none()
            {
                text = span::remove_member(&text, &root, "permissions");
            }
        }
        Surface::Opencode | Surface::Pi => {
            let key = surface.keys()[0];
            let empty = value.get(key).is_some_and(|value| {
                value.as_array().is_some_and(Vec::is_empty)
                    || value.as_object().is_some_and(serde_json::Map::is_empty)
            });
            if empty && baseline.get(key).is_none() {
                let root = span::parse(&text)?;
                text = span::remove_member(&text, &root, key);
            }
        }
    }
    Ok(text)
}

/// Reconciles a verified host's project deny settings, preserving foreign entries.
///
/// Ownership uses catalog names; Cursor MCP names and Pi exclusions are encoded
/// for their host. Dry runs do not write. The original file is backed up once,
/// and both settings and backup paths are excluded from git.
///
/// # Errors
/// Refuses unsupported hosts, tracked settings, escaping symlinks, malformed
/// settings, and failed git or filesystem operations.
pub fn update_host_deny(
    repo: &Path,
    host: Host,
    owned: &BTreeSet<String>,
    desired: &BTreeSet<String>,
    dry_run: bool,
) -> Result<DenyReport, AdapterError> {
    let (relative, surface) = match host {
        Host::Claude => (crate::claude::DENY_PATH, Surface::Permissions),
        Host::Cursor => (crate::cursor::DENY_PATH, Surface::Permissions),
        Host::Opencode => (crate::opencode::DENY_PATH, Surface::Opencode),
        Host::Pi => (crate::pi::DENY_PATH, Surface::Pi),
        _ => return Err(invalid("host has no verified project deny target")),
    };
    let encoded_owned = owned.iter().map(|name| token(host, name)).collect();
    let encoded_desired = desired.iter().map(|name| token(host, name)).collect();
    let mut report = update_surface(
        repo,
        relative,
        surface,
        &encoded_owned,
        &encoded_desired,
        dry_run,
    )?;
    report.owned = desired
        .iter()
        .filter(|name| report.owned.contains(&token(host, name)))
        .cloned()
        .collect();
    Ok(report)
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
    update_host_deny(repo, Host::Claude, owned, desired, dry_run)
}

fn update_surface(
    repo: &Path,
    relative: &str,
    surface: Surface,
    owned: &BTreeSet<String>,
    desired: &BTreeSet<String>,
    dry_run: bool,
) -> Result<DenyReport, AdapterError> {
    let path = repo.join(relative);
    let target = safe_target(repo, &path)?;
    refuse_tracked(repo, &path)?;
    refuse_tracked(repo, &target)?;
    let (backup, backup_target) = checked_backup(repo, &path, &target)?;
    refuse_tracked(repo, &backup)?;
    refuse_tracked(repo, &backup_target)?;
    let mut exclude_changed = false;
    retry_update(read_optional(&target)?, |original| {
        // Recheck the index too when retrying a concurrently changed document.
        refuse_tracked(repo, &path)?;
        refuse_tracked(repo, &target)?;
        let (mut changed, new_owned) =
            reconcile(original.unwrap_or("{}"), owned, desired, surface)?;
        let mut delete = false;
        if new_owned.is_empty()
            && !owned.is_empty()
            && original.is_some_and(|original| original != changed)
        {
            let backup_text = read_optional(&backup_target)?
                .ok_or_else(|| invalid("deny backup missing; cannot safely restore settings"))?;
            changed = restore_containers(changed, &backup_text, surface)?;
            delete = backup_text.is_empty() && span::parse(&changed)?.members.is_empty();
        }
        let settings_changed = delete || original.unwrap_or("{}") != changed;
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
        exclude_changed |= update_exclude(&exclude, &format!("/{relative}"), dry_run)?;
        exclude_changed |= update_exclude(&exclude, &format!("/{relative}.tkm-bak"), dry_run)?;
        if exclude_changed {
            report.changed_files.push(exclude);
        }
        if settings_changed && !dry_run {
            let backup_path = needs_backup.then_some(backup_target.as_path());
            let update = if delete {
                atomic_update(&target, original, None, || Ok(()))?
            } else {
                update_with_backup(&target, original, &changed, backup_path, || {})?
            };
            match update {
                Update::Done(()) => {}
                Update::Retry(current) => return Ok(Update::Retry(current)),
            }
        }
        Ok(Update::Done(report))
    })
}

/// Appends `line` to the exclude file once; returns whether it changed (or would).
pub(crate) fn update_exclude(path: &Path, line: &str, dry_run: bool) -> Result<bool, AdapterError> {
    retry_update(read_optional(path)?, |snapshot| {
        let previous = snapshot.unwrap_or_default();
        if previous.lines().any(|l| l == line) {
            return Ok(Update::Done(false));
        }
        let mut next = previous.to_owned();
        if !next.is_empty() && !next.ends_with('\n') {
            next.push('\n');
        }
        next.push_str(line);
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
