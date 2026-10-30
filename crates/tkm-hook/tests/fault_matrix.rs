//! SPEC 16 fault-injection matrix, hook side: daemon down, socket slow and shim timeout.
//! Each fault must give the no-op reply, an `errors.log` line and exit code 0. The daemon-side
//! faults (OpenRouter, locked databases, corrupt log) are in `crates/tkmd/tests/fault_matrix.rs`.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixListener;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use tempfile::TempDir;

const BIN: &str = env!("CARGO_BIN_EXE_tkm-hook");

fn hook(tmp: &TempDir, host: &str, event: &str) -> Command {
    let mut cmd = Command::new(BIN);
    cmd.args(["--host", host, "--event", event]);
    cmd.env_clear();
    for name in ["run", "data", "home"] {
        std::fs::create_dir_all(tmp.path().join(name)).unwrap();
    }
    cmd.env("XDG_RUNTIME_DIR", tmp.path().join("run"));
    cmd.env("XDG_DATA_HOME", tmp.path().join("data"));
    cmd.env("HOME", tmp.path().join("home"));
    if let Ok(path) = std::env::var("PATH") {
        cmd.env("PATH", path);
    }
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    cmd
}

fn socket(tmp: &TempDir) -> PathBuf {
    tmp.path().join("run/tokenimum/tkmd.sock")
}

fn errors_log(tmp: &TempDir) -> String {
    std::fs::read_to_string(tmp.path().join("data/tokenimum/errors.log")).unwrap_or_default()
}

/// Accepts one connection, reads the request and never answers.
fn silent_daemon(tmp: &TempDir) -> thread::JoinHandle<()> {
    let path = socket(tmp);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let listener = UnixListener::bind(path).unwrap();
    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0_u8; 1024];
            let _ = stream.read(&mut buf);
            thread::sleep(Duration::from_secs(5));
        }
    })
}

fn assert_noop(output: &Output, elapsed: Duration, tmp: &TempDir) {
    assert_eq!(output.status.code(), Some(0), "a hook never fails");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        matches!(stdout.trim(), "" | "{}"),
        "no-op reply expected, got {stdout:?}"
    );
    assert!(
        elapsed < Duration::from_millis(500),
        "hook took {elapsed:?}"
    );
    assert!(
        !errors_log(tmp).trim().is_empty(),
        "an errors.log line is required"
    );
}

#[test]
fn fault_daemon_down() {
    let tmp = TempDir::new().unwrap();
    let fake = tmp.path().join("tkmd");
    std::fs::write(&fake, "#!/bin/sh\nsleep 2\n").unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut cmd = hook(&tmp, "claude", "tool_done");
    cmd.env("TKM_TKMD_BIN", &fake);
    let start = Instant::now();
    let output = cmd.output().unwrap();
    assert_noop(&output, start.elapsed(), &tmp);
}

#[test]
fn fault_socket_slow() {
    let tmp = TempDir::new().unwrap();
    let _daemon = silent_daemon(&tmp);
    let start = Instant::now();
    let output = hook(&tmp, "claude", "tool_done").output().unwrap();
    assert_noop(&output, start.elapsed(), &tmp);
}

#[test]
fn fault_shim_timeout_pi_and_opencode() {
    // The Pi and opencode shims run `tkm-hook` under their own hard timeout; the hook must
    // stay inside its own read budget so a slow daemon never makes the shim time out first.
    for host in ["pi", "opencode"] {
        let tmp = TempDir::new().unwrap();
        let _daemon = silent_daemon(&tmp);
        let start = Instant::now();
        let output = hook(&tmp, host, "tool_done").output().unwrap();
        let elapsed = start.elapsed();
        assert_eq!(output.status.code(), Some(0), "{host}: hook never fails");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            matches!(stdout.trim(), "" | "{}"),
            "{host}: no-op reply expected, got {stdout:?}"
        );
        assert!(
            elapsed < Duration::from_millis(1000),
            "{host}: hook took {elapsed:?}, over the shim's 1s budget"
        );
        assert!(
            !errors_log(&tmp).trim().is_empty(),
            "{host}: errors.log line"
        );
    }
}
