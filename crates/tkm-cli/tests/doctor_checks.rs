#![forbid(unsafe_code)]
#![cfg(target_os = "linux")] // These real-binary fixtures assert Linux XDG path semantics.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]

mod support_init;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use support_init::{Fixture, stdout};

#[test]
fn doctor_reports_daemon_down_and_missing_hooks() {
    let fixture = Fixture::new();
    fixture.init();
    fs::write(
        fixture.home.join(".claude/settings.json"),
        "{\"hooks\":{}}\n",
    )
    .unwrap();
    let output = fixture.run(&["doctor"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let report = stdout(&output);
    assert!(report.contains("fail daemon:"), "{report}");
    assert!(report.contains("fail claude hooks:"), "{report}");
    for check in [
        "socket:",
        "database:",
        "claude version:",
        "claude events:",
        "opencode shim:",
        "key:",
    ] {
        assert!(report.contains(check), "{report}");
    }
    assert!(
        report.lines().all(|line| ["ok ", "warn ", "fail "]
            .iter()
            .any(|prefix| line.starts_with(prefix))),
        "{report}"
    );
}

#[test]
fn doctor_probe_offline() {
    let fixture = Fixture::new();
    let output = fixture.run(&["doctor", "--probe"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(
        stdout(&output).contains("model probes: skipped (offline)"),
        "{output:?}"
    );
    assert!(!fixture.paths.db.exists());
}

#[test]
fn doctor_detects_edited_shim() {
    let fixture = Fixture::new();
    fixture.init();
    let path = fixture.home.join(".pi/agent/extensions/tokenimum.ts");
    let content = fs::read_to_string(&path).unwrap() + "// edited\n";
    fs::write(path, content).unwrap();
    let output = fixture.run(&["doctor"]);
    assert!(stdout(&output).contains("fail pi shim:"), "{output:?}");
}

#[test]
fn doctor_key_contents_are_never_printed() {
    let fixture = Fixture::new();
    let path = tkm_decide::key::default_key_file(&fixture.home);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, "OPENROUTER_API_KEY=fixture-secret-never-display\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let output = fixture.run(&["doctor"]);
    let report = stdout(&output);
    assert!(report.contains("ok key:"), "{report}");
    assert!(!report.contains("fixture-secret-never-display"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("fixture-secret-never-display"));
}

#[test]
fn doctor_reports_healthy_with_fake_monitor_and_installed_hooks() {
    use std::io::Write;
    use std::os::unix::net::UnixListener;
    use tkm_proto::monitor::{MONITOR_MAX_FRAME, MONITOR_SOCKET_NAME, MonitorMessage};
    let fixture = Fixture::new();
    fixture.init();
    fs::create_dir_all(fixture.paths.socket.parent().unwrap()).unwrap();
    let listener =
        UnixListener::bind(fixture.paths.socket.with_file_name(MONITOR_SOCKET_NAME)).unwrap();
    let peer = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let frame = tkm_proto::encode_frame(
            &MonitorMessage::Hello {
                version: tkm_proto::PROTOCOL_VERSION,
                daemon_version: None,
                started_at_ms: None,
            },
            MONITOR_MAX_FRAME,
        )
        .unwrap();
        stream.write_all(&frame).unwrap();
    });
    let output = fixture.run(&["doctor"]);
    peer.join().unwrap();
    assert!(output.status.success(), "{output:?}");
    assert!(!stdout(&output).contains("fail "), "{output:?}");
}

#[test]
fn doctor_bounds_harness_version_to_one_second() {
    let fixture = Fixture::new();
    fixture.init();
    fixture.script("claude", "while :; do :; done\n");
    let started = std::time::Instant::now();
    let output = fixture.run(&["doctor"]);
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    assert!(
        stdout(&output).contains("fail claude version: tkm: --version timed out after 1 s"),
        "{output:?}"
    );
}

#[test]
fn doctor_persists_fallback_then_clears_it_after_repair() {
    use tkm_hosts::{HostAdapter, InstallContext};
    use tkm_proto::Host;
    let fixture = Fixture::new();
    fixture.init();
    fs::write(fixture.home.join(".claude/settings.json"), "{\"hooks\":{}}\n").unwrap();
    fixture.run(&["doctor"]);
    let store = tkm_store::Store::open(&fixture.paths.db).unwrap();
    assert!(store.host_fold_only(Host::Claude).unwrap());
    let plan = stdout(&fixture.run(&["init", "--dry-run", "--only", "claude"]));
    assert!(plan.contains("claude: fold-only"), "{plan}");
    tkm_hosts::claude::ClaudeAdapter.install(&InstallContext {
        agent_home: fixture.home.join(".claude"), repo_root: None, dry_run: false,
    }).unwrap();
    fixture.run(&["doctor"]);
    assert!(!store.host_fold_only(Host::Claude).unwrap());
}
