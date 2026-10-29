#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! `tkm-hook` client library (E4.1) - TDD stub.
//!
//! Minimal compilation stubs for the TDD red phase.

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::path::Path;
use tkm_proto::{EventKind, Host, Reply, Request};

/// Exact variable names that are always forwarded.
pub const ENV_ALLOWLIST: &[&str] = &[
    "CLAUDE_PROJECT_DIR",
    "CLAUDE_CONFIG_DIR",
    "CODEX_HOME",
    "TERM_PROGRAM",
];

/// Returns `true` when an env key should be forwarded.
#[must_use]
pub fn is_allowed_env_key(_key: &str) -> bool {
    false
}

/// Returns `true` when `TKM_DISABLE` is set to a truthy value.
#[must_use]
pub fn is_disabled(_val: &str) -> bool {
    false
}

/// Parsed hook command-line arguments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HookArgs {
    /// Host identifier.
    pub host: Host,
    /// Event kind.
    pub event: EventKind,
}

/// Parse `--host <host> --event <event>` from raw args.
#[must_use]
pub fn parse_args<I, S>(_args: I) -> Option<HookArgs>
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    None
}

/// Parse host and event independently from raw args.
#[must_use]
pub fn parse_host_and_event<I, S>(_args: I) -> (Option<Host>, Option<EventKind>)
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    (None, None)
}

/// Read all bytes from `reader`.
#[must_use]
pub fn read_payload<R: Read>(_reader: R) -> serde_json::Value {
    serde_json::Value::Null
}

/// Build the `Request` from parsed args, payload, and filtered env.
#[must_use]
pub fn build_request(
    host: Host,
    event: EventKind,
    payload: serde_json::Value,
    env: BTreeMap<String, String>,
    fallback_cwd: Option<String>,
) -> Request {
    Request {
        version: 0,
        host,
        event,
        host_event: None,
        payload,
        env,
        cwd: fallback_cwd,
    }
}

/// Outcome of one hook exchange with the daemon.
pub enum ExchangeOutcome {
    /// Valid reply.
    Reply(Reply),
    /// Protocol version mismatch.
    VersionMismatch,
    /// Daemon not running.
    DaemonDown,
    /// Daemon busy.
    DaemonBusy,
    /// Exchange error.
    Error(String),
}

/// Connect to the daemon socket and perform one synchronous exchange.
#[must_use]
pub fn connect_and_exchange(_socket_path: &Path, _request: &Request) -> ExchangeOutcome {
    ExchangeOutcome::DaemonDown
}

/// Spawn `tkmd` detached, not waiting for it.
#[must_use]
pub fn spawn_daemon_detached(_socket_path: &Path) -> bool {
    false
}

/// Append one line to the error log.
pub fn append_error_log(_log_path: &Path, _message: &str) {}

/// Run `f`, catching any panic.
pub fn run_with_panic_guard<F, W>(_host: Host, f: F, _out: &mut W)
where
    F: FnOnce(),
    W: Write,
{
    f();
}

/// Collect filtered env vars from an iterator.
#[must_use]
pub fn collect_env<I>(_iter: I) -> BTreeMap<String, String>
where
    I: IntoIterator<Item = (String, String)>,
{
    BTreeMap::new()
}

/// Core hook logic.
pub fn run_hook<I, S, E>(
    _args: I,
    _env_iter: E,
    _stdin_bytes: &[u8],
    _socket_path: Option<&Path>,
    _log_path: Option<&Path>,
    _out: &mut dyn Write,
) where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
    E: IntoIterator<Item = (String, String)>,
{
}
