#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use tkm_hosts::codewhale::CodewhaleAdapter;
use tkm_hosts::{AdapterError, HostAdapter, InstallContext};

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/codewhale/config"
);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "tkm-codewhale-install-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    fn with_fixtures() -> Self {
        let scratch = Self::new();
        fs::copy(
            format!("{FIXTURES}/config.toml"),
            scratch.0.join("config.toml"),
        )
        .unwrap();
        scratch
    }

    fn context(&self) -> InstallContext {
        InstallContext {
            agent_home: self.0.clone(),
            repo_root: None,
            dry_run: false,
        }
    }

    fn config_path(&self) -> PathBuf {
        self.0.join("config.toml")
    }

    fn backup_path(&self) -> PathBuf {
        self.0.join("config.toml.tkm-bak")
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn fixture_bytes(name: &str) -> Vec<u8> {
    fs::read(format!("{FIXTURES}/{name}")).unwrap()
}

/// Verifies that all bytes of `original` appear in `changed` in the same order.
fn assert_foreign_bytes_preserved(original: &[u8], changed: &[u8]) {
    let mut cursor = 0;
    for &byte in changed {
        if original.get(cursor) == Some(&byte) {
            cursor += 1;
        }
    }
    assert_eq!(
        cursor,
        original.len(),
        "some original foreign bytes were not preserved"
    );
}

#[test]
fn install_preserves_foreign_bytes_and_writes_backup() {
    let temp = Scratch::with_fixtures();
    let original = fixture_bytes("config.toml");
    let adapter = CodewhaleAdapter::new();

    let report = adapter.install(&temp.context()).unwrap();
    assert_eq!(
        report.changed_files,
        vec![temp.config_path(), temp.backup_path()]
    );
    assert!(!report.dry_run);

    let backup = fs::read(temp.backup_path()).unwrap();
    assert_eq!(backup, original);

    let changed = fs::read(temp.config_path()).unwrap();
    assert_foreign_bytes_preserved(&original, &changed);

    let text = String::from_utf8(changed).unwrap();
    assert!(text.contains("security-auditor --strict"));
    assert!(text.contains("tokenimum-session_start"));
    assert!(text.contains("tokenimum-message_submit"));
    assert!(text.contains("tokenimum-tool_call_before"));
    assert!(text.contains("tokenimum-tool_call_after"));
    assert!(text.contains("tokenimum-turn_end"));
    assert!(text.contains("tokenimum-session_end"));
    assert!(text.contains("timeout_secs = 3")); // For message_submit
    assert!(text.contains("timeout_secs = 1")); // For others
    assert!(text.contains("continue_on_error = true"));

    // Instructions should mention default_timeout_secs
    let instruction_text = report.instructions.join("\n");
    assert!(
        instruction_text.contains("default_timeout_secs"),
        "instruction missing note on default_timeout_secs"
    );
}

#[test]
fn install_is_idempotent() {
    let temp = Scratch::with_fixtures();
    let adapter = CodewhaleAdapter::new();

    // First install
    let report1 = adapter.install(&temp.context()).unwrap();
    assert_eq!(report1.changed_files.len(), 2);
    let after_first = fs::read(temp.config_path()).unwrap();

    // Second install
    let report2 = adapter.install(&temp.context()).unwrap();
    assert_eq!(
        report2.changed_files,
        Vec::<std::path::PathBuf>::new(),
        "second install should report no changes"
    );
    let after_second = fs::read(temp.config_path()).unwrap();
    assert_eq!(after_first, after_second);
}

#[test]
fn install_dry_run_does_not_modify_disk() {
    let temp = Scratch::with_fixtures();
    let original = fixture_bytes("config.toml");
    let adapter = CodewhaleAdapter::new();

    let mut ctx = temp.context();
    ctx.dry_run = true;
    let report = adapter.install(&ctx).unwrap();

    assert!(report.dry_run);
    assert_ne!(report.changed_files, [] as [std::path::PathBuf; 0]);
    assert!(!temp.backup_path().exists());
    assert_eq!(fs::read(temp.config_path()).unwrap(), original);
}

#[test]
fn uninstall_restores_original_bytes() {
    let temp = Scratch::with_fixtures();
    let original = fixture_bytes("config.toml");
    let adapter = CodewhaleAdapter::new();

    adapter.install(&temp.context()).unwrap();
    assert!(temp.backup_path().exists());

    let report = adapter.uninstall(&temp.context()).unwrap();
    assert_eq!(
        report.changed_files,
        vec![temp.config_path(), temp.backup_path()]
    );
    assert!(!temp.backup_path().exists());
    assert_eq!(fs::read(temp.config_path()).unwrap(), original);
}

#[test]
fn created_file_is_deleted_on_uninstall() {
    let temp = Scratch::new(); // config.toml does NOT exist
    let adapter = CodewhaleAdapter::new();

    assert!(!temp.config_path().exists());
    let report = adapter.install(&temp.context()).unwrap();
    assert!(temp.config_path().exists());
    assert!(temp.backup_path().exists());
    assert_eq!(
        report.changed_files,
        vec![temp.config_path(), temp.backup_path()]
    );

    let unreport = adapter.uninstall(&temp.context()).unwrap();
    assert_eq!(
        unreport.changed_files,
        vec![temp.config_path(), temp.backup_path()]
    );
    assert!(!temp.config_path().exists());
    assert!(!temp.backup_path().exists());
}

#[test]
fn instruction_when_hooks_disabled() {
    let temp = Scratch::new();
    let disabled_config = r"
[hooks]
enabled = false
default_timeout_secs = 20
";
    fs::write(temp.config_path(), disabled_config).unwrap();
    let adapter = CodewhaleAdapter::new();

    let report = adapter.install(&temp.context()).unwrap();
    let instruction_text = report.instructions.join("\n");
    assert!(
        instruction_text.contains("disabled") || instruction_text.contains("enabled = false"),
        "expected instruction about disabled hooks, got: {instruction_text}"
    );
}

#[test]
fn project_scope_is_unsupported() {
    let temp = Scratch::new();
    let adapter = CodewhaleAdapter::new();
    let mut ctx = temp.context();
    ctx.repo_root = Some(PathBuf::from("/home/user/project"));

    let err = adapter.install(&ctx).unwrap_err();
    match err {
        AdapterError::Unsupported { operation } => {
            assert!(
                operation.contains("codewhale project hooks need /hooks approve; use user scope"),
                "unexpected operation message: {operation}"
            );
        }
        other => panic!("expected Unsupported, got {other:?}"),
    }
}
