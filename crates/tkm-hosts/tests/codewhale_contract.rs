#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde_json::{Value, json};
use tkm_core::model::{HookEvent, StopReason, Usage};
use tkm_hosts::codewhale::{CodewhaleAdapter, ENV_MARKERS, EXPECTED_EVENTS};
use tkm_hosts::{AdapterError, HostAdapter, PruneTarget};
use tkm_proto::{EventKind, Host, PROTOCOL_VERSION, ReplyAction, Request};

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/codewhale/hooks"
);

fn make_request(
    event: EventKind,
    host_event: Option<&str>,
    payload: Value,
    env: &[(&str, &str)],
) -> Request {
    let mut env_map = BTreeMap::new();
    for &(k, v) in env {
        env_map.insert(k.to_owned(), v.to_owned());
    }
    Request {
        version: PROTOCOL_VERSION,
        host: Host::Deepseek,
        event,
        host_event: host_event.map(str::to_owned),
        payload,
        env: env_map,
        cwd: None,
    }
}

#[test]
fn constants_and_metadata_match_specification() {
    let adapter = CodewhaleAdapter::new();
    assert_eq!(adapter.host(), Host::Deepseek);
    assert_eq!(adapter.prune_target(), PruneTarget::ReportOnly);

    let expected_markers = [
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
    ];
    assert_eq!(ENV_MARKERS.len(), expected_markers.len());
    for marker in expected_markers {
        assert!(ENV_MARKERS.contains(&marker), "missing env marker {marker}");
    }

    assert_eq!(EXPECTED_EVENTS.len(), 6);
    for entry in EXPECTED_EVENTS {
        assert!(entry.required);
        assert!(
            entry
                .note
                .contains("`codewhale exec` fires only tool_call_before (with --hooks)")
        );
    }
    let start_entry = EXPECTED_EVENTS
        .iter()
        .find(|e| e.native == "session_start")
        .unwrap();
    assert_eq!(start_entry.kind, EventKind::SessionStart);
    assert!(start_entry.note.contains("env only, cannot inject"));

    let before_entry = EXPECTED_EVENTS
        .iter()
        .find(|e| e.native == "tool_call_before")
        .unwrap();
    assert_eq!(before_entry.kind, EventKind::ToolBefore);
    assert!(before_entry.note.contains("only injection surface"));
}

#[test]
fn mapping_session_start_null_payload_uses_env() {
    let adapter = CodewhaleAdapter::new();
    let req = make_request(
        EventKind::SessionStart,
        Some("session_start"),
        Value::Null,
        &[
            ("DEEPSEEK_SESSION_ID", "sess_001"),
            ("DEEPSEEK_WORKSPACE", "/home/user/workspace"),
        ],
    );

    let event = adapter.map_event(&req).unwrap().unwrap();
    let mut expected = HookEvent::new(Host::Deepseek, EventKind::SessionStart, "sess_001");
    expected.cwd = Some(PathBuf::from("/home/user/workspace"));
    assert_eq!(event, expected);

    // Fallback to CODEWHALE_SESSION_ID
    let req_cw = make_request(
        EventKind::SessionStart,
        Some("session_start"),
        Value::Null,
        &[
            ("CODEWHALE_SESSION_ID", "sess_cw_999"),
            ("DEEPSEEK_WORKSPACE", "/home/user/workspace"),
        ],
    );
    let event_cw = adapter.map_event(&req_cw).unwrap().unwrap();
    assert_eq!(event_cw.session_id, "sess_cw_999");
}

#[test]
fn mapping_message_submit_fixture() {
    let adapter = CodewhaleAdapter::new();
    let fixture_path = format!("{FIXTURES}/message_submit.json");
    let payload: Value = serde_json::from_slice(&fs::read(&fixture_path).unwrap()).unwrap();

    let req = make_request(
        EventKind::Prompt,
        Some("message_submit"),
        payload,
        &[("DEEPSEEK_SESSION_ID", "sess_override_if_any")],
    );

    let event = adapter.map_event(&req).unwrap().unwrap();
    assert_eq!(event.host, Host::Deepseek);
    assert_eq!(event.kind, EventKind::Prompt);
    assert_eq!(event.session_id, "sess_override_if_any");
    assert_eq!(event.cwd, Some(PathBuf::from("/home/user/project")));
    assert_eq!(
        event.prompt.as_deref(),
        Some("Please fix the failing unit test")
    );
}

#[test]
fn mapping_tool_call_before_null_payload() {
    let adapter = CodewhaleAdapter::new();
    let req = make_request(
        EventKind::ToolBefore,
        Some("tool_call_before"),
        Value::Null,
        &[
            ("DEEPSEEK_SESSION_ID", "sess_002"),
            ("DEEPSEEK_WORKSPACE", "/workspace"),
            ("DEEPSEEK_TOOL_NAME", "exec_shell"),
            ("DEEPSEEK_TOOL_ARGS", "{\"command\":\"cargo check\"}"),
        ],
    );

    let event = adapter.map_event(&req).unwrap().unwrap();
    assert_eq!(event.kind, EventKind::ToolBefore);
    let call = event.call.unwrap();
    assert_eq!(call.tool, "exec_shell");
    assert_eq!(call.args_digest, "cargo check");
}

#[test]
fn mapping_tool_call_before_raw_string_args() {
    let adapter = CodewhaleAdapter::new();
    let req = make_request(
        EventKind::ToolBefore,
        Some("tool_call_before"),
        Value::Null,
        &[
            ("DEEPSEEK_SESSION_ID", "sess_003"),
            ("DEEPSEEK_TOOL_NAME", "bash"),
            ("DEEPSEEK_TOOL_ARGS", "npm test -- --watch"),
        ],
    );

    let event = adapter.map_event(&req).unwrap().unwrap();
    let call = event.call.unwrap();
    assert_eq!(call.tool, "bash");
    assert_eq!(call.args_digest, "npm test -- --watch");
}

#[test]
fn mapping_tool_call_after_null_payload_success_and_failure() {
    let adapter = CodewhaleAdapter::new();
    let req_ok = make_request(
        EventKind::ToolDone,
        Some("tool_call_after"),
        Value::Null,
        &[
            ("DEEPSEEK_SESSION_ID", "sess_004"),
            ("DEEPSEEK_TOOL_NAME", "git_status"),
            ("DEEPSEEK_TOOL_RESULT", "clean working tree"),
            ("DEEPSEEK_TOOL_SUCCESS", "true"),
            ("DEEPSEEK_TOOL_EXIT_CODE", "0"),
        ],
    );

    let event_ok = adapter.map_event(&req_ok).unwrap().unwrap();
    assert_eq!(event_ok.kind, EventKind::ToolDone);
    let call_ok = event_ok.call.unwrap();
    assert_eq!(call_ok.tool, "git_status");
    assert_eq!(call_ok.output_digest, "clean working tree");
    assert!(!call_ok.is_error);
    assert_eq!(call_ok.exit_code, Some(0));

    let req_err = make_request(
        EventKind::ToolDone,
        Some("tool_call_after"),
        Value::Null,
        &[
            ("DEEPSEEK_SESSION_ID", "sess_004"),
            ("DEEPSEEK_TOOL_NAME", "git_status"),
            ("DEEPSEEK_TOOL_RESULT", "error: permission denied"),
            ("DEEPSEEK_TOOL_SUCCESS", "false"),
            ("DEEPSEEK_TOOL_EXIT_CODE", "1"),
        ],
    );

    let event_err = adapter.map_event(&req_err).unwrap().unwrap();
    let call_err = event_err.call.unwrap();
    assert!(call_err.is_error);
    assert_eq!(call_err.exit_code, Some(1));
    assert_eq!(call_err.output_digest, "error: permission denied");
}

#[test]
fn mapping_tool_call_after_shell_fixture_with_execution_receipt() {
    let adapter = CodewhaleAdapter::new();
    let fixture_path = format!("{FIXTURES}/tool_call_after-shell.json");
    let payload: Value = serde_json::from_slice(&fs::read(&fixture_path).unwrap()).unwrap();

    let req = make_request(
        EventKind::ToolDone,
        Some("tool_call_after"),
        payload,
        &[("DEEPSEEK_SESSION_ID", "sess_8f3a1b2c")],
    );

    let event = adapter.map_event(&req).unwrap().unwrap();
    assert_eq!(event.kind, EventKind::ToolDone);
    let call = event.call.unwrap();
    assert_eq!(call.tool, "exec_shell");
    assert_eq!(call.args_digest, "cargo test");
    assert_eq!(call.exit_code, Some(0));
    assert!(!call.is_error);
    assert_eq!(call.output_digest, "test result: ok. 1 passed; 0 failed");
}

#[test]
fn mapping_turn_end_fixture() {
    let adapter = CodewhaleAdapter::new();
    let fixture_path = format!("{FIXTURES}/turn_end.json");
    let payload: Value = serde_json::from_slice(&fs::read(&fixture_path).unwrap()).unwrap();

    let req = make_request(
        EventKind::TurnEnd,
        Some("turn_end"),
        payload,
        &[("DEEPSEEK_SESSION_ID", "sess_8f3a1b2c")],
    );

    let event = adapter.map_event(&req).unwrap().unwrap();
    assert_eq!(event.kind, EventKind::TurnEnd);
    assert_eq!(event.turn_id.as_deref(), Some("turn_001"));
    assert_eq!(event.stopped_by, Some(StopReason::OwnStop));
    assert_eq!(
        event.usage,
        Usage {
            input: 120,
            output: 45,
            cache_read: 80,
            cache_write: 10,
        }
    );
}

#[test]
fn mapping_turn_end_statuses() {
    let adapter = CodewhaleAdapter::new();
    for (status, expected_stop) in [
        ("completed", StopReason::OwnStop),
        ("success", StopReason::OwnStop),
        ("failed", StopReason::Error),
        ("error", StopReason::Error),
        ("interrupted", StopReason::Interrupted),
        ("cancelled", StopReason::Interrupted),
        ("other_unknown", StopReason::Unknown),
    ] {
        let req = make_request(
            EventKind::TurnEnd,
            Some("turn_end"),
            json!({
                "session_id": "sess_stat",
                "status": status,
            }),
            &[],
        );
        let event = adapter.map_event(&req).unwrap().unwrap();
        assert_eq!(
            event.stopped_by,
            Some(expected_stop),
            "status '{status}' mismatch"
        );
    }
}

#[test]
fn mapping_session_end_null_payload() {
    let adapter = CodewhaleAdapter::new();
    let req = make_request(
        EventKind::SessionEnd,
        Some("session_end"),
        Value::Null,
        &[("DEEPSEEK_SESSION_ID", "sess_end_01")],
    );

    let event = adapter.map_event(&req).unwrap().unwrap();
    assert_eq!(event.kind, EventKind::SessionEnd);
    assert_eq!(event.session_id, "sess_end_01");
}

#[test]
fn mapping_unhandled_native_events_return_ok_none() {
    let adapter = CodewhaleAdapter::new();
    let req = make_request(
        EventKind::SessionStart,
        Some("session_idle"),
        Value::Null,
        &[("DEEPSEEK_SESSION_ID", "sess_idle")],
    );
    assert_eq!(adapter.map_event(&req).unwrap(), None);
}

#[test]
fn replies_exact_stdout_and_capping() {
    let adapter = CodewhaleAdapter::new();
    let req_before = make_request(
        EventKind::ToolBefore,
        Some("tool_call_before"),
        Value::Null,
        &[("DEEPSEEK_SESSION_ID", "sess_reply")],
    );

    // tool_call_before with Inject action
    let inject_action = ReplyAction::Inject {
        text: "Relevant project rule: always run cargo check".to_owned(),
    };
    let stdout = adapter.render_reply(&req_before, &inject_action);
    assert_eq!(
        stdout,
        r#"{"decision":"allow","additionalContext":"Relevant project rule: always run cargo check"}"#
    );

    // tool_call_before with InjectAndNotice action (notice dropped, inject kept)
    let inject_notice = ReplyAction::InjectAndNotice {
        inject: "important context".to_owned(),
        notice: "user notice".to_owned(),
    };
    assert_eq!(
        adapter.render_reply(&req_before, &inject_notice),
        r#"{"decision":"allow","additionalContext":"important context"}"#
    );

    // Cap at 2,000 characters
    let long_text = "x".repeat(3_000);
    let capped_reply = adapter.render_reply(
        &req_before,
        &ReplyAction::Inject {
            text: long_text.clone(),
        },
    );
    let expected_inner = "x".repeat(2_000);
    assert_eq!(
        capped_reply,
        format!(r#"{{"decision":"allow","additionalContext":"{expected_inner}"}}"#)
    );

    // tool_call_before with Nothing or Notice action -> ""
    assert_eq!(adapter.render_reply(&req_before, &ReplyAction::Nothing), "");
    assert_eq!(
        adapter.render_reply(
            &req_before,
            &ReplyAction::Notice {
                text: "notice only".into()
            }
        ),
        ""
    );

    // Other events never inject -> ""
    for ev in [
        EventKind::SessionStart,
        EventKind::Prompt,
        EventKind::ToolDone,
        EventKind::TurnEnd,
        EventKind::SessionEnd,
    ] {
        let req_other = make_request(
            ev,
            Some(ev.as_str()),
            Value::Null,
            &[("DEEPSEEK_SESSION_ID", "sess_other")],
        );
        assert_eq!(
            adapter.render_reply(
                &req_other,
                &ReplyAction::Inject {
                    text: "some context".into()
                }
            ),
            "",
            "event {ev:?} should not inject"
        );
    }
}

#[test]
fn fail_open_and_malformed_requests() {
    let adapter = CodewhaleAdapter::new();

    // Version mismatch
    let mut bad_version = make_request(
        EventKind::SessionStart,
        Some("session_start"),
        Value::Null,
        &[("DEEPSEEK_SESSION_ID", "sess_1")],
    );
    bad_version.version = 99;
    assert!(matches!(
        adapter.map_event(&bad_version),
        Err(AdapterError::InvalidPayload { .. })
    ));
    assert_eq!(
        adapter.render_reply(&bad_version, &ReplyAction::Inject { text: "ctx".into() }),
        ""
    );

    // Host mismatch
    let mut bad_host = make_request(
        EventKind::SessionStart,
        Some("session_start"),
        Value::Null,
        &[("DEEPSEEK_SESSION_ID", "sess_1")],
    );
    bad_host.host = Host::Claude;
    assert!(matches!(
        adapter.map_event(&bad_host),
        Err(AdapterError::InvalidPayload { .. })
    ));

    // Missing session ID
    let missing_sess = make_request(
        EventKind::SessionStart,
        Some("session_start"),
        Value::Null,
        &[],
    );
    assert!(matches!(
        adapter.map_event(&missing_sess),
        Err(AdapterError::InvalidPayload { .. })
    ));

    // Conflicting event names
    let conflict = make_request(
        EventKind::SessionStart,
        Some("session_start"),
        json!({"event": "message_submit", "session_id": "sess_1"}),
        &[],
    );
    assert!(matches!(
        adapter.map_event(&conflict),
        Err(AdapterError::InvalidPayload { .. })
    ));

    // Non-object non-null payload
    let bad_payload = make_request(
        EventKind::SessionStart,
        Some("session_start"),
        json!(42),
        &[("DEEPSEEK_SESSION_ID", "sess_1")],
    );
    assert!(matches!(
        adapter.map_event(&bad_payload),
        Err(AdapterError::InvalidPayload { .. })
    ));
}
