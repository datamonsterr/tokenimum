#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]
#![forbid(unsafe_code)]
#![cfg(target_os = "linux")]

mod support_init;
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};
use support_init::{Fixture, snapshot, stdout};
use tkm_proto::Host;

fn orchestrator(f: &Fixture) -> (PathBuf, PathBuf) {
    let config = f.temp.path().join("orchestrator");
    fs::create_dir_all(config.join("accounts")).unwrap();
    let home = f.home.join(".claude-account");
    fs::create_dir(&home).unwrap();
    fs::write(home.join("settings.json"), "{\"foreign\":true}\n").unwrap();
    let account = serde_json::to_string(home.to_str().unwrap()).unwrap();
    fs::write(
        config.join("accounts/fixture.toml"),
        format!("name = 'fixture'\nharness = 'claude'\nhome = {account}\nenabled = true\n"),
    )
    .unwrap();
    f.script(
        "superorchestrator",
        "printf 'superorchestrator fixture\\n'\n",
    );
    (config, home)
}

#[test]
fn init_superorchestrator_installs_account_and_uninstall_restores_home() {
    let f = Fixture::new();
    let (config, home) = orchestrator(&f);
    let before = snapshot(&home);
    let out = f
        .command(&["init", "--yes", "--only", "claude", "--superorchestrator"])
        .env("SUPERORCHESTRATOR_CONFIG_DIR", &config)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert!(stdout(&out).contains(home.to_str().unwrap()), "{out:?}");
    let rows = f.installs();
    let row = rows
        .iter()
        .find(|row| row.agent_home == home)
        .expect("account install row");
    assert_eq!(row.host, Host::Claude);
    assert!(!row.entry_ids.is_empty());
    assert!(
        fs::read_to_string(home.join("settings.json"))
            .unwrap()
            .contains("tkm-hook")
    );
    let repeated = f
        .command(&["init", "--yes", "--only", "claude", "--superorchestrator"])
        .env("SUPERORCHESTRATOR_CONFIG_DIR", &config)
        .output()
        .unwrap();
    assert!(repeated.status.success(), "{repeated:?}");
    assert_eq!(
        f.installs().iter().filter(|r| r.agent_home == home).count(),
        1
    );
    // Uninstall relies on stored ownership, even after the orchestrator config is gone.
    fs::remove_dir_all(config).unwrap();
    let out = f.run(&["uninstall"]);
    assert!(out.status.success(), "{out:?}");
    assert!(f.installs().is_empty());
    assert_eq!(snapshot(&home), before);
}

#[test]
fn init_superorchestrator_dry_run_writes_nothing_and_filters_accounts() {
    let f = Fixture::new();
    let (config, home) = orchestrator(&f);
    let before = snapshot(f.temp.path());
    let out = f
        .command(&[
            "init",
            "--yes",
            "--dry-run",
            "--only",
            "claude",
            "--superorchestrator",
        ])
        .env("SUPERORCHESTRATOR_CONFIG_DIR", &config)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert!(stdout(&out).contains(home.to_str().unwrap()), "{out:?}");
    assert_eq!(snapshot(f.temp.path()), before);
    let filtered = f
        .command(&[
            "init",
            "--yes",
            "--dry-run",
            "--only",
            "codex",
            "--superorchestrator",
        ])
        .env("SUPERORCHESTRATOR_CONFIG_DIR", config)
        .output()
        .unwrap();
    assert!(filtered.status.success(), "{filtered:?}");
    assert!(
        !stdout(&filtered).contains(home.to_str().unwrap()),
        "{filtered:?}"
    );
}

#[test]
fn init_detection_uses_e9_probes_and_configuration() {
    let f = Fixture::new();
    let (config, _) = orchestrator(&f);
    let otherskill = f.home.join(".config/otherskill");
    fs::create_dir_all(&otherskill).unwrap();
    fs::write(otherskill.join("otherskill.config.json"), "{}").unwrap();
    f.script("otherskill", "[ \"$1\" = --help ]\n");
    let out = f
        .command(&["init", "--yes", "--dry-run", "--only", "claude"])
        .env("SUPERORCHESTRATOR_CONFIG_DIR", &config)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert!(stdout(&out).contains("superorchestrator: on"), "{out:?}");
    assert!(stdout(&out).contains("otherskill: on"), "{out:?}");
    fs::remove_file(f.bin.join("otherskill")).unwrap();
    let out = f
        .command(&["init", "--yes", "--dry-run", "--only", "claude"])
        .env("SUPERORCHESTRATOR_CONFIG_DIR", config)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert!(
        stdout(&out).contains("otherskill: off (binary missing)"),
        "{out:?}"
    );
}

#[test]
fn init_yes_never_consents_to_otherskill_key_reuse() {
    let f = Fixture::new();
    let key = f.home.join(".config/otherskill/credentials.json");
    fs::create_dir_all(key.parent().unwrap()).unwrap();
    let original = b"{\"openrouter\":\"fixture-otherskill-secret\"}";
    fs::write(&key, original).unwrap();
    fs::set_permissions(&key, fs::Permissions::from_mode(0o600)).unwrap();
    let out = f.run(&["init", "--yes", "--only", "claude"]);
    assert!(out.status.success(), "{out:?}");
    let config = f.run(&["config", "get", "integrations.otherskill_key_reuse"]);
    assert!(config.status.success(), "{config:?}");
    assert_eq!(stdout(&config).trim(), "false");
    assert!(!stdout(&out).contains("fixture-otherskill-secret"));
    assert!(!String::from_utf8_lossy(&out.stderr).contains("fixture-otherskill-secret"));
    assert_eq!(fs::read(key).unwrap(), original);
}

#[test]
fn init_noninteractive_does_not_accept_piped_key_consent() {
    use std::io::Write;
    use std::process::Stdio;
    let f = Fixture::new();
    let before = snapshot(f.temp.path());
    let mut child = f
        .command(&["init", "--only", "claude"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(b"yes\nyes\n")
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert_eq!(out.status.code(), Some(2), "{out:?}");
    assert_eq!(snapshot(f.temp.path()), before);
}

fn statuses(f: &Fixture, contents: &str) {
    let directory = f.paths.data_dir.join("integrations");
    fs::create_dir_all(&directory).unwrap();
    fs::write(directory.join("status.json"), contents).unwrap();
}

#[test]
fn status_and_doctor_read_enabled_and_backoff_adapter_state() {
    let f = Fixture::new();
    tkm_store::Store::open(&f.paths.db).unwrap();
    statuses(
        &f,
        r#"[{"name":"superorchestrator","mode":"auto","state":"enabled","reason":null},{"name":"otherskill","mode":"on","state":"backoff","reason":"selection timed out"}]"#,
    );
    let out = f.run(&["status", "--json"]);
    assert!(out.status.success(), "{out:?}");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["adapters"][0]["enabled"], true);
    assert_eq!(json["adapters"][0]["reason"], "enabled; configured auto");
    assert_eq!(json["adapters"][1]["enabled"], false);
    assert_eq!(
        json["adapters"][1]["reason"],
        "backoff: selection timed out; configured on"
    );
    let out = f.run(&["doctor"]);
    let text = stdout(&out);
    assert!(
        text.contains("ok superorchestrator adapter: enabled; configured auto"),
        "{text}"
    );
    assert!(
        text.contains("warn otherskill adapter: backoff: selection timed out; configured on"),
        "{text}"
    );
}

#[test]
fn malformed_adapter_status_falls_back_without_failure() {
    let f = Fixture::new();
    tkm_store::Store::open(&f.paths.db).unwrap();
    statuses(&f, "incomplete-json");
    let out = f.run(&["status", "--json"]);
    assert!(out.status.success(), "{out:?}");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["adapters"][0]["enabled"], false);
    assert_eq!(
        json["adapters"][0]["reason"],
        "adapter unavailable; configured auto"
    );
}

#[test]
fn init_skips_disabled_unsupported_and_duplicate_account_homes() {
    let f = Fixture::new();
    let (config, home) = orchestrator(&f);
    let account = fs::read_to_string(config.join("accounts/fixture.toml")).unwrap();
    fs::write(
        config.join("accounts/duplicate.toml"),
        account.replace("'fixture'", "'duplicate'"),
    )
    .unwrap();
    fs::write(
        config.join("accounts/disabled.toml"),
        account
            .replace("enabled = true", "enabled = false")
            .replace(".claude-account", ".disabled"),
    )
    .unwrap();
    fs::write(
        config.join("accounts/unsupported.toml"),
        account
            .replace("harness = 'claude'", "harness = 'unsupported'")
            .replace(".claude-account", ".unsupported"),
    )
    .unwrap();
    let out = f
        .command(&["init", "--yes", "--only", "claude", "--superorchestrator"])
        .env("SUPERORCHESTRATOR_CONFIG_DIR", config)
        .output()
        .unwrap();
    assert!(out.status.success(), "{out:?}");
    assert_eq!(
        f.installs()
            .iter()
            .filter(|row| row.agent_home == home)
            .count(),
        1
    );
    assert!(!f.home.join(".disabled").exists());
    assert!(!f.home.join(".unsupported").exists());
}

#[test]
fn doctor_reads_otherskill_key_only_with_prior_consent() {
    let f = Fixture::new();
    let key = f.home.join(".config/otherskill/credentials.json");
    fs::create_dir_all(key.parent().unwrap()).unwrap();
    fs::write(&key, r#"{"openrouter":"fixture-otherskill-secret"}"#).unwrap();
    fs::set_permissions(&key, fs::Permissions::from_mode(0o600)).unwrap();
    let out = f.run(&["doctor"]);
    assert!(stdout(&out).contains("key: offline (no key)"), "{out:?}");
    let out = f.run(&["config", "set", "integrations.otherskill_key_reuse", "true"]);
    assert!(out.status.success(), "{out:?}");
    let out = f.run(&["doctor"]);
    assert!(
        stdout(&out).contains("key: present (value hidden)"),
        "{out:?}"
    );
    assert!(!stdout(&out).contains("fixture-otherskill-secret"));
    assert!(!String::from_utf8_lossy(&out.stderr).contains("fixture-otherskill-secret"));
    // Consent cannot bypass a broken or insecure tokenimum key file.
    let own_key = tkm_decide::key::default_key_file(&f.home);
    fs::create_dir_all(own_key.parent().unwrap()).unwrap();
    fs::write(&own_key, "OPENROUTER_API_KEY=fixture-own\n").unwrap();
    fs::set_permissions(own_key, fs::Permissions::from_mode(0o644)).unwrap();
    let out = f.run(&["doctor"]);
    assert!(stdout(&out).contains("fail key:"), "{out:?}");
}

#[test]
fn configuration_off_overrides_enabled_runtime_adapter_status() {
    let f = Fixture::new();
    tkm_store::Store::open(&f.paths.db).unwrap();
    statuses(
        &f,
        r#"[{"name":"otherskill","mode":"auto","state":"enabled","reason":null}]"#,
    );
    let out = f.run(&["config", "set", "integrations.otherskill", "off"]);
    assert!(out.status.success(), "{out:?}");
    let out = f.run(&["status", "--json"]);
    assert!(out.status.success(), "{out:?}");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(json["adapters"][1]["enabled"], false);
    assert_eq!(json["adapters"][1]["reason"], "disabled in configuration");
}
