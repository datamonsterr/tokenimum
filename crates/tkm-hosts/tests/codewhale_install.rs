#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use tkm_hosts::codewhale::CodewhaleAdapter;
use tkm_hosts::{HostAdapter, InstallContext};

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
fn project_scope_preserves_foreign_hooks_and_uses_project_shape() {
    let temp = Scratch::new();
    let path = temp.0.join(".codewhale/hooks.toml");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let original = "# user hooks\n[[hooks]]\nevent = \"turn_end\"\ncommand = \"echo foreign\"\n";
    fs::write(&path, original).unwrap();
    let mut ctx = temp.context();
    ctx.repo_root = Some(temp.0.clone());
    let adapter = CodewhaleAdapter::new();
    let report = adapter.install(&ctx).unwrap();
    assert!(
        report
            .instructions
            .iter()
            .any(|text| text.contains("/hooks approve"))
    );
    let text = fs::read_to_string(&path).unwrap();
    assert_foreign_bytes_preserved(original.as_bytes(), text.as_bytes());
    let doc: toml_edit::DocumentMut = text.parse().unwrap();
    assert_eq!(doc["hooks"].as_array_of_tables().unwrap().len(), 8);
    assert!(adapter.install(&ctx).unwrap().changed_files.is_empty());
    adapter.uninstall(&ctx).unwrap();
    assert_eq!(fs::read_to_string(path).unwrap(), original);
}

#[test]
fn created_file_with_foreign_comment_is_kept_on_uninstall() {
    let temp = Scratch::new();
    let adapter = CodewhaleAdapter::new();

    adapter.install(&temp.context()).unwrap();
    let mut text = fs::read_to_string(temp.config_path()).unwrap();
    text.push_str("# my own note\n");
    fs::write(temp.config_path(), text).unwrap();

    adapter.uninstall(&temp.context()).unwrap();
    assert!(temp.config_path().exists());
    let after = fs::read_to_string(temp.config_path()).unwrap();
    assert!(after.contains("# my own note"));
    assert!(!after.contains("tkm-hook"));
}

#[test]
fn comment_above_owned_table_survives_uninstall() {
    let temp = Scratch::with_fixtures();
    let adapter = CodewhaleAdapter::new();

    adapter.install(&temp.context()).unwrap();
    let text = fs::read_to_string(temp.config_path()).unwrap();
    let marker = "[[hooks.hooks]]\nevent = \"message_submit\"";
    assert!(text.contains(marker));
    let edited = text.replacen(marker, &format!("# keep me\n{marker}"), 1);
    fs::write(temp.config_path(), edited).unwrap();

    adapter.uninstall(&temp.context()).unwrap();
    let after = fs::read_to_string(temp.config_path()).unwrap();
    assert!(after.contains("# keep me"));
    assert!(after.contains("security-auditor --strict"));
    assert!(!after.contains("tkm-hook"));
}

#[test]
fn reinstall_keeps_comment_above_stale_owned_table() {
    let temp = Scratch::new();
    fs::write(
        temp.config_path(),
        "# keep me\n[[hooks.hooks]]\nevent = \"turn_end\"\ncommand = \"tkm-hook --host deepseek --event turn_end\"\ntimeout_secs = 9\n",
    )
    .unwrap();
    let adapter = CodewhaleAdapter::new();

    adapter.install(&temp.context()).unwrap();
    let after = fs::read_to_string(temp.config_path()).unwrap();
    assert!(after.contains("# keep me"));
    assert!(after.contains("tkm-hook --host deepseek --event session_start"));
    assert!(!after.contains("timeout_secs = 9"));
}

#[test]
fn comment_added_after_install_survives_uninstall() {
    let temp = Scratch::with_fixtures();
    let adapter = CodewhaleAdapter::new();

    adapter.install(&temp.context()).unwrap();
    let mut text = fs::read_to_string(temp.config_path()).unwrap();
    text.push_str("# added later\n");
    fs::write(temp.config_path(), text).unwrap();

    adapter.uninstall(&temp.context()).unwrap();
    let after = fs::read_to_string(temp.config_path()).unwrap();
    assert!(after.contains("# added later"));
    assert!(after.contains("security-auditor --strict"));
    assert!(!after.contains("tkm-hook"));
}

#[test]
fn install_registers_session_idle_with_its_native_event() {
    let temp = Scratch::new();
    CodewhaleAdapter::new().install(&temp.context()).unwrap();
    let text = fs::read_to_string(temp.config_path()).unwrap();
    let doc: toml_edit::DocumentMut = text.parse().unwrap();
    let hooks = doc["hooks"]["hooks"].as_array_of_tables().unwrap();
    let idle = hooks
        .iter()
        .find(|hook| hook["event"].as_str() == Some("session_idle"))
        .unwrap();
    assert_eq!(
        idle["command"].as_str(),
        Some("tkm-hook --host deepseek --event turn_end --host-event session_idle")
    );
    assert_eq!(idle["continue_on_error"].as_bool(), Some(true));
}
