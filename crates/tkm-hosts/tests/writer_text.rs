#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use tkm_hosts::{
    AdapterError,
    writer::{self, HookSpec},
};
use tkm_proto::EventKind;

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "tkm-writer-text-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&p).unwrap();
        Self(p)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn backup_of(path: &std::path::Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".tkm-bak");
    PathBuf::from(name)
}

// Callback signature is fixed by the writer API, which allows edits to fail.
#[allow(clippy::unnecessary_wraps)]
fn add_line(current: Option<&str>) -> Result<String, AdapterError> {
    let current = current.unwrap_or("");
    if current.contains("# tkm\n") {
        Ok(current.to_owned())
    } else {
        Ok(format!("{current}# tkm\n"))
    }
}
#[allow(clippy::unnecessary_wraps)]
fn strip_line(current: &str) -> Result<String, AdapterError> {
    Ok(current.replace("# tkm\n", ""))
}
#[allow(clippy::unnecessary_wraps)]
fn text_same(edited: &str, backup: &str) -> Result<bool, AdapterError> {
    Ok(edited == backup)
}
#[allow(clippy::unnecessary_wraps)]
fn text_empty(edited: &str) -> Result<bool, AdapterError> {
    Ok(edited.trim().is_empty())
}

// ---- text core ----

#[test]
fn text_install_creates_file_and_empty_marker() {
    let temp = Scratch::new();
    let path = temp.0.join("config.toml");
    let report = writer::install_text_file(&temp.0, &path, false, &add_line).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), "# tkm\n");
    assert_eq!(fs::read(backup_of(&path)).unwrap(), b"");
    assert_eq!(report.changed_files, vec![path.clone(), backup_of(&path)]);
    assert!(!report.dry_run);
}

#[test]
fn text_install_is_idempotent() {
    let temp = Scratch::new();
    let path = temp.0.join("config.toml");
    writer::install_text_file(&temp.0, &path, false, &add_line).unwrap();
    let again = writer::install_text_file(&temp.0, &path, false, &add_line).unwrap();
    assert_eq!(again.changed_files, Vec::<PathBuf>::new());
    assert_eq!(fs::read_to_string(&path).unwrap(), "# tkm\n");
}

#[test]
fn text_dry_run_writes_nothing() {
    let temp = Scratch::new();
    let path = temp.0.join("config.toml");
    let report = writer::install_text_file(&temp.0, &path, true, &add_line).unwrap();
    assert!(report.dry_run);
    assert_eq!(report.changed_files.len(), 2);
    assert!(!path.exists());
    assert!(!backup_of(&path).exists());
}

#[test]
fn text_uninstall_restores_exact_backup_bytes() {
    let temp = Scratch::new();
    let path = temp.0.join("config.toml");
    let original = "a   =  1\r\n# foreign\n";
    fs::write(&path, original).unwrap();
    writer::install_text_file(&temp.0, &path, false, &add_line).unwrap();
    assert_eq!(
        fs::read_to_string(backup_of(&path)).unwrap(),
        original,
        "backup holds the pre-install bytes"
    );
    let dry =
        writer::uninstall_text_file(&temp.0, &path, true, &strip_line, &text_same, &text_empty)
            .unwrap();
    assert!(dry.dry_run);
    assert!(fs::read_to_string(&path).unwrap().contains("# tkm"));
    let report =
        writer::uninstall_text_file(&temp.0, &path, false, &strip_line, &text_same, &text_empty)
            .unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
    assert!(!backup_of(&path).exists());
    assert!(report.changed_files.contains(&path));
}

#[test]
fn text_uninstall_deletes_created_file_and_marker() {
    let temp = Scratch::new();
    let path = temp.0.join("config.toml");
    writer::install_text_file(&temp.0, &path, false, &add_line).unwrap();
    writer::uninstall_text_file(&temp.0, &path, false, &strip_line, &text_same, &text_empty)
        .unwrap();
    assert!(!path.exists());
    assert!(!backup_of(&path).exists());
}

#[test]
fn text_uninstall_missing_file_is_empty_report() {
    let temp = Scratch::new();
    let path = temp.0.join("config.toml");
    let report =
        writer::uninstall_text_file(&temp.0, &path, false, &strip_line, &text_same, &text_empty)
            .unwrap();
    assert_eq!(report.changed_files, Vec::<PathBuf>::new());
}

#[cfg(unix)]
#[test]
fn text_install_refuses_symlink_escaping_root() {
    let temp = Scratch::new();
    let outside = Scratch::new();
    fs::write(outside.0.join("real.toml"), "x = 1\n").unwrap();
    std::os::unix::fs::symlink(outside.0.join("real.toml"), temp.0.join("config.toml")).unwrap();
    let result = writer::install_text_file(&temp.0, &temp.0.join("config.toml"), false, &add_line);
    assert!(matches!(result, Err(AdapterError::UnsafeTarget { .. })));
    assert_eq!(
        fs::read_to_string(outside.0.join("real.toml")).unwrap(),
        "x = 1\n"
    );
}

// ---- member ----

const FOREIGN_MEMBER: &str = "{\n  \"otherskill\": {\"a\": 1,  \"b\": [1,2]},\n  \"x\": 5\n}\n";

#[test]
fn member_install_keeps_foreign_bytes_and_is_idempotent() {
    let temp = Scratch::new();
    let path = temp.0.join("hooks.json");
    fs::write(&path, FOREIGN_MEMBER).unwrap();
    let value = json!({"SessionStart": [{"type": "command"}]});
    writer::install_member(&temp.0, &path, "tokenimum", &value, false).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("\"otherskill\": {\"a\": 1,  \"b\": [1,2]}"));
    assert!(text.contains("\"x\": 5"));
    let parsed: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(parsed["tokenimum"], value);
    let again = writer::install_member(&temp.0, &path, "tokenimum", &value, false).unwrap();
    assert_eq!(again.changed_files, Vec::<PathBuf>::new());
    assert_eq!(fs::read_to_string(&path).unwrap(), text);
}

#[test]
fn member_install_replaces_changed_value() {
    let temp = Scratch::new();
    let path = temp.0.join("hooks.json");
    fs::write(&path, FOREIGN_MEMBER).unwrap();
    writer::install_member(&temp.0, &path, "tokenimum", &json!({"v": 1}), false).unwrap();
    let report =
        writer::install_member(&temp.0, &path, "tokenimum", &json!({"v": 2}), false).unwrap();
    assert_eq!(report.changed_files, vec![path.clone()]);
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("\"otherskill\": {\"a\": 1,  \"b\": [1,2]}"));
    let parsed: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(parsed["tokenimum"], json!({"v": 2}));
    assert_eq!(text.matches("tokenimum").count(), 1);
}

#[test]
fn member_uninstall_restores_original_bytes() {
    let temp = Scratch::new();
    let path = temp.0.join("hooks.json");
    fs::write(&path, FOREIGN_MEMBER).unwrap();
    writer::install_member(&temp.0, &path, "tokenimum", &json!({"v": 1}), false).unwrap();
    writer::uninstall_member(&temp.0, &path, "tokenimum", false).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), FOREIGN_MEMBER);
    assert!(!backup_of(&path).exists());
}

#[test]
fn member_uninstall_keeps_foreign_edits_and_deletes_created_file() {
    let temp = Scratch::new();
    let path = temp.0.join("hooks.json");
    writer::install_member(&temp.0, &path, "tokenimum", &json!({"v": 1}), false).unwrap();
    writer::uninstall_member(&temp.0, &path, "tokenimum", false).unwrap();
    assert!(!path.exists());
    assert!(!backup_of(&path).exists());

    fs::write(&path, FOREIGN_MEMBER).unwrap();
    writer::install_member(&temp.0, &path, "tokenimum", &json!({"v": 1}), false).unwrap();
    let edited = fs::read_to_string(&path)
        .unwrap()
        .replace("\"x\": 5", "\"x\": 6");
    fs::write(&path, edited).unwrap();
    writer::uninstall_member(&temp.0, &path, "tokenimum", false).unwrap();
    let text = fs::read_to_string(&path).unwrap();
    assert!(!text.contains("tokenimum"));
    assert!(text.contains("\"x\": 6"));
    let again = writer::uninstall_member(&temp.0, &path, "tokenimum", false).unwrap();
    assert_eq!(again.changed_files, Vec::<PathBuf>::new());
}

#[test]
fn member_install_rejects_non_object_root() {
    let temp = Scratch::new();
    let path = temp.0.join("hooks.json");
    fs::write(&path, "[1]").unwrap();
    let result = writer::install_member(&temp.0, &path, "tokenimum", &json!(1), false);
    assert!(matches!(result, Err(AdapterError::InvalidPayload { .. })));
    assert_eq!(fs::read_to_string(&path).unwrap(), "[1]");
}

// ---- flat hooks ----

fn cursor_handler(kind: EventKind, timeout: u64) -> Value {
    json!({
        "command": format!("tkm-hook --host cursor --event {}", kind.as_str()),
        "timeout": timeout,
    })
}

fn specs() -> Vec<HookSpec> {
    vec![
        HookSpec {
            native_event: "sessionStart".into(),
            kind: EventKind::SessionStart,
            matcher: None,
            handler: cursor_handler(EventKind::SessionStart, 1),
        },
        HookSpec {
            native_event: "stop".into(),
            kind: EventKind::TurnEnd,
            matcher: None,
            handler: cursor_handler(EventKind::TurnEnd, 1),
        },
    ]
}

fn install_flat(root: &std::path::Path, path: &std::path::Path) -> tkm_hosts::InstallReport {
    writer::install_flat_hooks(
        root,
        path,
        "cursor",
        "hooks",
        &[("version", json!(1))],
        &specs(),
        false,
    )
    .unwrap()
}

fn uninstall_flat(root: &std::path::Path, path: &std::path::Path) -> tkm_hosts::InstallReport {
    writer::uninstall_flat_hooks(root, path, "cursor", "hooks", &["version"], false).unwrap()
}

const FOREIGN_HANDLER: &str = "{\n  \"version\": 1,\n  \"hooks\": {\n    \"sessionStart\": [\n      {\"command\": \"foreign.sh\",   \"timeout\": 9}\n    ]\n  }\n}\n";

#[test]
fn flat_install_keeps_foreign_handler_bytes_and_is_idempotent() {
    let temp = Scratch::new();
    let path = temp.0.join("hooks.json");
    fs::write(&path, FOREIGN_HANDLER).unwrap();
    install_flat(&temp.0, &path);
    let text = fs::read_to_string(&path).unwrap();
    assert!(text.contains("{\"command\": \"foreign.sh\",   \"timeout\": 9}"));
    let parsed: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(parsed["hooks"]["sessionStart"].as_array().unwrap().len(), 2);
    assert_eq!(parsed["hooks"]["sessionStart"][0]["command"], "foreign.sh");
    assert_eq!(parsed["hooks"]["stop"].as_array().unwrap().len(), 1);
    assert_eq!(text.matches("\"version\"").count(), 1);
    let again = install_flat(&temp.0, &path);
    assert_eq!(again.changed_files, Vec::<PathBuf>::new());
    assert_eq!(fs::read_to_string(&path).unwrap(), text);
}

#[test]
fn flat_install_seeds_version_only_when_absent() {
    let temp = Scratch::new();
    let path = temp.0.join("hooks.json");
    fs::write(&path, "{\"hooks\":{}}").unwrap();
    install_flat(&temp.0, &path);
    let parsed: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(parsed["version"], 1);

    let other = temp.0.join("other.json");
    fs::write(&other, "{\"version\":2}").unwrap();
    install_flat(&temp.0, &other);
    let parsed: Value = serde_json::from_str(&fs::read_to_string(&other).unwrap()).unwrap();
    assert_eq!(parsed["version"], 2);
    assert!(parsed["hooks"]["sessionStart"].is_array());
}

#[test]
fn flat_install_replaces_stale_owned_handlers() {
    let temp = Scratch::new();
    let path = temp.0.join("hooks.json");
    let stale = json!({"version": 1, "hooks": {"sessionStart": [
        cursor_handler(EventKind::SessionStart, 9),
        {"command": "foreign.sh"},
        cursor_handler(EventKind::SessionStart, 7),
    ]}});
    fs::write(&path, stale.to_string()).unwrap();
    install_flat(&temp.0, &path);
    let parsed: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    let handlers = parsed["hooks"]["sessionStart"].as_array().unwrap();
    assert_eq!(handlers.len(), 2);
    assert!(handlers.contains(&json!({"command": "foreign.sh"})));
    assert!(handlers.contains(&cursor_handler(EventKind::SessionStart, 1)));
}

#[test]
fn flat_uninstall_deletes_created_file() {
    let temp = Scratch::new();
    let path = temp.0.join("hooks.json");
    install_flat(&temp.0, &path);
    assert!(path.exists());
    uninstall_flat(&temp.0, &path);
    assert!(!path.exists());
    assert!(!backup_of(&path).exists());
}

#[test]
fn flat_uninstall_restores_original_bytes() {
    let temp = Scratch::new();
    let path = temp.0.join("hooks.json");
    fs::write(&path, FOREIGN_HANDLER).unwrap();
    install_flat(&temp.0, &path);
    uninstall_flat(&temp.0, &path);
    assert_eq!(fs::read_to_string(&path).unwrap(), FOREIGN_HANDLER);
}

#[test]
fn flat_uninstall_keeps_version_and_hooks_when_foreign_handlers_remain() {
    let temp = Scratch::new();
    let path = temp.0.join("hooks.json");
    fs::write(
        &path,
        "{\"hooks\":{\"sessionStart\":[{\"command\":\"foreign.sh\"}]}}",
    )
    .unwrap();
    install_flat(&temp.0, &path);
    let mut value: Value = serde_json::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
    value["hooks"]["stop"]
        .as_array_mut()
        .unwrap()
        .push(json!({"command": "other.sh"}));
    fs::write(&path, serde_json::to_string_pretty(&value).unwrap()).unwrap();
    uninstall_flat(&temp.0, &path);
    let text = fs::read_to_string(&path).unwrap();
    assert!(!text.contains("tkm-hook"));
    let parsed: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(parsed["version"], 1);
    assert_eq!(
        parsed["hooks"]["sessionStart"],
        json!([{"command": "foreign.sh"}])
    );
    assert_eq!(parsed["hooks"]["stop"], json!([{"command": "other.sh"}]));
}

#[test]
fn flat_install_rejects_invalid_shapes() {
    let temp = Scratch::new();
    let path = temp.0.join("hooks.json");
    for body in ["[]", "{\"hooks\":[]}", "{\"hooks\":{\"stop\":{}}}"] {
        fs::write(&path, body).unwrap();
        let result = writer::install_flat_hooks(
            &temp.0,
            &path,
            "cursor",
            "hooks",
            &[("version", json!(1))],
            &specs(),
            false,
        );
        assert!(matches!(result, Err(AdapterError::InvalidPayload { .. })));
        assert_eq!(fs::read_to_string(&path).unwrap(), body);
    }
    fs::write(&path, "{}").unwrap();
    let foreign = [HookSpec {
        native_event: "stop".into(),
        kind: EventKind::TurnEnd,
        matcher: None,
        handler: json!({"command": "foreign.sh"}),
    }];
    let result =
        writer::install_flat_hooks(&temp.0, &path, "cursor", "hooks", &[], &foreign, false);
    assert!(matches!(result, Err(AdapterError::InvalidPayload { .. })));
}
