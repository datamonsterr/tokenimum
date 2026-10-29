#![forbid(unsafe_code)]
#![cfg(unix)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support_init;
use std::fs;
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
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let output = fixture.run(&["doctor"]);
    let report = stdout(&output);
    assert!(report.contains("ok key:"), "{report}");
    assert!(!report.contains("fixture-secret-never-display"));
    assert!(!String::from_utf8_lossy(&output.stderr).contains("fixture-secret-never-display"));
}
