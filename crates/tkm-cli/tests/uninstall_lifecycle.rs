#![forbid(unsafe_code)]
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support_init;
use std::fs;
use support_init::{Fixture, snapshot};

#[test]
fn init_then_uninstall_restores_home() {
    let fixture = Fixture::new();
    let before = snapshot(&fixture.home);
    fixture.init();
    let output = fixture.run(&["uninstall"]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(snapshot(&fixture.home), before);
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
