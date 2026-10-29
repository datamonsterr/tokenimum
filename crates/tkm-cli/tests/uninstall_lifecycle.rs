#![forbid(unsafe_code)]
#![cfg(target_os = "linux")] // These real-binary fixtures assert Linux XDG path semantics.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]

mod support_init;
use std::fs;
use support_init::{Fixture, snapshot};

#[test]
fn init_then_uninstall_restores_home() {
    let fixture = Fixture::new();
    let before = snapshot(&fixture.home);
    let config_before = snapshot(&fixture.temp.path().join("config"));
    fixture.init();
    let output = fixture.run(&["uninstall"]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(snapshot(&fixture.home), before);
    assert_eq!(snapshot(&fixture.temp.path().join("config")), config_before);
    assert!(fixture.installs().is_empty());
    let calls = fs::read_to_string(fixture.temp.path().join("calls.log")).unwrap();
    if cfg!(target_os = "linux") {
        for invocation in [
            "systemctl --user enable tkmd.service",
            "systemctl --user start tkmd.service",
            "systemctl --user stop tkmd.service",
            "systemctl --user disable tkmd.service",
        ] {
            assert!(calls.contains(invocation), "{calls}");
        }
    } else if cfg!(target_os = "macos") {
        assert!(calls.contains("launchctl load"), "{calls}");
        assert!(calls.contains("launchctl unload"), "{calls}");
    }
}

#[test]
fn uninstall_host_filter() {
    let fixture = Fixture::new();
    let original = fs::read(fixture.home.join(".claude/settings.json")).unwrap();
    fixture.init();
    let output = fixture.run(&["uninstall", "--host", "claude"]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(fixture.installs().len(), 7);
    assert_eq!(
        fs::read(fixture.home.join(".claude/settings.json")).unwrap(),
        original
    );
    assert!(
        fs::read_to_string(fixture.home.join(".codex/hooks.json"))
            .unwrap()
            .contains("tkm-hook")
    );
    let calls = fs::read_to_string(fixture.temp.path().join("calls.log")).unwrap();
    assert!(!calls.contains("--user stop"), "{calls}");
    assert!(!calls.contains("launchctl unload"), "{calls}");
    let output = fixture.run(&["uninstall"]);
    assert!(output.status.success(), "{output:?}");
    assert!(
        !fixture
            .temp
            .path()
            .join("config/systemd/user/tkmd.service")
            .exists()
    );
}

#[test]
fn uninstall_leaves_foreign_entries() {
    let fixture = Fixture::new();
    fixture.init();
    let path = fixture.home.join(".claude/settings.json");
    let content = fs::read_to_string(&path).unwrap();
    fs::write(
        &path,
        content.replacen(
            "\"foreign\": true",
            "\"foreign\": true, \"later\": \"keep-me\"",
            1,
        ),
    )
    .unwrap();
    let output = fixture.run(&["uninstall"]);
    assert!(output.status.success(), "{output:?}");
    let content = fs::read_to_string(path).unwrap();
    assert!(content.contains("foreign-hook"), "{content}");
    assert!(content.contains("keep-me"), "{content}");
    assert!(!content.contains("tkm-hook"), "{content}");
}

#[test]
fn uninstall_dry_run_writes_nothing() {
    let fixture = Fixture::new();
    fixture.init();
    let before = snapshot(fixture.temp.path());
    let output = fixture.run(&["uninstall", "--dry-run"]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(snapshot(fixture.temp.path()), before);
}

#[test]
fn uninstall_preserves_edited_shim_and_its_record() {
    let fixture = Fixture::new();
    fixture.init();
    let path = fixture.home.join(".config/opencode/plugins/tokenimum.ts");
    let mut content = fs::read_to_string(&path).unwrap();
    content.push_str("// person edit\n");
    fs::write(&path, &content).unwrap();
    let output = fixture.run(&["uninstall"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(fs::read_to_string(path).unwrap(), content);
    assert!(
        fixture
            .installs()
            .iter()
            .any(|row| row.host == tkm_proto::Host::Opencode)
    );
}

#[test]
fn uninstall_retains_manifest_when_service_file_was_edited() {
    let fixture = Fixture::new();
    fixture.init();
    let service = if cfg!(target_os = "macos") {
        fixture
            .home
            .join("Library/LaunchAgents/com.tokenimum.tkmd.plist")
    } else {
        fixture.temp.path().join("config/systemd/user/tkmd.service")
    };
    let original = fs::read(&service).unwrap();
    let mut edited = original.clone();
    edited.extend_from_slice(b"\n# edited by person\n");
    fs::write(&service, &edited).unwrap();
    let output = fixture.run(&["uninstall"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert_eq!(fs::read(&service).unwrap(), edited);
    assert!(!fixture.installs().is_empty());
    fs::write(&service, original).unwrap();
    let output = fixture.run(&["uninstall"]);
    assert!(output.status.success(), "{output:?}");
    assert!(!service.exists());
    assert!(fixture.installs().is_empty());
}

#[test]
fn uninstall_restores_existing_global_config_bytes_after_repeated_init() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.paths.global_config.parent().unwrap()).unwrap();
    fs::write(
        &fixture.paths.global_config,
        "# original bytes\r\npreset = 'balanced'\r\n[decide]\r\nenabled = true\r\n",
    )
    .unwrap();
    let config_before = snapshot(&fixture.temp.path().join("config"));
    for preset in ["observe", "offline"] {
        let output = fixture.run(&["init", "--yes", "--preset", preset]);
        assert!(output.status.success(), "{output:?}");
    }
    let output = fixture.run(&["uninstall", "--host", "claude"]);
    assert!(output.status.success(), "{output:?}");
    assert_ne!(snapshot(&fixture.temp.path().join("config")), config_before);
    let output = fixture.run(&["uninstall"]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(snapshot(&fixture.temp.path().join("config")), config_before);
}

#[test]
fn uninstall_stop_failure_keeps_recovery_row_until_retry() {
    let fixture = Fixture::new();
    fixture.init();
    fixture.script("systemctl", "exit 1\n");
    let output = fixture.run(&["uninstall"]);
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(!fixture.installs().is_empty());
    assert!(
        fixture
            .temp
            .path()
            .join("config/systemd/user/tkmd.service")
            .exists()
    );
    fixture.script("systemctl", "exit 0\n");
    let output = fixture.run(&["uninstall"]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(fixture.installs().len(), 0);
}

#[test]
fn uninstall_running_detached_daemon_prints_stop_command_and_keeps_manifest() {
    use std::io::Write;
    use std::os::unix::net::UnixListener;
    use tkm_proto::monitor::{MONITOR_MAX_FRAME, MONITOR_SOCKET_NAME, MonitorMessage};
    let fixture = Fixture::new();
    fs::remove_file(fixture.bin.join("systemctl")).unwrap();
    fixture.init();
    fs::create_dir_all(fixture.paths.socket.parent().unwrap()).unwrap();
    let listener =
        UnixListener::bind(fixture.paths.socket.with_file_name(MONITOR_SOCKET_NAME)).unwrap();
    let peer = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let hello = MonitorMessage::Hello {
            version: tkm_proto::PROTOCOL_VERSION,
            daemon_version: None,
            started_at_ms: None,
        };
        stream
            .write_all(&tkm_proto::encode_frame(&hello, MONITOR_MAX_FRAME).unwrap())
            .unwrap();
    });
    let output = fixture.run(&["uninstall"]);
    peer.join().unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("pkill -x tkmd"),
        "{output:?}"
    );
    assert!(!fixture.installs().is_empty());
    fs::remove_file(fixture.paths.socket.with_file_name(MONITOR_SOCKET_NAME)).unwrap();
    let output = fixture.run(&["uninstall"]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(fixture.installs().len(), 0);
}
