#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]
#![forbid(unsafe_code)]
#![cfg(target_os = "linux")]

mod support_init;
use std::{
    fs,
    os::unix::net::UnixListener,
    process::Stdio,
    time::{Duration, Instant},
};
use support_init::{Fixture, stdout};

fn bounded_output(fixture: &Fixture, args: &[&str]) -> std::process::Output {
    let mut child = fixture
        .command(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(6);
    loop {
        if child.try_wait().unwrap().is_some() {
            return child.wait_with_output().unwrap();
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            let output = child.wait_with_output().unwrap();
            panic!("command did not terminate: {args:?}: {output:?}");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn top_without_daemon_or_database_fails_cleanly_without_a_terminal() {
    let fixture = Fixture::new();
    let out = bounded_output(&fixture, &["top"]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(out.stdout.is_empty());
    let error = String::from_utf8(out.stderr).unwrap();
    assert!(
        error.contains("tkm top requires an interactive terminal"),
        "{error}"
    );
    assert_eq!(error.lines().count(), 1);
    assert!(!fixture.paths.db.exists());
}

#[test]
fn top_with_database_also_rejects_non_terminal_without_panicking() {
    let fixture = Fixture::new();
    tkm_store::Store::open(&fixture.paths.db).unwrap();
    let out = bounded_output(&fixture, &["top"]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    let error = String::from_utf8(out.stderr).unwrap();
    assert!(error.contains("interactive terminal"), "{error}");
    assert!(!error.contains("panicked"));
}

#[test]
fn dashboard_no_open_prints_discovery_url_without_browser_or_daemon_spawn() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.paths.socket.parent().unwrap()).unwrap();
    let _socket = UnixListener::bind(&fixture.paths.socket).unwrap();
    fs::write(
        fixture.paths.socket.with_file_name("tkmd-web.json"),
        r#"{"url":"http://127.0.0.1:12345/","token":"fixture-token"}"#,
    )
    .unwrap();
    fixture.script("xdg-open", "printf 'browser spawned\\n' >> \"$CALL_LOG\"\n");
    let out = bounded_output(&fixture, &["dashboard", "--no-open"]);
    assert!(out.status.success(), "{out:?}");
    assert_eq!(
        stdout(&out),
        "http://127.0.0.1:12345/#token=fixture-token\n"
    );
    assert!(out.stderr.is_empty(), "{out:?}");
    assert!(!fixture.temp.path().join("calls.log").exists());
}

#[test]
fn dashboard_without_web_fails_cleanly_with_enable_hint() {
    let fixture = Fixture::new();
    fs::create_dir_all(fixture.paths.socket.parent().unwrap()).unwrap();
    let _socket = UnixListener::bind(&fixture.paths.socket).unwrap();
    let out = bounded_output(&fixture, &["dashboard", "--no-open"]);
    assert_eq!(out.status.code(), Some(1), "{out:?}");
    assert!(out.stdout.is_empty());
    let error = String::from_utf8(out.stderr).unwrap();
    assert!(
        error.contains("tkm config set ui.web.enabled true"),
        "{error}"
    );
    assert!(error.contains("web"));
    assert_eq!(error.lines().count(), 1);
}

#[test]
fn usage_error_exits_two() {
    let fixture = Fixture::new();
    let out = fixture.run(&["calibrate", "--purpose", "unknown"]);
    assert_eq!(out.status.code(), Some(2));
    assert!(out.stdout.is_empty());
    assert!(!out.stderr.is_empty());
}
