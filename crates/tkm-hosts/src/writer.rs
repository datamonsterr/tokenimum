//! Byte-preserving JSON hook installation shared across command-hook hosts.
//!
//! Settings are validated with `serde_json`, then edited by token spans. The
//! document is never reserialized. Backups preserve exact original bytes;
//! uninstall restores them only when doing so cannot discard foreign changes.

pub mod deny;
mod span;

use crate::{AdapterError, InstallReport};
use serde_json::{Value, json};
use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use tkm_proto::EventKind;

/// One native event and the complete caller-defined command handler to install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookSpec {
    /// Native host event name, such as `UserPromptSubmit`.
    pub native_event: String,
    /// Normalized kind encoded in the supplied handler's command line.
    pub kind: EventKind,
    /// Matcher for this group; absent for events without a matcher.
    pub matcher: Option<String>,
    /// Complete handler object, supporting exec and shell command forms.
    pub handler: Value,
}

pub(crate) fn invalid(message: &str) -> AdapterError {
    AdapterError::InvalidPayload {
        message: message.to_owned(),
    }
}

fn unsafe_target(path: &Path, reason: &str) -> AdapterError {
    AdapterError::UnsafeTarget {
        path: path.to_path_buf(),
        reason: reason.to_owned(),
    }
}

fn absolute(path: &Path) -> Result<PathBuf, AdapterError> {
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()?.join(path)
    };
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            _ => normalized.push(component.as_os_str()),
        }
    }
    Ok(normalized)
}

fn resolved(path: &Path) -> Result<PathBuf, AdapterError> {
    resolve_links(path, 0)
}

fn resolve_links(path: &Path, depth: usize) -> Result<PathBuf, AdapterError> {
    if depth > 40 {
        return Err(unsafe_target(path, "too many symlinks in target"));
    }
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            let link = fs::read_link(path)?;
            let destination = if link.is_absolute() {
                link
            } else {
                let parent = path
                    .parent()
                    .ok_or_else(|| unsafe_target(path, "symlink has no parent"))?;
                resolve_links(parent, depth + 1)?.join(link)
            };
            resolve_links(&absolute(&destination)?, depth + 1)
        }
        Ok(_) => Ok(fs::canonicalize(path)?),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            let parent = path
                .parent()
                .ok_or_else(|| unsafe_target(path, "target has no parent"))?;
            let name = path
                .file_name()
                .ok_or_else(|| unsafe_target(path, "target has no file name"))?;
            Ok(resolve_links(parent, depth + 1)?.join(name))
        }
        Err(error) => Err(error.into()),
    }
}

/// Resolves a write target and rejects lexical or symlink escapes from `root`.
///
/// Existing parent symlinks are checked, including when the final file is absent.
/// In-root symlinks are followed so atomic replacement preserves the symlink.
///
/// # Errors
/// Returns [`AdapterError::UnsafeTarget`] for escapes, or an I/O error when path
/// resolution fails. No directories or files are created.
pub fn safe_target(root: &Path, path: &Path) -> Result<PathBuf, AdapterError> {
    let root = absolute(root)?;
    let path = absolute(path)?;
    if !path.starts_with(&root) {
        return Err(unsafe_target(&path, "target is outside the permitted root"));
    }
    let real_root = resolved(&root)?;
    let real_path = resolved(&path)?;
    if !real_path.starts_with(real_root) {
        return Err(unsafe_target(
            &path,
            "symlink resolves outside the permitted root",
        ));
    }
    if fs::metadata(&real_path).is_ok_and(|metadata| !metadata.is_file()) {
        return Err(unsafe_target(&path, "target is not a regular file"));
    }
    Ok(real_path)
}

fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".tkm-bak");
    PathBuf::from(name)
}

pub(super) fn checked_backup(
    root: &Path,
    path: &Path,
    target: &Path,
) -> Result<(PathBuf, PathBuf), AdapterError> {
    let backup = backup_path(path);
    let resolved = safe_target(root, &backup)?;
    if resolved == target {
        return Err(unsafe_target(&backup, "backup aliases the settings file"));
    }
    Ok((backup, resolved))
}

pub(super) fn read_optional(path: &Path) -> Result<Option<String>, AdapterError> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn temporary_file(parent: &Path) -> Result<(PathBuf, fs::File), AdapterError> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    fs::create_dir_all(parent)?;
    loop {
        let temporary = parent.join(format!(
            ".tkm-{}-{}.tmp",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        match OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)
        {
            Ok(file) => return Ok((temporary, file)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
    }
}

/// A changed snapshot triggers one recomputation before any target is replaced.
pub(super) enum Update<T> {
    Done(T),
    Retry(Option<String>),
}

pub(super) fn retry_update<T>(
    mut snapshot: Option<String>,
    mut update: impl FnMut(Option<&str>) -> Result<Update<T>, AdapterError>,
) -> Result<T, AdapterError> {
    for _ in 0..2 {
        match update(snapshot.as_deref())? {
            Update::Done(result) => return Ok(result),
            Update::Retry(current) => snapshot = current,
        }
    }
    Err(io::Error::new(
        io::ErrorKind::WouldBlock,
        "settings changed twice while preparing an update; retry the operation",
    )
    .into())
}

/// Stage bytes, then compare immediately before publishing or deleting.
/// The callback publishes the first-write backup only after a successful check.
pub(super) fn atomic_update(
    path: &Path,
    snapshot: Option<&str>,
    replacement: Option<&str>,
    before_publish: impl FnOnce() -> Result<(), AdapterError>,
) -> Result<Update<()>, AdapterError> {
    let temporary = if let Some(text) = replacement {
        let parent = path
            .parent()
            .ok_or_else(|| unsafe_target(path, "target has no parent"))?;
        let (temporary, mut file) = temporary_file(parent)?;
        let staged = (|| -> Result<(), AdapterError> {
            if let Ok(metadata) = fs::metadata(path) {
                file.set_permissions(metadata.permissions())?;
            }
            file.write_all(text.as_bytes())?;
            file.sync_all()?;
            Ok(())
        })();
        drop(file);
        if let Err(error) = staged {
            let _ = fs::remove_file(&temporary);
            return Err(error);
        }
        Some(temporary)
    } else {
        None
    };
    let result = (|| -> Result<Update<()>, AdapterError> {
        let current = read_optional(path)?;
        if current.as_deref() != snapshot {
            return Ok(Update::Retry(current));
        }
        before_publish()?;
        if let Some(temporary) = &temporary {
            fs::rename(temporary, path)?;
        } else if snapshot.is_some() {
            fs::remove_file(path)?;
        }
        Ok(Update::Done(()))
    })();
    if let Some(temporary) = temporary {
        let _ = fs::remove_file(temporary);
    }
    result
}

pub(super) fn save_backup(path: &Path, original: &[u8], source: &Path) -> Result<(), AdapterError> {
    if path.exists() {
        return Ok(());
    }
    let parent = path
        .parent()
        .ok_or_else(|| unsafe_target(path, "backup has no parent"))?;
    let (temporary, mut file) = temporary_file(parent)?;
    let result = (|| -> Result<(), AdapterError> {
        match fs::metadata(source) {
            Ok(metadata) => file.set_permissions(metadata.permissions())?,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        file.write_all(original)?;
        file.sync_all()?;
        drop(file);
        // A hard link publishes the complete backup atomically without replacing
        // an existing first-write backup from a concurrent installer.
        match fs::hard_link(&temporary, path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
            Err(error) => Err(error.into()),
        }
    })();
    let cleanup = fs::remove_file(&temporary);
    result?;
    cleanup?;
    Ok(())
}

fn shell_words(command: &str) -> Option<Vec<String>> {
    let mut words = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut escaped = false;
    for character in command.chars() {
        if matches!(character, '\n' | '\r') {
            return None;
        }
        // Expansions still execute inside double quotes.
        if quote == Some('"') && matches!(character, '$' | '`') && !escaped {
            return None;
        }
        if escaped {
            word.push(character);
            escaped = false;
            continue;
        }
        if character == '\\' && quote != Some('\'') {
            escaped = true;
            continue;
        }
        if let Some(delimiter) = quote {
            if character == delimiter {
                quote = None;
            } else {
                word.push(character);
            }
        } else if matches!(character, '\'' | '"') {
            quote = Some(character);
        } else if character == '#' && word.is_empty() {
            return None;
        } else if character.is_whitespace() {
            if !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
        } else if matches!(
            character,
            ';' | '|'
                | '&'
                | '<'
                | '>'
                | '`'
                | '$'
                | '('
                | ')'
                | '*'
                | '?'
                | '['
                | ']'
                | '{'
                | '}'
                | '~'
        ) {
            return None; // Complex shell programs cannot establish ownership.
        } else {
            word.push(character);
        }
    }
    if escaped || quote.is_some() {
        return None;
    }
    if !word.is_empty() {
        words.push(word);
    }
    Some(words)
}

/// Recognizes tokenimum handlers for this host, in exec or simple shell form.
///
/// The command's program basename must be exactly `tkm-hook`, and its argument
/// vector must contain the adjacent pair `--host`, `host`. Complex shell programs
/// are conservatively treated as foreign.
#[must_use]
pub fn owns_handler(handler: &Value, host: &str) -> bool {
    let Some(command) = handler.get("command").and_then(Value::as_str) else {
        return false;
    };
    let (program, args) = if let Some(args) = handler.get("args") {
        let Some(args) = args.as_array() else {
            return false;
        };
        let Some(args) = args.iter().map(Value::as_str).collect::<Option<Vec<_>>>() else {
            return false;
        };
        (
            command.to_owned(),
            args.into_iter().map(str::to_owned).collect::<Vec<_>>(),
        )
    } else {
        let Some(words) = shell_words(command) else {
            return false;
        };
        let Some((program, args)) = words.split_first() else {
            return false;
        };
        (program.clone(), args.to_vec())
    };
    Path::new(&program)
        .file_name()
        .is_some_and(|name| name == "tkm-hook")
        && args
            .windows(2)
            .any(|pair| pair[0] == "--host" && pair[1] == host)
}

fn owns_group(group: &Value, host: &str) -> bool {
    group
        .get("hooks")
        .and_then(Value::as_array)
        .is_some_and(|handlers| {
            !handlers.is_empty() && handlers.iter().all(|handler| owns_handler(handler, host))
        })
}

fn group(spec: &HookSpec) -> Value {
    if let Some(matcher) = &spec.matcher {
        json!({"matcher":matcher,"hooks":[spec.handler]})
    } else {
        json!({"hooks":[spec.handler]})
    }
}

fn ensure_hooks(mut text: String) -> Result<String, AdapterError> {
    let root = span::parse(&text)?;
    if !root.object {
        return Err(invalid("settings must be an object"));
    }
    if let Some(hooks) = root.member("hooks") {
        if !hooks.object {
            return Err(invalid("hooks must be an object"));
        }
    } else {
        text = span::append_member(&text, &root, "hooks", &json!({}));
    }
    Ok(text)
}

fn install_text(original: &str, host: &str, specs: &[HookSpec]) -> Result<String, AdapterError> {
    let mut text = ensure_hooks(original.to_owned())?;
    for spec in specs {
        if !spec.handler.is_object() || !owns_handler(&spec.handler, host) {
            return Err(invalid("hook specification must contain an owned handler"));
        }
        let root = span::parse(&text)?;
        let hooks = root
            .member("hooks")
            .ok_or_else(|| invalid("hooks object missing"))?;
        let desired = group(spec);
        let Some(event) = hooks.member(&spec.native_event) else {
            text = span::append_member(&text, hooks, &spec.native_event, &json!([desired]));
            continue;
        };
        if !event.array {
            return Err(invalid("hook event must be an array"));
        }
        let mut keep = false;
        let mut remove = Vec::new();
        for (index, node) in event.elements.iter().enumerate() {
            let value: Value = serde_json::from_str(&text[node.range.clone()])?;
            if owns_group(&value, host) {
                if value == desired && !keep {
                    keep = true;
                } else {
                    remove.push(index);
                }
            }
        }
        for index in remove.into_iter().rev() {
            let root = span::parse(&text)?;
            let event = root
                .member("hooks")
                .and_then(|hooks| hooks.member(&spec.native_event))
                .ok_or_else(|| invalid("hook event missing"))?;
            text = span::remove_element(&text, event, index);
        }
        if !keep {
            let root = span::parse(&text)?;
            let event = root
                .member("hooks")
                .and_then(|hooks| hooks.member(&spec.native_event))
                .ok_or_else(|| invalid("hook event missing"))?;
            text = span::append_element(&text, event, &desired);
        }
    }
    Ok(text)
}

fn uninstall_text(
    original: &str,
    host: &str,
    backup: Option<&Value>,
) -> Result<String, AdapterError> {
    let mut text = original.to_owned();
    let root = span::parse(&text)?;
    if !root.object {
        return Err(invalid("settings must be an object"));
    }
    let Some(hooks) = root.member("hooks") else {
        return Ok(text);
    };
    if !hooks.object {
        return Err(invalid("hooks must be an object"));
    }
    let mut removed_owned = false;
    let events: Vec<_> = hooks
        .members
        .iter()
        .map(|member| member.key.clone())
        .collect();
    for name in events {
        let root = span::parse(&text)?;
        let event = root
            .member("hooks")
            .and_then(|hooks| hooks.member(&name))
            .ok_or_else(|| invalid("hook event missing"))?;
        if !event.array {
            return Err(invalid("hook event must be an array"));
        }
        let mut remove = Vec::new();
        for (index, node) in event.elements.iter().enumerate() {
            let value: Value = serde_json::from_str(&text[node.range.clone()])?;
            if owns_group(&value, host) {
                remove.push(index);
            }
        }
        if remove.is_empty() {
            continue;
        }
        removed_owned = true;
        for index in remove.into_iter().rev() {
            let root = span::parse(&text)?;
            let event = root
                .member("hooks")
                .and_then(|hooks| hooks.member(&name))
                .ok_or_else(|| invalid("hook event missing"))?;
            text = span::remove_element(&text, event, index);
        }
        let root = span::parse(&text)?;
        let hooks = root
            .member("hooks")
            .ok_or_else(|| invalid("hooks missing"))?;
        if hooks
            .member(&name)
            .is_some_and(|event| event.elements.is_empty())
            && backup
                .and_then(|value| value.get("hooks"))
                .and_then(|value| value.get(&name))
                .is_none()
        {
            text = span::remove_member(&text, hooks, &name);
        }
    }
    let root = span::parse(&text)?;
    if removed_owned
        && root
            .member("hooks")
            .is_some_and(|hooks| hooks.members.is_empty())
        && backup.and_then(|value| value.get("hooks")).is_none()
    {
        text = span::remove_member(&text, &root, "hooks");
    }
    Ok(text)
}

/// Installs host-owned matcher groups while retaining all foreign JSON bytes.
///
/// Changed reports list settings first and a newly created backup second.
/// A repeated install with equal handlers reports no changed files.
///
/// # Errors
/// Returns an adapter error for unsafe paths, invalid settings/specifications,
/// malformed JSON, or filesystem failures. Dry runs perform no writes.
pub fn install_hooks(
    root: &Path,
    path: &Path,
    host: &str,
    hooks: &[HookSpec],
    dry_run: bool,
) -> Result<InstallReport, AdapterError> {
    let target = safe_target(root, path)?;
    let (backup, backup_target) = checked_backup(root, path, &target)?;
    let original = read_optional(&target)?;
    if hooks.is_empty() {
        return Ok(InstallReport {
            dry_run,
            ..InstallReport::default()
        });
    }
    retry_update(original, |original| {
        let changed = install_text(original.unwrap_or("{}"), host, hooks)?;
        let mut report = InstallReport {
            dry_run,
            ..InstallReport::default()
        };
        if original == Some(changed.as_str()) {
            return Ok(Update::Done(report));
        }
        report.changed_files.push(path.to_path_buf());
        let needs_backup = !backup_target.exists();
        if needs_backup {
            report.changed_files.push(backup.clone());
        }
        if dry_run {
            return Ok(Update::Done(report));
        }
        match atomic_update(&target, original, Some(&changed), || {
            if needs_backup {
                save_backup(
                    &backup_target,
                    original.unwrap_or_default().as_bytes(),
                    &target,
                )?;
            }
            Ok(())
        })? {
            Update::Done(()) => Ok(Update::Done(report)),
            Update::Retry(current) => Ok(Update::Retry(current)),
        }
    })
}

/// Removes only groups whose every handler belongs to `host`.
///
/// Restores exact backup bytes when the edited settings equal the backup as
/// JSON. Settings created by tokenimum are deleted when only an empty object
/// remains. Foreign edits made since installation are retained.
///
/// # Errors
/// Returns an adapter error for unsafe paths, invalid settings, malformed JSON,
/// or filesystem failures. Dry runs perform no writes.
pub fn uninstall_hooks(
    root: &Path,
    path: &Path,
    host: &str,
    dry_run: bool,
) -> Result<InstallReport, AdapterError> {
    let target = safe_target(root, path)?;
    let (backup, backup_target) = checked_backup(root, path, &target)?;
    retry_update(read_optional(&target)?, |snapshot| {
        let mut report = InstallReport {
            dry_run,
            ..InstallReport::default()
        };
        let Some(original) = snapshot else {
            return Ok(Update::Done(report));
        };
        let backup_text = read_optional(&backup_target)?;
        let backup_value = backup_text
            .as_deref()
            .filter(|text| !text.is_empty())
            .map(serde_json::from_str::<Value>)
            .transpose()?;
        let changed = uninstall_text(original, host, backup_value.as_ref())?;
        let value: Value = serde_json::from_str(&changed)?;
        let restore = backup_value.as_ref() == Some(&value);
        let replacement = if restore {
            backup_text.as_deref().unwrap_or(&changed)
        } else {
            &changed
        };
        let delete = backup_text.as_deref() == Some("")
            && value.as_object().is_some_and(serde_json::Map::is_empty);
        let settings_changed = original != replacement || delete;
        if settings_changed {
            report.changed_files.push(path.to_path_buf());
        }
        if restore || delete {
            report.changed_files.push(backup.clone());
        }
        if dry_run {
            return Ok(Update::Done(report));
        }
        if settings_changed {
            match atomic_update(
                &target,
                snapshot,
                if delete { None } else { Some(replacement) },
                || Ok(()),
            )? {
                Update::Done(()) => {}
                Update::Retry(current) => return Ok(Update::Retry(current)),
            }
        }
        if restore || delete {
            fs::remove_file(&backup)?;
        }
        Ok(Update::Done(report))
    })
}

#[cfg(test)]
mod review_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]
    use super::*;

    #[test]
    fn compare_and_retry_recomputes_once_from_new_contents() {
        let mut snapshots = Vec::new();
        let result = retry_update(Some("first".into()), |snapshot| {
            snapshots.push(snapshot.map(str::to_owned));
            if snapshots.len() == 1 {
                Ok(Update::Retry(Some("foreign edit".into())))
            } else {
                Ok(Update::Done(snapshot.map(str::to_owned)))
            }
        })
        .unwrap();
        assert_eq!(result.as_deref(), Some("foreign edit"));
        assert_eq!(
            snapshots,
            vec![Some("first".into()), Some("foreign edit".into())]
        );
    }

    #[test]
    fn compare_and_retry_refuses_second_change_without_publishing() {
        let mut attempts = 0;
        let result: Result<(), AdapterError> = retry_update(Some("first".into()), |_| {
            attempts += 1;
            Ok(Update::Retry(Some(format!("foreign edit {attempts}"))))
        });
        assert_eq!(attempts, 2);
        let Err(AdapterError::Io(error)) = result else {
            panic!("expected conflict I/O error");
        };
        assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
        assert!(error.to_string().contains("changed"));
    }
    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "tkm-review-update-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir_all(&root).unwrap();
            Self(root)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn atomic_update_checks_disk_before_replace_or_delete() {
        let scratch = Scratch::new();
        let path = scratch.0.join("settings.json");
        for replacement in [Some("replacement"), None] {
            fs::write(&path, "foreign edit").unwrap();
            let update = atomic_update(&path, Some("stale"), replacement, || {
                panic!("must not publish backup on conflict")
            })
            .unwrap();
            let Update::Retry(snapshot) = update else {
                panic!("expected conflict");
            };
            assert_eq!(snapshot.as_deref(), Some("foreign edit"));
            assert_eq!(fs::read_to_string(&path).unwrap(), "foreign edit");
            assert_eq!(fs::read_dir(&scratch.0).unwrap().count(), 1);
        }
    }

    #[test]
    fn atomic_update_retries_preserve_foreign_bytes_and_abort_on_second_change() {
        let scratch = Scratch::new();
        let path = scratch.0.join("settings.json");
        fs::write(&path, "initial").unwrap();
        let mut attempt = 0;
        let result = retry_update(read_optional(&path).unwrap(), |snapshot| {
            attempt += 1;
            fs::write(&path, format!("foreign {attempt}")).unwrap();
            atomic_update(&path, snapshot, Some("ours"), || {
                panic!("must not publish on conflict")
            })
        });
        assert!(
            matches!(result, Err(AdapterError::Io(ref error)) if error.kind() == io::ErrorKind::WouldBlock)
        );
        assert_eq!(fs::read_to_string(&path).unwrap(), "foreign 2");
        assert_eq!(fs::read_dir(&scratch.0).unwrap().count(), 1);
    }
}
