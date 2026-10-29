#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! `tkm-hook` client library (E4.1).
//!
//! All hook logic lives here so that integration and unit tests can exercise
//! it without a real daemon. The binary `src/main.rs` is a thin wrapper.
//!
//! ## Architecture
//!
//! ```text
//! main()
//!   └─ catch_unwind(run_inner())
//!        ├─ parse_args()          – hand-rolled, no clap
//!        ├─ check_disable()       – TKM_DISABLE early exit
//!        ├─ read_stdin()          – JSON or null
//!        ├─ build_env()           – allowlisted env keys only
//!        ├─ build_request()       – fills Request struct (payload cwd beats process cwd)
//!        ├─ connect_and_exchange()– sync Unix-domain socket (bounds: connect 2 ms, read timeout)
//!        │    ├─ on miss: spawn_daemon_detached() guarded by 2s marker
//!        │    └─ on reply: verify version, print stdout verbatim
//!        └─ on any error: log, print host noop, exit 0
//! ```

use std::collections::BTreeMap;
use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use tkm_config::paths::{ERRORS_LOG_MAX_BYTES, ErrorLog};
use tkm_proto::{
    CONNECT_TIMEOUT, EventKind, Host, MAX_REPLY_FRAME, MAX_REQUEST_FRAME, PROTOCOL_VERSION, Reply,
    Request,
};

// ── Env allowlist ─────────────────────────────────────────────────────────────

/// Exact variable names that are always forwarded (case-sensitive).
///
/// Documented in ADR 0014. The list is intentionally small; only variables
/// that the daemon adapter actually uses to reproduce session context are
/// included. Any `TKM_*` variable whose name contains `KEY`, `TOKEN`, or
/// `SECRET` (case-insensitive) is silently dropped by the prefix rule below.
pub const ENV_ALLOWLIST: &[&str] = &[
    "CLAUDE_PROJECT_DIR",
    "CLAUDE_CONFIG_DIR",
    "CODEX_HOME",
    "TERM_PROGRAM",
];

/// Prefix rule: forward `TKM_*` unless the name contains a secret marker.
const TKM_PREFIX: &str = "TKM_";

/// Secret markers: any `TKM_*` key whose name (uppercased) contains one of
/// these substrings is suppressed.
const SECRET_MARKERS: &[&str] = &["KEY", "TOKEN", "SECRET"];

/// Prefix for SUPERORCHESTRATOR vars (forwarded as documented in SPEC 9.3).
const SO_PREFIX: &str = "SUPERORCHESTRATOR_";

/// Returns `true` when an env key should be forwarded.
#[must_use]
pub fn is_allowed_env_key(key: &str) -> bool {
    if ENV_ALLOWLIST.contains(&key) {
        return true;
    }
    let upper = key.to_ascii_uppercase();
    if upper.starts_with(TKM_PREFIX) {
        return !SECRET_MARKERS.iter().any(|m| upper.contains(m));
    }
    if upper.starts_with(SO_PREFIX) {
        return true;
    }
    false
}

// ── Disable check ─────────────────────────────────────────────────────────────

/// Returns `true` when `TKM_DISABLE` is set to a truthy value.
///
/// Truthy values (case-insensitive): `1`, `true`, `yes`.
#[must_use]
pub fn is_disabled(val: &str) -> bool {
    matches!(val.to_ascii_lowercase().as_str(), "1" | "true" | "yes")
}

// ── Argument parsing ──────────────────────────────────────────────────────────

/// Parsed hook command-line arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookArgs {
    /// Host identifier.
    pub host: Host,
    /// Event kind.
    pub event: EventKind,
    /// Optional host lifecycle event override.
    pub host_event: Option<String>,
    /// Optional JSON payload supplied on the command line.
    pub payload: Option<String>,
}

/// Parse `--host <host> --event <event>` from raw args (without program name).
///
/// Returns `None` if either argument is missing or unrecognized.
#[must_use]
pub fn parse_args<I, S>(args: I) -> Option<HookArgs>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let args: Vec<String> = args.into_iter().map(|s| s.as_ref().to_owned()).collect();
    let (host, event) = parse_host_and_event(args.iter());
    let mut host_event = None;
    let mut payload = None;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--host-event" || args[i] == "--payload" {
            let is_host_event = args[i] == "--host-event";
            if args.get(i + 1).is_some_and(|v| !v.starts_with("--")) {
                if is_host_event {
                    host_event = args.get(i + 1).cloned();
                } else {
                    payload = args.get(i + 1).cloned();
                }
                i += 1;
            }
        }
        i += 1;
    }
    Some(HookArgs {
        host: host?,
        event: event?,
        host_event,
        payload,
    })
}

/// Parse host and event independently from raw args.
///
/// Returns `(Some(Host), ...)` if `--host <valid>` is present, even if
/// `--event` is missing or invalid.
#[must_use]
pub fn parse_host_and_event<I, S>(args: I) -> (Option<Host>, Option<EventKind>)
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    let args: Vec<String> = args.into_iter().map(|s| s.as_ref().to_owned()).collect();
    let mut host: Option<Host> = None;
    let mut event: Option<EventKind> = None;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--host" if args.get(i + 1).is_some_and(|v| !v.starts_with("--")) => {
                i += 1;
                host = args[i].parse::<Host>().ok();
            }
            "--event" if args.get(i + 1).is_some_and(|v| !v.starts_with("--")) => {
                i += 1;
                event = args[i].parse::<EventKind>().ok();
            }
            _ => {}
        }
        i += 1;
    }
    (host, event)
}

// ── Stdin reading ─────────────────────────────────────────────────────────────

/// Read all bytes from `reader`. Returns `serde_json::Value::Null` for empty
/// input; returns `Null` on non-UTF-8 or non-JSON input (fail open).
#[must_use]
pub fn read_payload<R: Read>(mut reader: R) -> serde_json::Value {
    let mut buf = Vec::new();
    if reader.read_to_end(&mut buf).is_err() || buf.is_empty() {
        return serde_json::Value::Null;
    }
    serde_json::from_slice(&buf).unwrap_or(serde_json::Value::Null)
}

// ── Request building ──────────────────────────────────────────────────────────

/// Build the `Request` from parsed args, payload, and filtered env.
///
/// Working directory resolution:
/// Payload `cwd` string takes precedence; process cwd is only used as a fallback.
#[must_use]
pub fn build_request(
    host: Host,
    event: EventKind,
    payload: serde_json::Value,
    env: BTreeMap<String, String>,
    fallback_cwd: Option<String>,
) -> Request {
    let host_event = payload
        .as_object()
        .and_then(|o| o.get("hook_event_name"))
        .and_then(|v| v.as_str())
        .map(str::to_owned);

    let cwd = payload
        .as_object()
        .and_then(|o| o.get("cwd"))
        .and_then(|v| v.as_str())
        .map(str::to_owned)
        .or(fallback_cwd);

    Request {
        version: PROTOCOL_VERSION,
        host,
        event,
        host_event,
        payload,
        env,
        cwd,
    }
}

// ── Spawn storm guard ─────────────────────────────────────────────────────────

/// Age threshold under which a spawn marker is considered "fresh" (2 seconds).
const SPAWN_MARKER_MAX_AGE: Duration = Duration::from_secs(2);

/// Path of the spawn-storm guard marker file.
#[must_use]
fn spawn_marker_path(socket_path: &Path) -> PathBuf {
    socket_path
        .parent()
        .unwrap_or(Path::new("/tmp"))
        .join("tkmd.spawn")
}

/// Returns `true` when a spawn attempt was recorded in the last 2 seconds.
#[must_use]
fn spawn_recently(socket_path: &Path) -> bool {
    let marker = spawn_marker_path(socket_path);
    std::fs::metadata(&marker)
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .is_some_and(|age| age < SPAWN_MARKER_MAX_AGE)
}

/// Touch the spawn marker so subsequent invocations within 2 s skip spawning.
fn touch_spawn_marker(socket_path: &Path) {
    let marker = spawn_marker_path(socket_path);
    if let Some(parent) = marker.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&marker, b"");
}

// ── Daemon spawning ───────────────────────────────────────────────────────────

/// Resolve the `tkmd` binary path:
/// 1. `TKM_TKMD_BIN` env (for tests).
/// 2. Same directory as `current_exe()`.
/// 3. `tkmd` on PATH (fallback).
#[must_use]
fn resolve_tkmd_bin() -> PathBuf {
    if let Ok(v) = std::env::var("TKM_TKMD_BIN") {
        return PathBuf::from(v);
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        let candidate = dir.join("tkmd");
        if candidate.exists() {
            return candidate;
        }
    }
    PathBuf::from("tkmd")
}

/// Spawn `tkmd` detached, not waiting for it.
///
/// Uses `process_group(0)` so the daemon survives hook process exit.
/// Stdin/stdout/stderr are all set to `Stdio::null()`.
///
/// Returns `true` if the spawn succeeded.
#[must_use]
pub fn spawn_daemon_detached(socket_path: &Path) -> bool {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};

    if spawn_recently(socket_path) {
        return false;
    }
    touch_spawn_marker(socket_path);

    let bin = resolve_tkmd_bin();
    let result = Command::new(bin)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0)
        .spawn();
    result.is_ok()
}

// ── Connect and exchange ──────────────────────────────────────────────────────

/// Outcome of a bounded connect attempt.
#[derive(Debug)]
pub enum ConnectResult {
    /// Connected successfully.
    Connected(UnixStream),
    /// Daemon is down (socket missing or connection refused).
    DaemonDown,
    /// Daemon is busy (listen backlog full / connect timed out).
    DaemonBusy,
    /// Connection failed with another I/O error.
    Error(String),
}

/// Connect to a Unix stream socket with a bounded timeout in safe Rust.
///
/// Spawns a background thread running `connect_fn`, communicating via an `mpsc`
/// channel with `recv_timeout(timeout)`. On timeout, returns `ConnectResult::DaemonBusy`
/// and does not join the thread (the process exit terminates it).
#[must_use]
pub fn connect_bounded<F>(timeout: Duration, connect_fn: F) -> ConnectResult
where
    F: FnOnce() -> io::Result<UnixStream> + Send + 'static,
{
    let (tx, rx) = std::sync::mpsc::channel();
    let _ = std::thread::spawn(move || {
        let res = connect_fn();
        let _ = tx.send(res);
    });

    match rx.recv_timeout(timeout) {
        Ok(Ok(stream)) => ConnectResult::Connected(stream),
        Ok(Err(e))
            if e.kind() == io::ErrorKind::NotFound
                || e.kind() == io::ErrorKind::ConnectionRefused =>
        {
            ConnectResult::DaemonDown
        }
        Ok(Err(e)) if e.kind() == io::ErrorKind::WouldBlock => ConnectResult::DaemonBusy,
        Ok(Err(e)) => ConnectResult::Error(format!("connect: {e}")),
        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => ConnectResult::DaemonBusy,
        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
            ConnectResult::Error("connect thread died".to_string())
        }
    }
}

/// Outcome of one hook exchange with the daemon.
pub enum ExchangeOutcome {
    /// Daemon replied with a valid, version-matching reply.
    Reply(Reply),
    /// Daemon replied but version did not match; fall back to noop.
    VersionMismatch,
    /// Socket missing or connection refused; caller should invoke spawner.
    DaemonDown,
    /// Daemon is busy (listen backlog full / `WouldBlock`); caller should not spawn.
    DaemonBusy,
    /// I/O or frame error during exchange.
    Error(String),
}

/// Timeout bound for connecting to the daemon socket (10 ms).
///
/// Provides a small margin above the 2 ms `CONNECT_TIMEOUT` target to absorb
/// thread spawn jitter on busy systems, while strictly bounding hook latency.
const CONNECT_BOUND: Duration = Duration::from_millis(10);

/// Floor for socket write timeout (50 ms).
const WRITE_TIMEOUT_FLOOR: Duration = Duration::from_millis(50);

/// Connect to the daemon socket and perform one synchronous exchange.
///
/// Bounded by `CONNECT_BOUND` (10 ms) on connect, `event.read_timeout().max(50 ms)`
/// on write, and `event.read_timeout()` on read.
#[must_use]
pub fn connect_and_exchange(socket_path: &Path, request: &Request) -> ExchangeOutcome {
    let _ = CONNECT_TIMEOUT;
    let path = socket_path.to_path_buf();
    let mut stream = match connect_bounded(CONNECT_BOUND, move || UnixStream::connect(&path)) {
        ConnectResult::Connected(s) => s,
        ConnectResult::DaemonDown => return ExchangeOutcome::DaemonDown,
        ConnectResult::DaemonBusy => return ExchangeOutcome::DaemonBusy,
        ConnectResult::Error(e) => return ExchangeOutcome::Error(e),
    };

    let write_timeout = request.event.read_timeout().max(WRITE_TIMEOUT_FLOOR);
    if let Err(e) = stream.set_write_timeout(Some(write_timeout)) {
        return ExchangeOutcome::Error(format!("set_write_timeout: {e}"));
    }

    if let Err(e) = tkm_proto::write_frame(&mut stream, request, MAX_REQUEST_FRAME) {
        return ExchangeOutcome::Error(format!("write_frame: {e}"));
    }
    if let Err(e) = stream.flush() {
        return ExchangeOutcome::Error(format!("flush: {e}"));
    }

    let timeout = request.event.read_timeout();
    if let Err(e) = stream.set_read_timeout(Some(timeout)) {
        return ExchangeOutcome::Error(format!("set_read_timeout: {e}"));
    }

    let reply: Reply = match tkm_proto::read_frame(&mut stream, MAX_REPLY_FRAME) {
        Ok(r) => r,
        Err(e) => return ExchangeOutcome::Error(format!("read_frame: {e}")),
    };

    if reply.version != PROTOCOL_VERSION {
        return ExchangeOutcome::VersionMismatch;
    }
    ExchangeOutcome::Reply(reply)
}

// ── Error logging ─────────────────────────────────────────────────────────────

/// Append one line to the error log at `log_path` using `ErrorLog` (best-effort, never panics).
pub fn append_error_log(log_path: &Path, message: &str) {
    let log = ErrorLog::new(log_path.to_path_buf(), ERRORS_LOG_MAX_BYTES);
    let _ = log.append(&format!("tkm-hook: {message}"));
}

// ── Panic guard ───────────────────────────────────────────────────────────────

/// Run `f`, catching any panic. On panic, write the host's noop reply to `out`.
///
/// Replaces the default panic hook with a no-op hook so that no panic trace
/// escapes to stderr.
pub fn run_with_panic_guard<F, W>(host: Host, f: F, out: &mut W)
where
    F: FnOnce(),
    W: Write,
{
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    std::panic::set_hook(prev);

    if result.is_err() {
        let _ = out.write_all(host.noop_reply().as_bytes());
        let _ = out.flush();
    }
}

// ── Main logic ────────────────────────────────────────────────────────────────

// Mirrors tkm_hosts::env_markers; checked against that API in tests.
const fn host_env_markers(host: Host) -> &'static [&'static str] {
    match host {
        Host::Agy => &["ANTIGRAVITY_CONVERSATION_ID"],
        Host::Cursor => &[
            "CURSOR_PROJECT_DIR",
            "CURSOR_VERSION",
            "CURSOR_TRANSCRIPT_PATH",
        ],
        Host::Kimi => &["KIMI_SHARE_DIR"],
        Host::Pi => &["PI_CODING_AGENT_DIR"],
        Host::Opencode => &["OPENCODE_DB", "XDG_DATA_HOME"],
        Host::Deepseek => &[
            "CODEWHALE_HOME",
            "DEEPSEEK_SESSION_ID",
            "CODEWHALE_SESSION_ID",
            "DEEPSEEK_WORKSPACE",
            "DEEPSEEK_MODEL",
            "DEEPSEEK_MODE",
            "DEEPSEEK_TOOL_NAME",
            "DEEPSEEK_TOOL_ARGS",
            "DEEPSEEK_TOOL_CALL_ID",
            "CODEWHALE_TOOL_CALL_ID",
            "DEEPSEEK_TOOL_RESULT",
            "DEEPSEEK_TOOL_SUCCESS",
            "DEEPSEEK_TOOL_EXIT_CODE",
            "DEEPSEEK_TOOL_STATUS",
            "DEEPSEEK_TOTAL_TOKENS",
        ],
        _ => &[],
    }
}

fn is_host_env_key(host: Host, key: &str) -> bool {
    host_env_markers(host).iter().any(|marker| {
        if marker.ends_with('_') {
            key.starts_with(*marker)
        } else {
            key == *marker
        }
    })
}

/// Collect filtered env vars from an iterator of `(key, value)` pairs.
#[must_use]
pub fn collect_env<I>(iter: I) -> BTreeMap<String, String>
where
    I: IntoIterator<Item = (String, String)>,
{
    iter.into_iter()
        .filter(|(k, _)| is_allowed_env_key(k))
        .collect()
}

/// Core hook logic. Injectable for testing.
///
/// # Parameters
/// - `args`: raw argument strings (without the program name).
/// - `env_iter`: iterator over `(key, value)` env pairs.
/// - `stdin_bytes`: raw stdin bytes (ignored when `--payload` is provided).
/// - `socket_path`: path to the daemon socket (if resolved).
/// - `log_path`: path to the error log (if resolved).
/// - `out`: where to write the reply or noop.
pub fn run_hook<I, S, E>(
    args: I,
    env_iter: E,
    stdin_bytes: &[u8],
    socket_path: Option<&Path>,
    log_path: Option<&Path>,
    out: &mut dyn Write,
) where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
    E: IntoIterator<Item = (String, String)>,
{
    run_hook_with_stdin(
        args,
        env_iter,
        || stdin_bytes.to_vec(),
        socket_path,
        log_path,
        out,
    );
}

/// Core hook logic with lazy stdin access. The callback is called only when
/// `--payload` is absent, allowing callers to avoid blocking on unused stdin.
/// Other parameters and fail-open behavior match [`run_hook`].
pub fn run_hook_with_stdin<I, S, E, F>(
    args: I,
    env_iter: E,
    read_stdin: F,
    socket_path: Option<&Path>,
    log_path: Option<&Path>,
    out: &mut dyn Write,
) where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
    E: IntoIterator<Item = (String, String)>,
    F: FnOnce() -> Vec<u8>,
{
    run_hook_with_fallible_stdin(
        args,
        env_iter,
        || Ok(read_stdin()),
        socket_path,
        log_path,
        out,
    );
}

/// Like [`run_hook_with_stdin`], but a stdin failure logs once and returns the host's no-op.
pub fn run_hook_with_fallible_stdin<I, S, E, F>(
    args: I,
    env_iter: E,
    read_stdin: F,
    socket_path: Option<&Path>,
    log_path: Option<&Path>,
    out: &mut dyn Write,
) where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
    E: IntoIterator<Item = (String, String)>,
    F: FnOnce() -> Result<Vec<u8>, String>,
{
    let args: Vec<String> = args.into_iter().map(|s| s.as_ref().to_owned()).collect();
    let env: Vec<(String, String)> = env_iter.into_iter().collect();

    let parsed = parse_args(&args);
    let (parsed_host, parsed_event) = parse_host_and_event(&args);

    let (host, event) = match (parsed_host, parsed_event) {
        (Some(h), Some(e)) => (h, e),
        (Some(h), None) => {
            if let Some(log) = log_path {
                append_error_log(log, "missing or invalid --event argument");
            }
            let _ = out.write_all(h.noop_reply().as_bytes());
            return;
        }
        (None, _) => {
            if let Some(log) = log_path {
                append_error_log(log, "unknown host or invalid arguments");
            }
            let _ = out.write_all(b"");
            return;
        }
    };

    let disabled = env
        .iter()
        .find(|(k, _)| k == "TKM_DISABLE")
        .is_some_and(|(_, v)| is_disabled(v));

    if disabled {
        let _ = out.write_all(host.noop_reply().as_bytes());
        return;
    }

    let Some(socket_path) = socket_path else {
        let _ = out.write_all(host.noop_reply().as_bytes());
        return;
    };

    let Some(parsed) = parsed else {
        return;
    };
    let payload = if let Some(raw_payload) = parsed.payload {
        if let Ok(payload) = serde_json::from_str(&raw_payload) {
            payload
        } else {
            if let Some(log) = log_path {
                append_error_log(log, "invalid --payload JSON");
            }
            serde_json::Value::Null
        }
    } else {
        let bytes = match read_stdin() {
            Ok(bytes) => bytes,
            Err(error) => {
                if let Some(log) = log_path {
                    append_error_log(log, &error);
                }
                let _ = out.write_all(host.noop_reply().as_bytes());
                return;
            }
        };
        read_payload(std::io::Cursor::new(bytes))
    };
    let filtered_env = env
        .into_iter()
        .filter(|(key, _)| is_allowed_env_key(key) || is_host_env_key(host, key))
        .collect();
    let process_cwd = std::env::current_dir()
        .ok()
        .and_then(|p| p.to_str().map(str::to_owned));
    let mut request = build_request(host, event, payload, filtered_env, process_cwd);
    if let Some(host_event) = parsed.host_event {
        request.host_event = Some(host_event);
    }

    match connect_and_exchange(socket_path, &request) {
        ExchangeOutcome::Reply(reply) => {
            let _ = out.write_all(reply.stdout.as_bytes());
        }
        ExchangeOutcome::VersionMismatch => {
            if let Some(log) = log_path {
                append_error_log(log, "version mismatch in daemon reply");
            }
            let _ = out.write_all(host.noop_reply().as_bytes());
        }
        ExchangeOutcome::DaemonDown => {
            if let Some(log) = log_path {
                append_error_log(log, "daemon not running; spawning");
            }
            let _ = spawn_daemon_detached(socket_path);
            let _ = out.write_all(host.noop_reply().as_bytes());
        }
        ExchangeOutcome::DaemonBusy => {
            if let Some(log) = log_path {
                append_error_log(log, "daemon busy (listen queue full)");
            }
            let _ = out.write_all(host.noop_reply().as_bytes());
        }
        ExchangeOutcome::Error(msg) => {
            if let Some(log) = log_path {
                append_error_log(log, &msg);
            }
            let _ = out.write_all(host.noop_reply().as_bytes());
        }
    }
}

#[cfg(test)]
mod arg_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used)]

    use super::{parse_args, parse_host_and_event};
    use tkm_proto::{EventKind, Host};

    #[test]
    fn parses_host_and_event_in_either_order_ignoring_unknown_flags() {
        let args = ["--event", "prompt", "--ignored", "x", "--host", "cursor"];
        let parsed = parse_args(args).unwrap();
        assert_eq!(parsed.host, Host::Cursor);
        assert_eq!(parsed.event, EventKind::Prompt);
    }

    #[test]
    fn ignores_flag_missing_its_value() {
        let (host, event) = parse_host_and_event(["--host", "claude", "--event"]);
        assert_eq!(host, Some(Host::Claude));
        assert_eq!(event, None);
    }

    #[test]
    fn parses_payload_and_host_event_options_in_any_order() {
        let parsed = parse_args([
            "--payload",
            "{\"x\":1}",
            "--event",
            "prompt",
            "--host-event",
            "PiBeforeTool",
            "--host",
            "claude",
        ])
        .unwrap();
        assert_eq!(parsed.host_event.as_deref(), Some("PiBeforeTool"));
        assert_eq!(parsed.payload.as_deref(), Some("{\"x\":1}"));
    }

    #[test]
    fn missing_optional_values_are_ignored() {
        let parsed = parse_args([
            "--host",
            "claude",
            "--event",
            "prompt",
            "--host-event",
            "--payload",
        ])
        .unwrap();
        assert_eq!(parsed.host_event, None);
        assert_eq!(parsed.payload, None);
    }
}

#[cfg(test)]
mod host_marker_tests {
    use super::host_env_markers;
    use tkm_proto::Host;

    #[test]
    fn session_store_overrides_are_forwarded_for_their_host() {
        for (host, key) in [
            (Host::Kimi, "KIMI_SHARE_DIR"),
            (Host::Deepseek, "CODEWHALE_HOME"),
            (Host::Pi, "PI_CODING_AGENT_DIR"),
            (Host::Opencode, "OPENCODE_DB"),
            (Host::Opencode, "XDG_DATA_HOME"),
        ] {
            assert!(super::is_host_env_key(host, key), "{host:?} {key}");
        }
    }

    #[test]
    fn marker_table_matches_host_adapters() {
        for host in [
            Host::Claude,
            Host::Codex,
            Host::Agy,
            Host::Cursor,
            Host::Kimi,
            Host::Deepseek,
            Host::Opencode,
            Host::Pi,
        ] {
            assert_eq!(
                host_env_markers(host),
                tkm_hosts::env_markers(host),
                "{host:?}"
            );
        }
    }
}
