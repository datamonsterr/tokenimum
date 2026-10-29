#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use tkm_hosts::{AdapterError, HostAdapter, InstallContext, kimi::KimiAdapter};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/kimi/config");

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "tkm-kimi-install-{}-{}",
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
    let adapter = KimiAdapter::new();

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
    assert!(text.contains("foreign-logger --start"));
    assert!(text.contains("# tokenimum"));
    assert!(text.contains("tkm-hook --host kimi --event session_start"));
    assert!(text.contains("tkm-hook --host kimi --event prompt"));
    assert!(text.contains("tkm-hook --host kimi --event tool_done"));
    assert!(text.contains("tkm-hook --host kimi --event turn_end"));
    assert!(text.contains("tkm-hook --host kimi --event session_end"));
    assert!(text.contains("timeout = 3")); // For UserPromptSubmit
    assert!(text.contains("timeout = 5")); // For SessionEnd
    assert!(text.contains("timeout = 1")); // For others
}

#[test]
fn install_is_idempotent() {
    let temp = Scratch::with_fixtures();
    let adapter = KimiAdapter::new();

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
    let adapter = KimiAdapter::new();

    let mut ctx = temp.context();
    ctx.dry_run = true;
    let report = adapter.install(&ctx).unwrap();

    assert!(report.dry_run);
    assert_eq!(
        report.changed_files,
        vec![temp.config_path(), temp.backup_path()]
    );
    assert!(!temp.backup_path().exists());
    assert_eq!(fs::read(temp.config_path()).unwrap(), original);
}

#[test]
fn uninstall_restores_original_bytes() {
    let temp = Scratch::with_fixtures();
    let original = fixture_bytes("config.toml");
    let adapter = KimiAdapter::new();

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
    let adapter = KimiAdapter::new();

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
fn project_scope_is_unsupported() {
    let temp = Scratch::new();
    let adapter = KimiAdapter::new();
    let mut ctx = temp.context();
    ctx.repo_root = Some(PathBuf::from("/home/user/project"));

    let err = adapter.install(&ctx).unwrap_err();
    match err {
        AdapterError::Unsupported { operation } => {
            assert_eq!(operation, "kimi project-scope hooks");
        }
        other => panic!("expected Unsupported, got {other:?}"),
    }
}

#[test]
fn invalid_hooks_shape_returns_invalid_payload() {
    let temp = Scratch::new();
    fs::write(temp.config_path(), "hooks = \"invalid\"\n").unwrap();
    let adapter = KimiAdapter::new();

    let err = adapter.install(&temp.context()).unwrap_err();
    assert!(matches!(err, AdapterError::InvalidPayload { .. }));
}

#[test]
fn created_file_with_foreign_comment_is_kept_on_uninstall() {
    let temp = Scratch::new();
    let adapter = KimiAdapter::new();

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
    let adapter = KimiAdapter::new();

    adapter.install(&temp.context()).unwrap();
    let text = fs::read_to_string(temp.config_path()).unwrap();
    let marker = "[[hooks]]\nevent = \"UserPromptSubmit\"";
    assert!(text.contains(marker));
    let edited = text.replacen(marker, &format!("# keep me\n{marker}"), 1);
    fs::write(temp.config_path(), edited).unwrap();

    adapter.uninstall(&temp.context()).unwrap();
    let after = fs::read_to_string(temp.config_path()).unwrap();
    assert!(after.contains("# keep me"));
    assert!(after.contains("foreign-logger --start"));
    assert!(!after.contains("tkm-hook"));
}

#[test]
fn reinstall_keeps_comment_above_stale_owned_table() {
    let temp = Scratch::new();
    fs::write(
        temp.config_path(),
        "# keep me\n[[hooks]]\nevent = \"Stop\"\ncommand = \"tkm-hook --host kimi --event turn_end\"\ntimeout = 9\n",
    )
    .unwrap();
    let adapter = KimiAdapter::new();

    adapter.install(&temp.context()).unwrap();
    let after = fs::read_to_string(temp.config_path()).unwrap();
    assert!(after.contains("# keep me"));
    assert!(after.contains("tkm-hook --host kimi --event session_start"));
    assert!(!after.contains("timeout = 9"));
}

#[test]
fn comment_added_after_install_survives_uninstall() {
    let temp = Scratch::with_fixtures();
    let adapter = KimiAdapter::new();

    adapter.install(&temp.context()).unwrap();
    let mut text = fs::read_to_string(temp.config_path()).unwrap();
    text.push_str("# added later\n");
    fs::write(temp.config_path(), text).unwrap();

    adapter.uninstall(&temp.context()).unwrap();
    let after = fs::read_to_string(temp.config_path()).unwrap();
    assert!(after.contains("# added later"));
    assert!(after.contains("foreign-logger --start"));
    assert!(!after.contains("tkm-hook"));
}
