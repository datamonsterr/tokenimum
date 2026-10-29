#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::Value;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use tkm_hosts::{HostAdapter, InstallContext, agy::AgyAdapter};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "tkm-agy-install-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn user_context(&self) -> InstallContext {
        InstallContext {
            agent_home: self.0.clone(),
            repo_root: None,
            dry_run: false,
        }
    }

    fn project_context(&self) -> InstallContext {
        InstallContext {
            agent_home: self.0.clone(),
            repo_root: Some(self.0.clone()),
            dry_run: false,
        }
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn assert_foreign_bytes_preserved(original: &[u8], changed: &[u8]) {
    let mut cursor = 0;
    for &byte in changed {
        if original.get(cursor) == Some(&byte) {
            cursor += 1;
        }
    }
    assert_eq!(cursor, original.len());
}

#[test]
fn fixture_install_preserves_foreign_bytes_idempotence_and_restore() {
    let original = fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/agy/config/hooks.json"
    ))
    .unwrap();

    let temp = Scratch::new();
    let context = temp.user_context();
    let config_dir = temp.0.join("config");
    fs::create_dir_all(&config_dir).unwrap();
    let path = config_dir.join("hooks.json");
    let backup = config_dir.join("hooks.json.tkm-bak");
    fs::write(&path, &original).unwrap();

    let adapter = AgyAdapter::new();

    // 1. Install
    let report = adapter.install(&context).unwrap();
    assert_eq!(report.instructions.len(), 0);
    assert_eq!(report.changed_files, vec![path.clone(), backup.clone()]);
    assert_eq!(fs::read(&backup).unwrap(), original);

    let installed = fs::read(&path).unwrap();
    assert_foreign_bytes_preserved(&original, &installed);

    let config: Value = serde_json::from_slice(&installed).unwrap();
    // Verify foreign "otherskill" is unchanged
    assert_eq!(config["otherskill"]["PreInvocation"][0]["type"], "command");
    assert_eq!(config["otherskill"]["PreInvocation"][0]["timeout"], 107);

    // Verify tokenimum entries
    let tokenimum = &config["tokenimum"];
    for (event, kind, timeout) in [
        ("SessionStart", "session_start", 1),
        ("PreInvocation", "prompt", 3),
        ("PostToolUse", "tool_done", 1),
        ("Stop", "turn_end", 1),
    ] {
        let handlers = tokenimum[event].as_array().unwrap();
        assert_eq!(handlers.len(), 1);
        let h = &handlers[0];
        assert_eq!(h["type"], "command");
        assert_eq!(h["command"], format!("tkm-hook --host agy --event {kind}"));
        assert_eq!(h["timeout"], timeout);
    }

    // 2. Idempotent install
    let second_report = adapter.install(&context).unwrap();
    assert_eq!(second_report.changed_files.len(), 0);
    assert_eq!(fs::read(&path).unwrap(), installed);

    // 3. Uninstall
    let uninstall_report = adapter.uninstall(&context).unwrap();
    assert_eq!(
        uninstall_report.changed_files,
        vec![path.clone(), backup.clone()]
    );
    assert_eq!(fs::read(&path).unwrap(), original);
    assert!(!backup.exists());

    // 4. Idempotent uninstall
    let second_uninstall = adapter.uninstall(&context).unwrap();
    assert_eq!(second_uninstall.changed_files.len(), 0);
}

#[test]
fn install_dry_run_does_not_modify_filesystem() {
    let temp = Scratch::new();
    let mut context = temp.user_context();
    context.dry_run = true;

    let config_dir = temp.0.join("config");
    fs::create_dir_all(&config_dir).unwrap();
    let path = config_dir.join("hooks.json");
    let original = b"{\"otherskill\":{}}";
    fs::write(&path, original).unwrap();

    let adapter = AgyAdapter::new();
    let report = adapter.install(&context).unwrap();
    assert!(report.dry_run);
    assert!(report.changed_files.contains(&path));
    // File content remains untouched
    assert_eq!(fs::read(&path).unwrap(), original);
    assert!(!config_dir.join("hooks.json.tkm-bak").exists());
}

#[test]
fn project_scoped_install_path() {
    let temp = Scratch::new();
    let context = temp.project_context();
    let agents_dir = temp.0.join(".agents");
    fs::create_dir_all(&agents_dir).unwrap();
    let path = agents_dir.join("hooks.json");
    let backup = agents_dir.join("hooks.json.tkm-bak");
    let original = b"{}";
    fs::write(&path, original).unwrap();

    let adapter = AgyAdapter::new();
    let report = adapter.install(&context).unwrap();
    assert_eq!(report.changed_files, vec![path.clone(), backup.clone()]);

    let installed: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert!(installed.get("tokenimum").is_some());

    adapter.uninstall(&context).unwrap();
    assert_eq!(fs::read(&path).unwrap(), original);
}
