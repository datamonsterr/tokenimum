#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use serde_json::{Value, json};
use std::{
    collections::BTreeSet,
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};
use tkm_hosts::{
    AdapterError,
    writer::{self, HookSpec, deny::update_deny},
};
use tkm_proto::EventKind;
struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "tkm-writer-{}-{}",
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
fn init_git(temp: &Scratch) -> bool {
    match Command::new("git")
        .arg("init")
        .arg("--quiet")
        .arg(&temp.0)
        .status()
    {
        Ok(s) => {
            assert!(s.success());
            true
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            eprintln!("SKIP: git is unavailable; prune git safety test not run");
            false
        }
        Err(e) => panic!("git init: {e}"),
    }
}
fn set(entries: &[&str]) -> BTreeSet<String> {
    entries.iter().map(|s| (*s).to_owned()).collect()
}
#[test]
fn shell_handler_ownership_is_host_specific_and_mixed_groups_survive() {
    let temp = Scratch::new();
    let path = temp.0.join("hooks.json");
    let original = r#"{"hooks":{"Stop":[{"hooks":[{"command":"/bin/tkm-hook --host codex --event turn_end"},{"command":"foreign"}]},{"hooks":[{"command":"tkm-hook --host claude --event turn_end"}]},{"hooks":[{"command":"echo tkm-hook --host codex"}]}]}}"#;
    fs::write(&path, original).unwrap();
    let hooks = [HookSpec {
        native_event: "Stop".into(),
        kind: EventKind::TurnEnd,
        matcher: None,
        handler: json!({"type":"command","command":"tkm-hook --host codex --event turn_end","additionalContextLimit":2500}),
    }];
    writer::install_hooks(&temp.0, &path, "codex", &hooks, false).unwrap();
    let bytes = fs::read(&path).unwrap();
    assert!(String::from_utf8_lossy(&bytes).contains("additionalContextLimit"));
    assert!(
        writer::install_hooks(&temp.0, &path, "codex", &hooks, false)
            .unwrap()
            .changed_files
            .is_empty()
    );
    writer::uninstall_hooks(&temp.0, &path, "codex", false).unwrap();
    assert_eq!(fs::read_to_string(&path).unwrap(), original);
}
#[test]
fn prune_owned_set_preserves_person_rules_and_excludes_once() {
    let temp = Scratch::new();
    if !init_git(&temp) {
        return;
    }
    let path = temp.0.join(".claude/settings.local.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let original = fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/claude/config/settings.local.json"
    ))
    .unwrap();
    fs::write(&path, &original).unwrap();
    let desired = set(&["WebFetch", "UnusedTool", "mcp__unused__*"]);
    let result = update_deny(&temp.0, &BTreeSet::new(), &desired, false).unwrap();
    assert_eq!(result.owned, set(&["UnusedTool", "mcp__unused__*"]));
    let first = fs::read(&path).unwrap();
    assert!(
        update_deny(&temp.0, &result.owned, &desired, false)
            .unwrap()
            .changed_files
            .is_empty()
    );
    assert_eq!(fs::read(&path).unwrap(), first);
    let result = update_deny(&temp.0, &result.owned, &BTreeSet::new(), false).unwrap();
    assert!(result.owned.is_empty());
    let value: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
    assert_eq!(
        value["permissions"]["deny"],
        json!(["WebFetch", "Bash(rm -rf *)"])
    );
    let exclude = Command::new("git")
        .current_dir(&temp.0)
        .args(["rev-parse", "--git-path", "info/exclude"])
        .output()
        .unwrap();
    let exclude = String::from_utf8(exclude.stdout).unwrap();
    let contents = fs::read_to_string(temp.0.join(exclude.trim())).unwrap();
    assert_eq!(
        contents
            .lines()
            .filter(|line| *line == "/.claude/settings.local.json")
            .count(),
        1
    );
}
#[test]
fn prune_refuses_tracked_settings_and_dry_run_has_no_writes() {
    let temp = Scratch::new();
    if !init_git(&temp) {
        return;
    }
    let desired = set(&["UnusedTool"]);
    let result = update_deny(&temp.0, &BTreeSet::new(), &desired, true).unwrap();
    assert!(!result.changed_files.is_empty());
    assert!(!temp.0.join(".claude").exists());
    let path = temp.0.join(".claude/settings.local.json");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, b"{}").unwrap();
    assert!(
        Command::new("git")
            .current_dir(&temp.0)
            .args(["add", "--force", ".claude/settings.local.json"])
            .status()
            .unwrap()
            .success()
    );
    assert!(matches!(
        update_deny(&temp.0, &BTreeSet::new(), &desired, false),
        Err(AdapterError::UnsafeTarget { .. })
    ));
    assert_eq!(fs::read(&path).unwrap(), b"{}");
}
#[cfg(unix)]
#[test]
fn prune_refuses_escaping_symlinks() {
    let temp = Scratch::new();
    if !init_git(&temp) {
        return;
    }
    let outside = Scratch::new();
    fs::write(outside.0.join("settings.json"), b"{}").unwrap();
    fs::create_dir_all(temp.0.join(".claude")).unwrap();
    std::os::unix::fs::symlink(
        outside.0.join("settings.json"),
        temp.0.join(".claude/settings.local.json"),
    )
    .unwrap();
    assert!(matches!(
        update_deny(&temp.0, &BTreeSet::new(), &set(&["UnusedTool"]), false),
        Err(AdapterError::UnsafeTarget { .. })
    ));
    assert_eq!(fs::read(outside.0.join("settings.json")).unwrap(), b"{}");
}

#[test]
fn foreign_empty_hooks_and_invalid_documents_are_never_rewritten() {
    let temp = Scratch::new();
    let path = temp.0.join("settings.json");
    fs::write(&path, "{ \"hooks\" : {} }\r\n").unwrap();
    assert!(
        writer::uninstall_hooks(&temp.0, &path, "claude", false)
            .unwrap()
            .changed_files
            .is_empty()
    );
    assert_eq!(fs::read_to_string(&path).unwrap(), "{ \"hooks\" : {} }\r\n");
    for text in ["{", "[]", "{\"hooks\":false}"] {
        fs::write(&path, text).unwrap();
        assert!(writer::install_hooks(&temp.0, &path, "claude", &[HookSpec { native_event:"Stop".into(), kind:EventKind::TurnEnd, matcher:None,
            handler:json!({"command":"tkm-hook","args":["--host","claude","--event","turn_end"]}) }], false).is_err());
        assert_eq!(fs::read_to_string(&path).unwrap(), text);
        assert!(!temp.0.join("settings.json.tkm-bak").exists());
    }
}

#[test]
fn ownership_requires_exact_program_and_adjacent_host_arguments() {
    for handler in [
        json!({"command":"/opt/bin/tkm-hook","args":["--host","claude"]}),
        json!({"command":"'/opt/token tools/tkm-hook' --host 'claude' --event prompt"}),
    ] {
        assert!(writer::owns_handler(&handler, "claude"));
        assert!(!writer::owns_handler(&handler, "codex"));
    }
    for handler in [
        json!({"command":"echo tkm-hook --host claude"}),
        json!({"command":"tkm-hook-malicious --host claude"}),
        json!({"command":"tkm-hook --host claude-extra"}),
        json!({"command":"tkm-hook --host codex; echo --host claude"}),
        json!({"command":"tkm-hook","args":["--host",7]}),
        json!({"command":"tkm-hook","args":["--host","other","claude"]}),
    ] {
        assert!(!writer::owns_handler(&handler, "claude"));
    }
}

#[test]
fn prune_exclude_path_supports_linked_git_worktrees() {
    let main = Scratch::new();
    if !init_git(&main) {
        return;
    }
    assert!(
        Command::new("git")
            .current_dir(&main.0)
            .args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.invalid",
                "commit",
                "--allow-empty",
                "--quiet",
                "-m",
                "fixture"
            ])
            .status()
            .unwrap()
            .success()
    );
    let linked = main.0.join("linked");
    assert!(
        Command::new("git")
            .current_dir(&main.0)
            .arg("worktree")
            .arg("add")
            .arg("--quiet")
            .arg("--detach")
            .arg(&linked)
            .status()
            .unwrap()
            .success()
    );
    update_deny(&linked, &BTreeSet::new(), &set(&["UnusedTool"]), false).unwrap();
    let contents = fs::read_to_string(main.0.join(".git/info/exclude")).unwrap();
    assert!(
        contents
            .lines()
            .any(|line| line == "/.claude/settings.local.json")
    );
    assert!(linked.join(".claude/settings.local.json").exists());
}
