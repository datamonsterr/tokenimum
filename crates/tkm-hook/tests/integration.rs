#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Integration tests for `tkm-hook` binary.
//!
//! These tests exercise the full binary using a fake Unix-domain socket server
//! and `TKM_TKMD_BIN` injection for the daemon-spawn path.
//!
//! Each test sets `XDG_RUNTIME_DIR`, `XDG_DATA_HOME`, and `HOME` on the child
//! process to avoid touching the real user environment.

use std::io::{Read, Write};
use std::os::unix::ffi::OsStringExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use tempfile::TempDir;
use tkm_proto::{
    EventKind, Host, MAX_REPLY_FRAME, MAX_REQUEST_FRAME, PROTOCOL_VERSION, Reply, ReplyAction,
    read_frame, write_frame,
};

// Binary path injected by Cargo.
const BIN: &str = env!("CARGO_BIN_EXE_tkm-hook");

/// Build a `Command` for the hook binary with isolated env vars pointing at
/// `tmp`. Inherits nothing from the parent process environment by default;
/// callers add what they need.
fn hook_cmd(tmp: &TempDir, host: &str, event: &str) -> Command {
    let mut cmd = Command::new(BIN);
    cmd.arg("--host").arg(host).arg("--event").arg(event);
    cmd.env_clear();
    // Minimal environment: point XDG dirs at tmp so socket and error log land
    // in a temp location.
    let xdg_runtime = tmp.path().join("run");
    let xdg_data = tmp.path().join("data");
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&xdg_runtime).unwrap();
    std::fs::create_dir_all(&xdg_data).unwrap();
    std::fs::create_dir_all(&home).unwrap();
    cmd.env("XDG_RUNTIME_DIR", &xdg_runtime);
    cmd.env("XDG_DATA_HOME", &xdg_data);
    cmd.env("HOME", &home);
    // PATH must be set so the binary can find itself (needed by spawn path).
    if let Ok(path) = std::env::var("PATH") {
        cmd.env("PATH", path);
    }
    cmd
}

/// Returns the socket path that `Paths::from_env()` would produce given the
/// env we set in `hook_cmd`.
fn socket_path(tmp: &TempDir) -> PathBuf {
    tmp.path().join("run").join("tokenimum").join("tkmd.sock")
}

/// Returns the errors.log path.
fn errors_log_path(tmp: &TempDir) -> PathBuf {
    tmp.path().join("data").join("tokenimum").join("errors.log")
}

/// Spawn a fake Unix-domain server that accepts one connection and immediately
/// sends `reply`, then closes. Returns the join handle.
fn fake_server_one_reply(sock_path: &Path, reply: Reply) -> thread::JoinHandle<()> {
    std::fs::create_dir_all(sock_path.parent().unwrap()).unwrap();
    let listener = UnixListener::bind(sock_path).unwrap();
    thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let _req: tkm_proto::Request = read_frame(&mut stream, MAX_REQUEST_FRAME).unwrap();
        write_frame(&mut stream, &reply, MAX_REPLY_FRAME).unwrap();
        stream.flush().unwrap();
    })
}

/// Spawn a fake Unix-domain server that accepts one connection and **never**
/// replies (used for read-timeout test).
fn fake_server_no_reply(sock_path: &Path) -> thread::JoinHandle<()> {
    std::fs::create_dir_all(sock_path.parent().unwrap()).unwrap();
    let listener = UnixListener::bind(sock_path).unwrap();
    thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let mut buf = [0u8; 1024];
            let _ = stream.read(&mut buf);
            // Sleep longer than any read timeout.
            thread::sleep(Duration::from_secs(5));
        }
    })
}

// ── Test: happy path ─────────────────────────────────────────────────────────

/// Happy path: a fake daemon returns a Reply; the hook prints its stdout
/// byte-for-byte. Validate the Request sent by the hook, including cwd precedence.
#[test]
fn test_happy_path_claude_post_tool_use() {
    let tmp = TempDir::new().unwrap();
    let sock = socket_path(&tmp);

    let expected_stdout = r#"{"systemMessage":"x"}"#;
    let reply = Reply {
        version: PROTOCOL_VERSION,
        action: ReplyAction::Nothing,
        stdout: expected_stdout.to_string(),
    };

    // Read fixture payload.
    let fixture = include_str!("../../../fixtures/claude/hooks/PostToolUse-bash.json");

    let received_request: Arc<std::sync::Mutex<Option<tkm_proto::Request>>> =
        Arc::new(std::sync::Mutex::new(None));
    let received_clone = Arc::clone(&received_request);

    std::fs::create_dir_all(sock.parent().unwrap()).unwrap();
    let listener = UnixListener::bind(&sock).unwrap();
    let _srv = {
        let reply = reply.clone();
        thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let req: tkm_proto::Request = read_frame(&mut stream, MAX_REQUEST_FRAME).unwrap();
            *received_clone.lock().unwrap() = Some(req);
            write_frame(&mut stream, &reply, MAX_REPLY_FRAME).unwrap();
            stream.flush().unwrap();
        })
    };

    let mut cmd = hook_cmd(&tmp, "claude", "tool_done");
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = cmd.spawn().unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(fixture.as_bytes())
        .unwrap();
    drop(child.stdin.take());

    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(0), "exit code must be 0");
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        expected_stdout,
        "stdout must equal reply.stdout verbatim"
    );

    // Validate the request the server received.
    let req = received_request.lock().unwrap().take().unwrap();
    assert_eq!(req.version, PROTOCOL_VERSION);
    assert_eq!(req.host, Host::Claude);
    assert_eq!(req.event, EventKind::ToolDone);
    assert_eq!(req.host_event.as_deref(), Some("PostToolUse"));
    // cwd in PostToolUse-bash.json is "~/dev/example"
    assert_eq!(
        req.cwd.as_deref(),
        Some("~/dev/example"),
        "payload cwd must take precedence over process cwd"
    );
    // payload must contain what's in the fixture.
    let v: serde_json::Value = serde_json::from_str(fixture).unwrap();
    assert_eq!(req.payload, v);
    // env must only contain allowlisted keys.
    for k in req.env.keys() {
        assert!(
            k.starts_with("TKM_")
                || k == "CLAUDE_PROJECT_DIR"
                || k == "CODEX_HOME"
                || k == "CLAUDE_CONFIG_DIR"
                || k.starts_with("SUPERORCHESTRATOR_")
                || k == "TERM_PROGRAM",
            "unexpected env key forwarded: {k}"
        );
    }
}

// ── Test: cwd priority ────────────────────────────────────────────────────────

#[test]
fn test_cwd_priority_explicit_payload_and_fallback() {
    let tmp = TempDir::new().unwrap();
    let sock = socket_path(&tmp);

    let received_request: Arc<std::sync::Mutex<Option<tkm_proto::Request>>> =
        Arc::new(std::sync::Mutex::new(None));
    let received_clone = Arc::clone(&received_request);

    std::fs::create_dir_all(sock.parent().unwrap()).unwrap();
    let listener = UnixListener::bind(&sock).unwrap();
    let _srv = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let req: tkm_proto::Request = read_frame(&mut stream, MAX_REQUEST_FRAME).unwrap();
        *received_clone.lock().unwrap() = Some(req);
        let reply = Reply {
            version: PROTOCOL_VERSION,
            action: ReplyAction::Nothing,
            stdout: String::new(),
        };
        write_frame(&mut stream, &reply, MAX_REPLY_FRAME).unwrap();
        stream.flush().unwrap();
    });

    let mut cmd = hook_cmd(&tmp, "claude", "prompt");
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = cmd.spawn().unwrap();
    let payload = r#"{"cwd": "/custom/payload/dir"}"#;
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(payload.as_bytes())
        .unwrap();
    drop(child.stdin.take());

    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(0));

    let req = received_request.lock().unwrap().take().unwrap();
    assert_eq!(req.cwd.as_deref(), Some("/custom/payload/dir"));
}

// ── Test: TKM_DISABLE ────────────────────────────────────────────────────────

#[test]
fn test_tkm_disable_skips_socket() {
    let tmp = TempDir::new().unwrap();
    let sock = socket_path(&tmp);

    std::fs::create_dir_all(sock.parent().unwrap()).unwrap();
    let listener = UnixListener::bind(&sock).unwrap();
    listener
        .set_nonblocking(true)
        .expect("set nonblocking on listener");

    let mut cmd = hook_cmd(&tmp, "claude", "prompt");
    cmd.env("TKM_DISABLE", "1");
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let output = cmd.output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");

    match listener.accept() {
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
        Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {}
        Ok(_) => panic!("fake server received a connection, but TKM_DISABLE was set"),
        Err(e) => {
            let _ = e;
        }
    }
}

#[test]
fn test_tkm_disable_true_variant() {
    let tmp = TempDir::new().unwrap();
    let mut cmd = hook_cmd(&tmp, "cursor", "prompt");
    cmd.env("TKM_DISABLE", "true");
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let output = cmd.output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "{}");
}

// ── Test: daemon down / spawn ─────────────────────────────────────────────────

#[test]
fn test_daemon_down_noop_and_spawn() {
    let tmp = TempDir::new().unwrap();
    let marker = tmp.path().join("daemon_started");
    let script_path = tmp.path().join("tkmd");
    std::fs::write(
        &script_path,
        format!("#!/bin/sh\ntouch '{}'\nsleep 2\n", marker.display()),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let mut cmd = hook_cmd(&tmp, "claude", "tool_done");
    cmd.env("TKM_TKMD_BIN", &script_path);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let start = Instant::now();
    let output = cmd.output().unwrap();
    let elapsed = start.elapsed();

    assert_eq!(output.status.code(), Some(0), "exit code must be 0");
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
    assert!(
        elapsed < Duration::from_millis(500),
        "hook took too long: {elapsed:?}"
    );
    let log = errors_log_path(&tmp);
    assert!(log.exists(), "errors.log should be created");
    let contents = std::fs::read_to_string(&log).unwrap();
    assert!(!contents.is_empty(), "errors.log should have content");

    for _ in 0..20 {
        if marker.exists() {
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    assert!(
        marker.exists(),
        "marker file must be created by spawned tkmd"
    );
}

// ── Test: socket slow / read timeout ─────────────────────────────────────────

#[test]
fn test_socket_slow_read_timeout() {
    let tmp = TempDir::new().unwrap();
    let sock = socket_path(&tmp);
    let _srv = fake_server_no_reply(&sock);

    let mut cmd = hook_cmd(&tmp, "claude", "tool_done");
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let start = Instant::now();
    let output = cmd.output().unwrap();
    let elapsed = start.elapsed();

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
    assert!(
        elapsed < Duration::from_millis(200),
        "hook took too long: {elapsed:?}"
    );
    let log = errors_log_path(&tmp);
    assert!(log.exists(), "errors.log should be written on timeout");
}

// ── Test: garbage / oversize / wrong version reply ───────────────────────────

#[test]
fn test_wrong_version_reply_noop() {
    let tmp = TempDir::new().unwrap();
    let sock = socket_path(&tmp);

    let reply = Reply {
        version: 0xFFFF,
        action: ReplyAction::Nothing,
        stdout: "SHOULD_NOT_APPEAR".to_string(),
    };
    let _srv = fake_server_one_reply(&sock, reply);

    let mut cmd = hook_cmd(&tmp, "claude", "session_start");
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let output = cmd.output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
}

#[test]
fn test_oversize_reply_noop() {
    let tmp = TempDir::new().unwrap();
    let sock = socket_path(&tmp);

    std::fs::create_dir_all(sock.parent().unwrap()).unwrap();
    let listener = UnixListener::bind(&sock).unwrap();
    let _srv = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let _ = read_frame::<_, tkm_proto::Request>(&mut stream, MAX_REQUEST_FRAME);
        let oversize_len = u32::try_from(MAX_REPLY_FRAME + 1).unwrap();
        stream.write_all(&oversize_len.to_be_bytes()).unwrap();
        stream.flush().unwrap();
    });

    let mut cmd = hook_cmd(&tmp, "claude", "prompt");
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let output = cmd.output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
}

#[test]
fn test_garbage_reply_noop() {
    let tmp = TempDir::new().unwrap();
    let sock = socket_path(&tmp);

    std::fs::create_dir_all(sock.parent().unwrap()).unwrap();
    let listener = UnixListener::bind(&sock).unwrap();
    let _srv = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let _ = read_frame::<_, tkm_proto::Request>(&mut stream, MAX_REQUEST_FRAME);
        let body = b"not json at all!";
        let len = u32::try_from(body.len()).unwrap().to_be_bytes();
        stream.write_all(&len).unwrap();
        stream.write_all(body).unwrap();
        stream.flush().unwrap();
    });

    let mut cmd = hook_cmd(&tmp, "cursor", "tool_done");
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let output = cmd.output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "{}");
}

// ── Test: bad args => logs error and exits 0 ─────────────────────────────────

#[test]
fn test_cursor_invalid_event_returns_cursor_noop() {
    let tmp = TempDir::new().unwrap();
    let output = hook_cmd(&tmp, "cursor", "invalid")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"{}");
    assert_eq!(
        std::fs::read_to_string(errors_log_path(&tmp))
            .unwrap()
            .lines()
            .count(),
        1
    );
}

#[test]
fn test_non_utf8_argument_does_not_panic() {
    let tmp = TempDir::new().unwrap();
    let output = hook_cmd(&tmp, "cursor", "tool_done")
        .arg(std::ffi::OsString::from_vec(vec![0xff]))
        .env("TKM_DISABLE", "1")
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"{}");
    assert!(output.stderr.is_empty());
}

#[test]
fn test_stdin_timeout_fails_open() {
    let tmp = TempDir::new().unwrap();
    let mut child = hook_cmd(&tmp, "cursor", "tool_done")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let start = Instant::now();
    while child.try_wait().unwrap().is_none() && start.elapsed() < Duration::from_secs(2) {
        thread::sleep(Duration::from_millis(10));
    }
    if child.try_wait().unwrap().is_none() {
        child.kill().unwrap();
        child.wait().unwrap();
        panic!("hook blocked on stdin");
    }
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"{}");
    let log = std::fs::read_to_string(errors_log_path(&tmp)).unwrap();
    assert_eq!(log.lines().count(), 1);
    assert!(log.contains("stdin"));
}

#[test]
fn test_oversize_stdin_fails_open() {
    let tmp = TempDir::new().unwrap();
    let mut input = tempfile::tempfile().unwrap();
    input.write_all(&vec![b'x'; MAX_REQUEST_FRAME + 1]).unwrap();
    use std::io::Seek;
    input.rewind().unwrap();
    let output = hook_cmd(&tmp, "cursor", "tool_done")
        .stdin(Stdio::from(input))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(output.stdout, b"{}");
    let log = std::fs::read_to_string(errors_log_path(&tmp)).unwrap();
    assert_eq!(log.lines().count(), 1);
    assert!(log.contains("stdin"));
}

#[test]
fn test_bad_args_unknown_host_logs_error_and_prints_empty() {
    let tmp = TempDir::new().unwrap();
    let mut cmd = hook_cmd(&tmp, "notahost", "tool_done");
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let output = cmd.output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
    let log = errors_log_path(&tmp);
    assert!(
        log.exists(),
        "errors.log should be written for unknown host"
    );
    let contents = std::fs::read_to_string(&log).unwrap();
    assert!(!contents.is_empty(), "errors.log should have content");
}

#[test]
fn test_bad_args_missing_event_logs_error_and_prints_noop() {
    let tmp = TempDir::new().unwrap();
    let mut cmd = Command::new(BIN);
    cmd.arg("--host").arg("cursor");
    cmd.env_clear();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    cmd.env("HOME", &home);
    cmd.env("XDG_RUNTIME_DIR", tmp.path().join("run"));
    cmd.env("XDG_DATA_HOME", tmp.path().join("data"));
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let output = cmd.output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "{}");
    let log = errors_log_path(&tmp);
    assert!(
        log.exists(),
        "errors.log should be written for missing event"
    );
    let contents = std::fs::read_to_string(&log).unwrap();
    assert!(!contents.is_empty(), "errors.log should have content");
}

#[test]
fn test_bad_args_missing_all_args_logs_error_and_prints_empty() {
    let tmp = TempDir::new().unwrap();
    let mut cmd = Command::new(BIN);
    cmd.env_clear();
    let home = tmp.path().join("home");
    std::fs::create_dir_all(&home).unwrap();
    cmd.env("HOME", &home);
    cmd.env("XDG_RUNTIME_DIR", tmp.path().join("run"));
    cmd.env("XDG_DATA_HOME", tmp.path().join("data"));
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let output = cmd.output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
    let log = errors_log_path(&tmp);
    assert!(
        log.exists(),
        "errors.log should be written when no args provided"
    );
    let contents = std::fs::read_to_string(&log).unwrap();
    assert!(!contents.is_empty(), "errors.log should have content");
}

// ── Test: Paths::from_env() None (no HOME) ───────────────────────────────────

#[test]
fn test_no_home_paths_none_no_tmp_claude() {
    let mut cmd = Command::new(BIN);
    cmd.arg("--host")
        .arg("claude")
        .arg("--event")
        .arg("tool_done");
    cmd.env_clear();
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = cmd.output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
    assert_eq!(String::from_utf8_lossy(&output.stderr), "");
}

#[test]
fn test_no_home_paths_none_no_tmp_cursor() {
    let mut cmd = Command::new(BIN);
    cmd.arg("--host")
        .arg("cursor")
        .arg("--event")
        .arg("tool_done");
    cmd.env_clear();
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = cmd.output().unwrap();
    assert_eq!(output.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "{}");
    assert_eq!(String::from_utf8_lossy(&output.stderr), "");
}

// ── Test: env allowlist ───────────────────────────────────────────────────────

#[test]
fn test_env_allowlist_only_tkm_vars_forwarded() {
    let tmp = TempDir::new().unwrap();
    let sock = socket_path(&tmp);

    let received: Arc<std::sync::Mutex<Option<tkm_proto::Request>>> =
        Arc::new(std::sync::Mutex::new(None));
    let recv_clone = Arc::clone(&received);

    std::fs::create_dir_all(sock.parent().unwrap()).unwrap();
    let listener = UnixListener::bind(&sock).unwrap();
    let _srv = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let req: tkm_proto::Request = read_frame(&mut stream, MAX_REQUEST_FRAME).unwrap();
        *recv_clone.lock().unwrap() = Some(req);
        let reply = Reply {
            version: PROTOCOL_VERSION,
            action: ReplyAction::Nothing,
            stdout: String::new(),
        };
        write_frame(&mut stream, &reply, MAX_REPLY_FRAME).unwrap();
        stream.flush().unwrap();
    });

    let mut cmd = hook_cmd(&tmp, "claude", "prompt");
    cmd.env("TKM_FOO", "bar");
    cmd.env("TKM_API_KEY", "sekret");
    cmd.env("TKM_TOKEN", "tok");
    cmd.env("TKM_SECRET", "sec");
    cmd.env("CLAUDE_PROJECT_DIR", "/some/path");
    cmd.env("HOME_PRIVATE", "nope");
    cmd.env("TERM_PROGRAM", "iterm2");
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let output = cmd.output().unwrap();
    assert_eq!(output.status.code(), Some(0));

    let req = received.lock().unwrap().take().unwrap();
    assert_eq!(req.env.get("TKM_FOO").map(String::as_str), Some("bar"));
    assert!(req.env.contains_key("CLAUDE_PROJECT_DIR"));
    assert!(req.env.contains_key("TERM_PROGRAM"));
    assert!(
        !req.env.contains_key("TKM_API_KEY"),
        "KEY var must be filtered"
    );
    assert!(
        !req.env.contains_key("TKM_TOKEN"),
        "TOKEN var must be filtered"
    );
    assert!(
        !req.env.contains_key("TKM_SECRET"),
        "SECRET var must be filtered"
    );
    assert!(!req.env.contains_key("HOME_PRIVATE"));
}

// ── Test: empty stdin => payload null ────────────────────────────────────────

#[test]
fn test_empty_stdin_payload_null() {
    let tmp = TempDir::new().unwrap();
    let sock = socket_path(&tmp);

    let received: Arc<std::sync::Mutex<Option<tkm_proto::Request>>> =
        Arc::new(std::sync::Mutex::new(None));
    let recv_clone = Arc::clone(&received);

    std::fs::create_dir_all(sock.parent().unwrap()).unwrap();
    let listener = UnixListener::bind(&sock).unwrap();
    let _srv = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let req: tkm_proto::Request = read_frame(&mut stream, MAX_REQUEST_FRAME).unwrap();
        *recv_clone.lock().unwrap() = Some(req);
        let reply = Reply {
            version: PROTOCOL_VERSION,
            action: ReplyAction::Nothing,
            stdout: String::new(),
        };
        write_frame(&mut stream, &reply, MAX_REPLY_FRAME).unwrap();
        stream.flush().unwrap();
    });

    let mut cmd = hook_cmd(&tmp, "claude", "session_start");
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let output = cmd.output().unwrap();
    assert_eq!(output.status.code(), Some(0));

    let req = received.lock().unwrap().take().unwrap();
    assert_eq!(req.payload, serde_json::Value::Null);
}

// ── Test: non-JSON stdin => fail open ─────────────────────────────────────────

#[test]
fn test_non_json_stdin_fail_open() {
    let tmp = TempDir::new().unwrap();
    let mut cmd = hook_cmd(&tmp, "claude", "tool_done");
    let script_path = tmp.path().join("tkmd_noop");
    std::fs::write(&script_path, "#!/bin/sh\nexit 0\n").unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    cmd.env("TKM_TKMD_BIN", &script_path);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = cmd.spawn().unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(b"not valid json!!!")
        .unwrap();
    drop(child.stdin.take());
    let output = child.wait_with_output().unwrap();
    assert_eq!(output.status.code(), Some(0));
}

// ── Test: spawn storm guard ───────────────────────────────────────────────────

#[test]
fn test_spawn_storm_guard() {
    let tmp = TempDir::new().unwrap();
    let counter_path = tmp.path().join("spawn_count");
    std::fs::write(&counter_path, "0").unwrap();

    let script_path = tmp.path().join("tkmd_counter");
    std::fs::write(
        &script_path,
        format!(
            "#!/bin/sh\nn=$(cat '{c}' 2>/dev/null || echo 0); echo $((n+1)) > '{c}'; sleep 10\n",
            c = counter_path.display()
        ),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    let mut cmd1 = hook_cmd(&tmp, "claude", "tool_done");
    cmd1.env("TKM_TKMD_BIN", &script_path);
    cmd1.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let out1 = cmd1.output().unwrap();
    assert_eq!(out1.status.code(), Some(0));

    thread::sleep(Duration::from_millis(200));

    let mut cmd2 = hook_cmd(&tmp, "claude", "tool_done");
    cmd2.env("TKM_TKMD_BIN", &script_path);
    cmd2.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let out2 = cmd2.output().unwrap();
    assert_eq!(out2.status.code(), Some(0));

    thread::sleep(Duration::from_millis(200));

    let count: u32 = std::fs::read_to_string(&counter_path)
        .unwrap_or_default()
        .trim()
        .parse()
        .unwrap_or(0);
    assert_eq!(
        count, 1,
        "storm guard should prevent second spawn; got {count}"
    );
}

// ── Test: slow write timeout ─────────────────────────────────────────────────

/// Fake server accepts connection but never reads: hook writing a large payload
/// (~4 MiB) for `tool_done` must not hang; write timeout (50 ms floor) bounds it.
/// Hook exits 0 with noop within 500 ms and writes errors.log.
#[test]
fn test_socket_slow_write_timeout_large_payload() {
    let tmp = TempDir::new().unwrap();
    let sock = socket_path(&tmp);

    std::fs::create_dir_all(sock.parent().unwrap()).unwrap();
    let listener = UnixListener::bind(&sock).unwrap();
    let _srv = thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            // Never read from stream; keep connection open.
            thread::sleep(Duration::from_secs(5));
            let mut buf = [0u8; 1];
            let _ = stream.read(&mut buf);
        }
    });

    let large_string = "a".repeat(4 * 1024 * 1024);
    let payload = format!(r#"{{"hook_event_name":"PostToolUse","data":"{large_string}"}}"#);

    let mut cmd = hook_cmd(&tmp, "claude", "tool_done");
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let start = Instant::now();
    let mut child = cmd.spawn().unwrap();
    let mut stdin = child.stdin.take().unwrap();
    let feeder = thread::spawn(move || {
        let _ = stdin.write_all(payload.as_bytes());
    });

    let output = child.wait_with_output().unwrap();
    let elapsed = start.elapsed();
    let _ = feeder.join();

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
    assert!(
        elapsed < Duration::from_millis(500),
        "hook took too long on slow write: {elapsed:?}"
    );
    let log = errors_log_path(&tmp);
    assert!(
        log.exists(),
        "errors.log should be written on write timeout"
    );
}

// ── Test: full backlog bounded connect ────────────────────────────────────────

#[test]
fn test_connect_full_backlog_bounded_treated_as_busy() {
    let tmp = TempDir::new().unwrap();
    let sock = socket_path(&tmp);

    std::fs::create_dir_all(sock.parent().unwrap()).unwrap();
    let _listener = UnixListener::bind(&sock).unwrap();

    let mut holders = Vec::new();
    for _ in 0..500 {
        match UnixStream::connect(&sock) {
            Ok(s) => {
                let _ = s.set_nonblocking(true);
                holders.push(s);
            }
            Err(_) => break,
        }
    }

    let mut cmd = hook_cmd(&tmp, "claude", "tool_done");
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    let start = Instant::now();
    let output = cmd.output().unwrap();
    let elapsed = start.elapsed();

    assert_eq!(output.status.code(), Some(0));
    assert_eq!(String::from_utf8_lossy(&output.stdout), "");
    assert!(
        elapsed < Duration::from_millis(500),
        "hook took too long on full backlog connect: {elapsed:?}"
    );
    drop(holders);
}

#[test]
fn test_host_event_flag_overrides_payload_hook_event_name() {
    let tmp = TempDir::new().unwrap();
    let sock = socket_path(&tmp);
    let received = Arc::new(std::sync::Mutex::new(None));
    let received_clone = Arc::clone(&received);
    std::fs::create_dir_all(sock.parent().unwrap()).unwrap();
    let listener = UnixListener::bind(&sock).unwrap();
    let _srv = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let req: tkm_proto::Request = read_frame(&mut stream, MAX_REQUEST_FRAME).unwrap();
        *received_clone.lock().unwrap() = Some(req);
        let reply = Reply {
            version: PROTOCOL_VERSION,
            action: ReplyAction::Nothing,
            stdout: String::new(),
        };
        write_frame(&mut stream, &reply, MAX_REPLY_FRAME).unwrap();
    });
    let mut cmd = hook_cmd(&tmp, "claude", "prompt");
    cmd.arg("--host-event").arg("PiBeforeTool");
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut child = cmd.spawn().unwrap();
    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(br#"{"hook_event_name":"OtherEvent"}"#)
        .unwrap();
    drop(child.stdin.take());
    assert_eq!(child.wait().unwrap().code(), Some(0));
    assert_eq!(
        received
            .lock()
            .unwrap()
            .as_ref()
            .unwrap()
            .host_event
            .as_deref(),
        Some("PiBeforeTool")
    );
}

#[test]
fn test_payload_flag_does_not_read_open_stdin() {
    let tmp = TempDir::new().unwrap();
    let sock = socket_path(&tmp);
    let received = Arc::new(std::sync::Mutex::new(None));
    let received_clone = Arc::clone(&received);
    std::fs::create_dir_all(sock.parent().unwrap()).unwrap();
    let listener = UnixListener::bind(&sock).unwrap();
    let _srv = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let req: tkm_proto::Request = read_frame(&mut stream, MAX_REQUEST_FRAME).unwrap();
        *received_clone.lock().unwrap() = Some(req);
        let reply = Reply {
            version: PROTOCOL_VERSION,
            action: ReplyAction::Nothing,
            stdout: String::new(),
        };
        write_frame(&mut stream, &reply, MAX_REPLY_FRAME).unwrap();
    });
    let mut cmd = hook_cmd(&tmp, "claude", "prompt");
    cmd.arg("--payload").arg(r#"{"source":"flag"}"#);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let start = Instant::now();
    let mut child = cmd.spawn().unwrap();
    // Keep the pipe open until after the hook exits. Eager stdin reads hang here.
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        assert!(
            start.elapsed() < Duration::from_millis(500),
            "hook waited on open stdin"
        );
        thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(status.code(), Some(0));
    drop(child.stdin.take());
    assert_eq!(
        received.lock().unwrap().as_ref().unwrap().payload,
        serde_json::json!({"source":"flag"})
    );
}

#[test]
fn test_invalid_payload_flag_sends_null_and_logs_error() {
    let tmp = TempDir::new().unwrap();
    let sock = socket_path(&tmp);
    let received = Arc::new(std::sync::Mutex::new(None));
    let received_clone = Arc::clone(&received);
    std::fs::create_dir_all(sock.parent().unwrap()).unwrap();
    let listener = UnixListener::bind(&sock).unwrap();
    let _srv = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let req: tkm_proto::Request = read_frame(&mut stream, MAX_REQUEST_FRAME).unwrap();
        *received_clone.lock().unwrap() = Some(req);
        let reply = Reply {
            version: PROTOCOL_VERSION,
            action: ReplyAction::Nothing,
            stdout: String::new(),
        };
        write_frame(&mut stream, &reply, MAX_REPLY_FRAME).unwrap();
    });
    let mut cmd = hook_cmd(&tmp, "claude", "prompt");
    cmd.args(["--payload", "not-json"]);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    assert_eq!(cmd.output().unwrap().status.code(), Some(0));
    assert_eq!(
        received.lock().unwrap().as_ref().unwrap().payload,
        serde_json::Value::Null
    );
    let log = std::fs::read_to_string(errors_log_path(&tmp)).unwrap();
    assert_eq!(log.lines().count(), 1);
}
