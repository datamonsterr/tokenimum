#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use serde_json::Value;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use tkm_hosts::{AdapterError, HostAdapter, InstallContext, claude::ClaudeAdapter};
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "tkm-claude-install-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    fn context(&self) -> InstallContext {
        InstallContext {
            agent_home: self.0.clone(),
            ..InstallContext::default()
        }
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn assert_foreign_bytes_preserved(original: &[u8], changed: &[u8]) {
    // Deleting only inserted bytes must recover the original byte sequence.
    let mut cursor = 0;
    for &byte in changed {
        if original.get(cursor) == Some(&byte) {
            cursor += 1;
        }
    }
    assert_eq!(cursor, original.len());
}
fn assert_round_trip(original: &[u8]) {
    let temp = Scratch::new();
    let context = temp.context();
    let path = temp.0.join("settings.json");
    let backup = temp.0.join("settings.json.tkm-bak");
    fs::write(&path, original).unwrap();
    let adapter = ClaudeAdapter::new();
    let report = adapter.install(&context).unwrap();
    assert!(report.instructions.is_empty());
    assert_eq!(report.changed_files, vec![path.clone(), backup.clone()]);
    assert_eq!(fs::read(&backup).unwrap(), original);
    let installed = fs::read(&path).unwrap();
    assert_foreign_bytes_preserved(original, &installed);
    let config: Value = serde_json::from_slice(&installed).unwrap();
    for (event, kind) in [
        ("SessionStart", "session_start"),
        ("UserPromptSubmit", "prompt"),
        ("PostToolUse", "tool_done"),
        ("PostToolUseFailure", "tool_done"),
        ("Stop", "turn_end"),
        ("SessionEnd", "session_end"),
    ] {
        let groups = config["hooks"][event].as_array().unwrap();
        let own = groups
            .iter()
            .find(|v| v["hooks"][0]["command"] == "tkm-hook")
            .unwrap();
        assert_eq!(
            own["hooks"][0],
            serde_json::json!({"type":"command","command":"tkm-hook","args":["--host","claude","--event",kind],"timeout":if kind=="prompt"{3}else{1}})
        );
        assert_eq!(
            own.get("matcher").and_then(Value::as_str),
            matches!(event, "PostToolUse" | "PostToolUseFailure").then_some("*")
        );
    }
    assert!(adapter.install(&context).unwrap().changed_files.is_empty());
    assert_eq!(fs::read(&path).unwrap(), installed);
    adapter.uninstall(&context).unwrap();
    assert_eq!(fs::read(&path).unwrap(), original);
    assert!(!backup.exists());
    assert!(
        adapter
            .uninstall(&context)
            .unwrap()
            .changed_files
            .is_empty()
    );
}
#[test]
fn fixture_install_preserves_foreign_bytes_backup_idempotence_and_restore() {
    assert_round_trip(
        &fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/claude/config/settings.json"
        ))
        .unwrap(),
    );
}
#[test]
fn weird_json_tabs_compact_crlf_and_escaped_keys_round_trip() {
    for text in [
        "{\"theme\":\"界\",\"hooks\":{\"Stop\":[]}}",
        "{\r\n\t\"theme\" : \"dark\",\r\n\t\"hooks\" : {\r\n\t\t\"UserPromptSubmit\" : [ {\"hooks\":[{\"command\":\"foreign\"}]} ]\r\n\t}\r\n}\r\n",
        "{\"ho\\u006fks\":{},\"message\":\"} ] \\\"\"}",
        "{}",
    ] {
        assert_round_trip(text.as_bytes());
    }
}
#[test]
fn dry_run_writes_nothing_and_missing_settings_are_deleted_on_uninstall() {
    let temp = Scratch::new();
    let adapter = ClaudeAdapter::new();
    let mut context = temp.context();
    context.dry_run = true;
    assert!(!adapter.install(&context).unwrap().changed_files.is_empty());
    assert!(!temp.0.join("settings.json").exists());
    assert_eq!(fs::read_dir(&temp.0).unwrap().count(), 0);
    context.dry_run = false;
    adapter.install(&context).unwrap();
    let bytes = fs::read(temp.0.join("settings.json")).unwrap();
    context.dry_run = true;
    adapter.uninstall(&context).unwrap();
    assert_eq!(fs::read(temp.0.join("settings.json")).unwrap(), bytes);
    context.dry_run = false;
    adapter.uninstall(&context).unwrap();
    assert!(!temp.0.join("settings.json").exists());
}
#[cfg(unix)]
#[test]
fn settings_and_parent_symlinks_outside_root_are_refused() {
    use std::os::unix::fs::symlink;
    let temp = Scratch::new();
    let outside = Scratch::new();
    let target = outside.0.join("target.json");
    fs::write(&target, b"{}").unwrap();
    symlink(&target, temp.0.join("settings.json")).unwrap();
    let adapter = ClaudeAdapter::new();
    assert!(matches!(
        adapter.install(&temp.context()),
        Err(AdapterError::UnsafeTarget { .. })
    ));
    assert!(matches!(
        adapter.uninstall(&temp.context()),
        Err(AdapterError::UnsafeTarget { .. })
    ));
    assert_eq!(fs::read(&target).unwrap(), b"{}");
    fs::remove_file(temp.0.join("settings.json")).unwrap();
    symlink(&outside.0, temp.0.join(".claude")).unwrap();
    let mut context = temp.context();
    context.repo_root = Some(temp.0.clone());
    assert!(matches!(
        adapter.install(&context),
        Err(AdapterError::UnsafeTarget { .. })
    ));
}
#[test]
fn uninstall_preserves_foreign_changes_made_after_install() {
    let temp = Scratch::new();
    let path = temp.0.join("settings.json");
    fs::write(&path, b"{\"theme\":\"auto\"}").unwrap();
    let adapter = ClaudeAdapter::new();
    adapter.install(&temp.context()).unwrap();
    let text = fs::read_to_string(&path).unwrap().replace("auto", "dark");
    fs::write(&path, text).unwrap();
    adapter.uninstall(&temp.context()).unwrap();
    let result = fs::read_to_string(&path).unwrap();
    assert!(result.contains("\"theme\":\"dark\""));
    assert!(!result.contains("tkm-hook"));
}
#[test]
fn repo_scope_uses_local_settings() {
    let temp = Scratch::new();
    let mut context = temp.context();
    context.repo_root = Some(temp.0.clone());
    ClaudeAdapter::new().install(&context).unwrap();
    assert!(temp.0.join(".claude/settings.local.json").exists());
    assert!(!temp.0.join("settings.json").exists());
}

#[cfg(unix)]
#[test]
fn escaping_backup_symlinks_are_refused_and_in_root_settings_links_survive() {
    use std::os::unix::fs::symlink;
    let temp = Scratch::new();
    let outside = Scratch::new();
    let path = temp.0.join("settings.json");
    let backup = temp.0.join("settings.json.tkm-bak");
    fs::write(&path, b"{}").unwrap();
    fs::write(outside.0.join("secret"), b"private").unwrap();
    symlink(outside.0.join("secret"), &backup).unwrap();
    assert!(matches!(
        ClaudeAdapter::new().install(&temp.context()),
        Err(AdapterError::UnsafeTarget { .. })
    ));
    assert_eq!(fs::read(&path).unwrap(), b"{}");
    assert_eq!(fs::read(outside.0.join("secret")).unwrap(), b"private");
    fs::remove_file(&backup).unwrap();
    fs::rename(&path, temp.0.join("inner.json")).unwrap();
    symlink(temp.0.join("inner.json"), &path).unwrap();
    ClaudeAdapter::new().install(&temp.context()).unwrap();
    assert!(
        fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    ClaudeAdapter::new().uninstall(&temp.context()).unwrap();
    assert_eq!(fs::read(&path).unwrap(), b"{}");
    assert!(fs::symlink_metadata(path).unwrap().file_type().is_symlink());
}

#[cfg(unix)]
#[test]
fn dangling_escaping_settings_and_aliasing_backups_are_refused() {
    use std::os::unix::fs::symlink;
    let temp = Scratch::new();
    let outside = Scratch::new();
    let path = temp.0.join("settings.json");
    symlink(outside.0.join("missing.json"), &path).unwrap();
    assert!(matches!(
        ClaudeAdapter::new().install(&temp.context()),
        Err(AdapterError::UnsafeTarget { .. })
    ));
    assert!(!outside.0.join("missing.json").exists());
    fs::remove_file(&path).unwrap();
    fs::write(&path, b"{}").unwrap();
    symlink(&path, temp.0.join("settings.json.tkm-bak")).unwrap();
    assert!(matches!(
        ClaudeAdapter::new().uninstall(&temp.context()),
        Err(AdapterError::UnsafeTarget { .. })
    ));
    assert_eq!(fs::read(path).unwrap(), b"{}");
}
