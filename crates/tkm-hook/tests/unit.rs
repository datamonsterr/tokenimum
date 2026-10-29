#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Unit tests for the `tkm_hook` library.

use std::collections::BTreeMap;
use tkm_hook::{
    ConnectResult, ENV_ALLOWLIST, build_request, connect_bounded, is_allowed_env_key, is_disabled,
    parse_args, parse_host_and_event, read_payload, run_with_panic_guard,
};
use tkm_proto::{EventKind, Host, PROTOCOL_VERSION};

#[test]
fn test_catch_unwind_panicking_closure_noop() {
    let mut stdout = Vec::new();
    run_with_panic_guard(
        Host::Claude,
        || panic!("injected panic for testing"),
        &mut stdout,
    );
    assert_eq!(
        std::str::from_utf8(&stdout).unwrap(),
        Host::Claude.noop_reply()
    );
}

#[test]
fn test_catch_unwind_normal_closure_passthrough() {
    let mut stdout = Vec::new();
    run_with_panic_guard(
        Host::Cursor,
        || {
            // normal execution
        },
        &mut stdout,
    );
    assert_eq!(stdout, b"");
}

#[test]
fn test_is_allowed_env_key() {
    for &key in ENV_ALLOWLIST {
        assert!(is_allowed_env_key(key), "key {key} should be allowed");
    }
    assert!(is_allowed_env_key("TKM_DEBUG"));
    assert!(is_allowed_env_key("TKM_VERBOSE"));
    assert!(is_allowed_env_key("SUPERORCHESTRATOR_RUN_ID"));
    assert!(is_allowed_env_key("SUPERORCHESTRATOR_TASK"));

    // Secret-like markers must be suppressed.
    assert!(!is_allowed_env_key("TKM_API_KEY"));
    assert!(!is_allowed_env_key("TKM_TOKEN"));
    assert!(!is_allowed_env_key("TKM_SECRET"));
    assert!(!is_allowed_env_key("TKM_OAUTH_TOKEN"));
    assert!(!is_allowed_env_key("TKM_PRIVATE_KEY"));

    // Non-whitelisted keys must not be forwarded.
    assert!(!is_allowed_env_key("PATH"));
    assert!(!is_allowed_env_key("HOME"));
    assert!(!is_allowed_env_key("USER"));
    assert!(!is_allowed_env_key("SHELL"));
}

#[test]
fn test_is_disabled() {
    assert!(is_disabled("1"));
    assert!(is_disabled("true"));
    assert!(is_disabled("TRUE"));
    assert!(is_disabled("True"));
    assert!(is_disabled("yes"));
    assert!(is_disabled("YES"));
    assert!(is_disabled("Yes"));

    assert!(!is_disabled("0"));
    assert!(!is_disabled("false"));
    assert!(!is_disabled("no"));
    assert!(!is_disabled(""));
    assert!(!is_disabled("anything_else"));
}

#[test]
fn test_parse_args_and_host_event() {
    let args = vec!["--host", "claude", "--event", "tool_done"];
    let parsed = parse_args(&args).unwrap();
    assert_eq!(parsed.host, Host::Claude);
    assert_eq!(parsed.event, EventKind::ToolDone);

    let (h, e) = parse_host_and_event(&args);
    assert_eq!(h, Some(Host::Claude));
    assert_eq!(e, Some(EventKind::ToolDone));

    // Missing event
    let args_no_event = vec!["--host", "cursor"];
    assert!(parse_args(&args_no_event).is_none());
    let (h, e) = parse_host_and_event(&args_no_event);
    assert_eq!(h, Some(Host::Cursor));
    assert_eq!(e, None);

    // Unknown host
    let args_bad_host = vec!["--host", "invalid", "--event", "prompt"];
    assert!(parse_args(&args_bad_host).is_none());
    let (h, e) = parse_host_and_event(&args_bad_host);
    assert_eq!(h, None);
    assert_eq!(e, Some(EventKind::Prompt));
}

#[test]
fn test_build_request_cwd_payload_precedence() {
    let payload = serde_json::json!({
        "cwd": "/explicit/payload/dir",
        "hook_event_name": "PostToolUse"
    });
    let req = build_request(
        Host::Claude,
        EventKind::ToolDone,
        payload,
        BTreeMap::new(),
        Some("/process/dir".to_string()),
    );
    assert_eq!(req.version, PROTOCOL_VERSION);
    assert_eq!(req.cwd.as_deref(), Some("/explicit/payload/dir"));
    assert_eq!(req.host_event.as_deref(), Some("PostToolUse"));
}

#[test]
fn test_build_request_cwd_fallback_to_process() {
    let payload = serde_json::json!({
        "hook_event_name": "SessionStart"
    });
    let req = build_request(
        Host::Claude,
        EventKind::SessionStart,
        payload,
        BTreeMap::new(),
        Some("/process/dir".to_string()),
    );
    assert_eq!(req.cwd.as_deref(), Some("/process/dir"));
}

#[test]
fn test_read_payload_json_and_fallback() {
    let valid = br#"{"foo": "bar"}"#;
    let res = read_payload(&valid[..]);
    assert_eq!(res, serde_json::json!({"foo": "bar"}));

    let empty = b"";
    assert_eq!(read_payload(&empty[..]), serde_json::Value::Null);

    let invalid = b"invalid json data";
    assert_eq!(read_payload(&invalid[..]), serde_json::Value::Null);
}

#[test]
fn test_bounded_connect_timeout_returns_daemon_busy() {
    let start = std::time::Instant::now();
    let outcome = connect_bounded(std::time::Duration::from_millis(10), || {
        std::thread::sleep(std::time::Duration::from_secs(1));
        Err(std::io::Error::new(
            std::io::ErrorKind::ConnectionRefused,
            "late error",
        ))
    });
    let elapsed = start.elapsed();

    assert!(
        matches!(outcome, ConnectResult::DaemonBusy),
        "expected DaemonBusy on connect timeout"
    );
    assert!(
        elapsed < std::time::Duration::from_millis(50),
        "bounded connect took too long: {elapsed:?}"
    );
}

#[test]
fn test_bounded_connect_immediate_error_returns_down() {
    let outcome = connect_bounded(std::time::Duration::from_millis(50), || {
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "socket not found",
        ))
    });
    assert!(matches!(outcome, ConnectResult::DaemonDown));
}
