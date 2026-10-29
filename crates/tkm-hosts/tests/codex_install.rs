#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};
use tkm_hosts::codex::CodexAdapter;
use tkm_hosts::{AdapterError, HostAdapter, InstallContext};

const FIXTURES: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/codex/config");

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "tkm-codex-install-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }

    /// Scratch home holding copies of the fixture `hooks.json` and `config.toml`.
    fn with_fixtures() -> Self {
        let scratch = Self::new();
        for name in ["hooks.json", "config.toml"] {
            fs::copy(format!("{FIXTURES}/{name}"), scratch.0.join(name)).unwrap();
        }
        scratch
    }

    fn context(&self) -> InstallContext {
        InstallContext {
            agent_home: self.0.clone(),
            ..InstallContext::default()
        }
    }

    fn hooks(&self) -> PathBuf {
        self.0.join("hooks.json")
    }

    fn backup(&self) -> PathBuf {
        self.0.join("hooks.json.tkm-bak")
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

/// Deleting only inserted bytes from `changed` must recover `original`.
fn assert_foreign_bytes_preserved(original: &[u8], changed: &[u8]) {
    let mut cursor = 0;
    for &byte in changed {
        if original.get(cursor) == Some(&byte) {
            cursor += 1;
        }
    }
    assert_eq!(cursor, original.len());
}

fn handler(kind: &str, timeout: u64, context_limit: bool) -> Value {
    let mut handler = json!({
        "type": "command",
        "command": format!("tkm-hook --host codex --event {kind}"),
        "timeout": timeout,
    });
    if context_limit {
        handler["additionalContextLimit"] = json!(2500);
    }
    handler
}

#[test]
fn install_preserves_foreign_bytes_config_toml_and_writes_backup() {
    let temp = Scratch::with_fixtures();
    let original = fixture_bytes("hooks.json");
    let config = fixture_bytes("config.toml");
    let report = CodexAdapter::new().install(&temp.context()).unwrap();

    assert_eq!(report.changed_files, vec![temp.hooks(), temp.backup()]);
    assert!(!report.dry_run);
    assert_eq!(fs::read(temp.backup()).unwrap(), original);
    assert_foreign_bytes_preserved(&original, &fs::read(temp.hooks()).unwrap());
    assert_eq!(fs::read(temp.0.join("config.toml")).unwrap(), config);
    let text = report.instructions.join("\n");
    assert!(text.contains("/hooks"), "{text}");
    assert!(text.contains(&temp.hooks().display().to_string()), "{text}");
    assert!(text.contains("trust"), "{text}");
    let installed = fs::read_to_string(temp.hooks()).unwrap();
    assert!(!installed.contains("trusted_hash"));
}

#[test]
fn installed_handlers_have_the_exact_shape() {
    let temp = Scratch::with_fixtures();
    CodexAdapter::new().install(&temp.context()).unwrap();
    let config: Value = serde_json::from_slice(&fs::read(temp.hooks()).unwrap()).unwrap();

    let cases = [
        ("SessionStart", "session_start", 1, true, false),
        ("UserPromptSubmit", "prompt", 3, true, false),
        ("PostToolUse", "tool_done", 1, true, true),
        ("Stop", "turn_end", 1, false, false),
        ("SessionEnd", "session_end", 1, false, false),
    ];
    for (event, kind, timeout, limit, matcher) in cases {
        let groups = config["hooks"][event].as_array().unwrap();
        let own: Vec<&Value> = groups
            .iter()
            .filter(|group| {
                group["hooks"][0]["command"]
                    .as_str()
                    .is_some_and(|c| c.starts_with("tkm-hook"))
            })
            .collect();
        assert_eq!(own.len(), 1, "{event}");
        assert_eq!(
            own[0]["hooks"],
            json!([handler(kind, timeout, limit)]),
            "{event}"
        );
        assert_eq!(
            own[0].get("matcher").and_then(Value::as_str),
            matcher.then_some("*"),
            "{event}"
        );
    }
    // Foreign entries stay where they were.
    assert_eq!(config["hooks"]["PreToolUse"][0]["matcher"], "Bash");
    assert_eq!(
        config["hooks"]["UserPromptSubmit"][0]["hooks"][0]["timeout"],
        66
    );
    assert_eq!(config["description"], "Jev skill routing");
}

#[test]
fn second_install_changes_nothing() {
    let temp = Scratch::with_fixtures();
    let adapter = CodexAdapter::new();
    let first = adapter.install(&temp.context()).unwrap();
    let installed = fs::read(temp.hooks()).unwrap();
    let again = adapter.install(&temp.context()).unwrap();
    assert_eq!(again.changed_files, [] as [PathBuf; 0]);
    assert_eq!(fs::read(temp.hooks()).unwrap(), installed);
    assert!(again.instructions.join("\n").contains("/hooks"));
    assert_eq!(first.instructions, again.instructions);
}

#[test]
fn uninstall_restores_original_bytes_and_removes_backup() {
    let temp = Scratch::with_fixtures();
    let original = fixture_bytes("hooks.json");
    let config = fixture_bytes("config.toml");
    let adapter = CodexAdapter::new();
    adapter.install(&temp.context()).unwrap();
    let report = adapter.uninstall(&temp.context()).unwrap();

    assert!(report.changed_files.contains(&temp.hooks()));
    assert_eq!(fs::read(temp.hooks()).unwrap(), original);
    assert!(!temp.backup().exists());
    assert_eq!(fs::read(temp.0.join("config.toml")).unwrap(), config);
    let again = adapter.uninstall(&temp.context()).unwrap();
    assert_eq!(again.changed_files, [] as [PathBuf; 0]);
}

#[test]
fn uninstall_deletes_a_hooks_file_tokenimum_created() {
    let temp = Scratch::new();
    let adapter = CodexAdapter::new();
    adapter.install(&temp.context()).unwrap();
    assert!(temp.hooks().exists());
    assert_eq!(fs::read(temp.backup()).unwrap().len(), 0);
    adapter.uninstall(&temp.context()).unwrap();
    assert!(!temp.hooks().exists());
    assert!(!temp.0.join("config.toml").exists());
}

#[test]
fn dry_run_writes_nothing_but_reports_changes_and_trust_step() {
    let temp = Scratch::with_fixtures();
    let original = fixture_bytes("hooks.json");
    let mut context = temp.context();
    context.dry_run = true;
    let report = CodexAdapter::new().install(&context).unwrap();
    assert!(report.dry_run);
    assert_eq!(report.changed_files, vec![temp.hooks(), temp.backup()]);
    assert!(report.instructions.join("\n").contains("/hooks"));
    assert_eq!(fs::read(temp.hooks()).unwrap(), original);
    assert!(!temp.backup().exists());
    assert_eq!(fs::read_dir(&temp.0).unwrap().count(), 2);

    context.dry_run = false;
    CodexAdapter::new().install(&context).unwrap();
    let installed = fs::read(temp.hooks()).unwrap();
    context.dry_run = true;
    let removal = CodexAdapter::new().uninstall(&context).unwrap();
    assert!(removal.dry_run);
    assert_ne!(removal.changed_files.len(), 0);
    assert_eq!(fs::read(temp.hooks()).unwrap(), installed);
}

#[test]
fn project_scope_targets_dot_codex_hooks_json() {
    let home = Scratch::new();
    let repo = Scratch::new();
    let context = InstallContext {
        agent_home: home.0.clone(),
        repo_root: Some(repo.0.clone()),
        dry_run: false,
    };
    let adapter = CodexAdapter::new();
    let report = adapter.install(&context).unwrap();
    let path = repo.0.join(".codex/hooks.json");
    assert_eq!(
        report.changed_files,
        vec![path.clone(), repo.0.join(".codex/hooks.json.tkm-bak")]
    );
    assert!(path.exists());
    assert!(!home.hooks().exists());
    assert!(
        report
            .instructions
            .join("\n")
            .contains(&path.display().to_string())
    );
    adapter.uninstall(&context).unwrap();
    assert!(!path.exists());
}

#[cfg(unix)]
#[test]
fn hooks_json_symlink_outside_home_is_refused() {
    use std::os::unix::fs::symlink;
    let temp = Scratch::new();
    let outside = Scratch::new();
    let target = outside.0.join("target.json");
    fs::write(&target, b"{}").unwrap();
    symlink(&target, temp.hooks()).unwrap();
    let adapter = CodexAdapter::new();
    assert!(matches!(
        adapter.install(&temp.context()),
        Err(AdapterError::UnsafeTarget { .. })
    ));
    assert!(matches!(
        adapter.uninstall(&temp.context()),
        Err(AdapterError::UnsafeTarget { .. })
    ));
    assert_eq!(fs::read(&target).unwrap(), b"{}");
}
