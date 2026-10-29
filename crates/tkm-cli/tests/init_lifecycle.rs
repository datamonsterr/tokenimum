#![forbid(unsafe_code)]
#![cfg(target_os = "linux")] // These real-binary fixtures assert Linux XDG path semantics.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]

mod support_init;

use std::fs;
use std::os::unix::fs::PermissionsExt;
use support_init::{Fixture, HOMES, snapshot, stdout};

#[test]
fn init_dry_run_writes_nothing() {
    let fixture = Fixture::new();
    let before = snapshot(fixture.temp.path());
    let output = fixture.run(&["init", "--yes", "--dry-run"]);
    assert!(output.status.success(), "{output:?}");
    let plan = stdout(&output);
    for (host, home, file) in HOMES {
        assert!(plan.contains(host), "missing {host}: {plan}");
        let target = match host {
            "opencode" => "plugins/tokenimum.ts",
            "pi" => "extensions/tokenimum.ts",
            _ => file,
        };
        assert!(
            plan.contains(&fixture.home.join(home).join(target).display().to_string()),
            "{plan}"
        );
    }
    for line in [
        "superorchestrator:",
        "otherskill:",
        "service",
        "key",
        "preset",
        "/hooks",
        "/reload",
    ] {
        assert!(plan.contains(line), "{plan}");
    }
    assert_eq!(snapshot(fixture.temp.path()), before);
}

#[test]
fn init_records_installs_rows() {
    let fixture = Fixture::new();
    fixture.init();
    let rows = fixture.installs();
    assert_eq!(rows.len(), 8);
    for row in &rows {
        assert!(!row.files_written.is_empty(), "{row:?}");
        assert!(!row.entry_ids.is_empty(), "{row:?}");
        if matches!(row.host, tkm_proto::Host::Opencode | tkm_proto::Host::Pi) {
            assert_eq!(row.shim_hashes.len(), 1);
        }
    }
    assert!(rows.iter().any(|row| row.files_written.iter().any(|path| {
        path.extension()
            .is_some_and(|extension| extension == "service" || extension == "plist")
    })));
}

#[test]
fn init_only_restricts_hosts() {
    let fixture = Fixture::new();
    let before = snapshot(&fixture.home.join(".kimi"));
    let output = fixture.run(&["init", "--yes", "--only", "claude,codex"]);
    assert!(output.status.success(), "{output:?}");
    let rows = fixture.installs();
    assert_eq!(rows.len(), 2);
    assert!(
        rows.iter()
            .all(|row| matches!(row.host, tkm_proto::Host::Claude | tkm_proto::Host::Codex))
    );
    assert_eq!(snapshot(&fixture.home.join(".kimi")), before);
}

#[test]
fn init_home_host_single_target() {
    let fixture = Fixture::new();
    let explicit = fixture.temp.path().join("account/claude");
    let output = fixture.run(&[
        "init",
        "--yes",
        "--home",
        explicit.to_str().unwrap(),
        "--host",
        "claude",
    ]);
    assert!(output.status.success(), "{output:?}");
    let rows = fixture.installs();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].agent_home, explicit);
    assert!(explicit.join("settings.json").is_file());
}

#[test]
fn init_no_shims_uses_fold_only() {
    let fixture = Fixture::new();
    let output = fixture.run(&["init", "--yes", "--only", "opencode,pi", "--no-shims"]);
    assert!(output.status.success(), "{output:?}");
    assert!(stdout(&output).contains("fold-only"));
    assert!(stdout(&output).contains("instructions block: added per repository"));
    assert!(stdout(&output).contains("No verified skill directory for pi"));
    for home in [".config/opencode", ".pi/agent"] {
        assert!(!fixture.home.join(home).join("AGENTS.md").exists());
    }
    assert!(
        !fixture
            .home
            .join(".config/opencode/plugins/tokenimum.ts")
            .exists()
    );
    assert!(
        !fixture
            .home
            .join(".pi/agent/extensions/tokenimum.ts")
            .exists()
    );
    assert!(
        fixture
            .home
            .join(".config/opencode/skills/tokenimum/SKILL.md")
            .exists()
    );
}

#[test]
fn init_fold_only_repository_block_is_removed_from_recorded_repository() {
    let fixture = Fixture::new();
    let repo = fixture.repository();
    let original = "# Foreign instructions\r\nKeep these bytes.\r\n";
    fs::write(repo.join("AGENTS.md"), original).unwrap();
    let output = fixture
        .command(&["init", "--yes", "--only", "opencode,pi", "--no-shims"])
        .current_dir(&repo)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(
        fs::read_to_string(repo.join("AGENTS.md"))
            .unwrap()
            .contains("<!-- tokenimum:begin -->")
    );
    assert!(
        fs::read_to_string(repo.join(".git/info/exclude"))
            .unwrap()
            .contains("/AGENTS.md.tkm-bak")
    );
    let output = fixture.run(&["uninstall"]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(
        fs::read(repo.join("AGENTS.md")).unwrap(),
        original.as_bytes()
    );
    assert!(!repo.join("AGENTS.md.tkm-bak").exists());
}

#[test]
fn init_without_tty_or_yes_exits_2() {
    let fixture = Fixture::new();
    let before = snapshot(fixture.temp.path());
    let output = fixture.run(&["init"]);
    assert_eq!(output.status.code(), Some(2), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("re-run with --yes to apply"));
    assert!(stdout(&output).contains("claude"));
    assert_eq!(snapshot(fixture.temp.path()), before);
}

#[test]
fn init_one_host_failure_continues() {
    let fixture = Fixture::new();
    fs::write(fixture.home.join(".claude/settings.json"), "{broken").unwrap();
    let output = fixture.run(&["init", "--yes"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("claude"));
    let rows = fixture.installs();
    assert_eq!(rows.len(), 7);
    assert!(rows.iter().any(|row| row.host == tkm_proto::Host::Pi));
    assert_eq!(
        fs::read(fixture.home.join(".claude/settings.json")).unwrap(),
        b"{broken"
    );
}

#[test]
fn init_rejects_host_without_home_and_invalid_preset_without_writes() {
    let fixture = Fixture::new();
    let before = snapshot(fixture.temp.path());
    for args in [
        &["init", "--yes", "--host", "claude"][..],
        &["init", "--yes", "--preset", "unknown"][..],
    ] {
        let output = fixture.run(args);
        assert_eq!(output.status.code(), Some(2), "{output:?}");
        assert_eq!(snapshot(fixture.temp.path()), before);
    }
}

#[test]
fn init_repeated_install_retains_reversal_manifest() {
    let fixture = Fixture::new();
    let before = snapshot(&fixture.home);
    fixture.init();
    fixture.init();
    let output = fixture.run(&["uninstall"]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(snapshot(&fixture.home), before);
}

#[test]
fn init_missing_service_manager_uses_fake_detached_daemon() {
    let fixture = Fixture::new();
    fs::remove_file(fixture.bin.join("systemctl")).unwrap();
    fs::remove_file(fixture.bin.join("launchctl")).unwrap();
    let output = fixture.run(&["init", "--yes", "--only", "claude"]);
    assert!(output.status.success(), "{output:?}");
    assert!(String::from_utf8_lossy(&output.stderr).contains("detached"));
    for _ in 0..100 {
        if fs::read_to_string(fixture.temp.path().join("calls.log"))
            .is_ok_and(|log| log.contains("tkmd"))
        {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    panic!("fake detached daemon was not invoked");
}

#[test]
fn init_detects_binary_aliases_without_existing_agent_homes() {
    let fixture = Fixture::new();
    fs::remove_dir_all(&fixture.home).unwrap();
    fs::create_dir(&fixture.home).unwrap();
    fixture.script("cursor-agent", "printf 'cursor fixture\\n'\n");
    fixture.script("codewhale", "printf 'codewhale fixture\\n'\n");
    let output = fixture.run(&["init", "--yes", "--only", "cursor,deepseek"]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(fixture.installs().len(), 2);
    assert!(fixture.home.join(".cursor/hooks.json").is_file());
    assert!(fixture.home.join(".codewhale/config.toml").is_file());
}

#[test]
fn init_reuses_key_file_and_writes_selected_preset() {
    let fixture = Fixture::new();
    let path = tkm_decide::key::default_key_file(&fixture.home);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "OPENROUTER_API_KEY=fixture-reused-secret\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let before = fs::read(&path).unwrap();
    let output = fixture.run(&["init", "--yes", "--only", "claude", "--preset", "observe"]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(fs::read(path).unwrap(), before);
    let config = fs::read_to_string(&fixture.paths.global_config).unwrap();
    assert!(config.contains("preset = \"observe\""), "{config}");
    assert!(!stdout(&output).contains("fixture-reused-secret"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("fixture-reused-secret"));
}

#[test]
fn init_environment_key_keeps_decisions_enabled_without_copying_secret() {
    let fixture = Fixture::new();
    let secret = "sk-or-v1-environment-fixture-secret";
    let output = fixture
        .command(&["init", "--yes", "--only", "claude"])
        .env("OPENROUTER_API_KEY", secret)
        .output()
        .unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(
        stdout(&output).contains("key: from environment"),
        "{output:?}"
    );
    assert!(!stdout(&output).contains(secret));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(secret));
    let config = fs::read_to_string(&fixture.paths.global_config).unwrap();
    assert!(config.contains("enabled = true"), "{config}");
    assert!(!config.contains(secret));
    assert!(!tkm_decide::key::default_key_file(&fixture.home).exists());
}
