#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use serde_json::{Value, json};
use tkm_core::model::StopReason;
use tkm_hosts::cursor::{CursorAdapter, is_cursor_payload};
use tkm_hosts::{AdapterError, HostAdapter, PruneTarget};
use tkm_proto::{EventKind, Host, PROTOCOL_VERSION, ReplyAction, Request};

fn request(payload: Value) -> Request {
    Request {
        version: PROTOCOL_VERSION,
        host: Host::Cursor,
        event: EventKind::Prompt,
        host_event: None,
        payload,
        env: BTreeMap::new(),
        cwd: None,
    }
}

#[test]
fn every_cursor_hook_fixture_maps_to_expected_event() {
    let adapter = CursorAdapter::new();
    let fixtures_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/cursor/hooks");

    for (file_name, expected_kind) in [
        ("sessionStart.json", EventKind::SessionStart),
        ("beforeSubmitPrompt.json", EventKind::Prompt),
        ("postToolUse.json", EventKind::ToolDone),
        ("postToolUseFailure.json", EventKind::ToolDone),
        ("stop.json", EventKind::TurnEnd),
        ("sessionEnd.json", EventKind::SessionEnd),
    ] {
        let path = format!("{fixtures_dir}/{file_name}");
        let raw = fs::read(&path).unwrap();
        let payload: Value = serde_json::from_slice(&raw).unwrap();

        let event = adapter
            .map_event(&request(payload))
            .unwrap()
            .expect("mapped event");
        assert_eq!(event.host, Host::Cursor);
        assert_eq!(event.kind, expected_kind);
        assert_eq!(event.session_id, "c1a2b3c4-d5e6-7f8a-9b0c-1d2e3f4a5b6c");
        assert_eq!(event.turn_id.as_deref(), Some("gen-001"));
        assert_eq!(
            event.cwd.as_deref(),
            Some(std::path::Path::new("/workspace/tokenimum"))
        );

        match expected_kind {
            EventKind::Prompt => {
                assert_eq!(
                    event.prompt.as_deref(),
                    Some("Fix the compiler error in main.rs")
                );
            }
            EventKind::ToolDone => {
                let call = event.call.expect("call details present");
                assert_eq!(call.tool, "Bash");
                if file_name == "postToolUse.json" {
                    assert_eq!(call.args_digest, "cargo check");
                    assert_eq!(call.output_digest, "Finished dev profile");
                    assert_eq!(call.duration_ms, Some(1250));
                    assert!(!call.is_error);
                    assert_eq!(call.exit_code, None);
                } else {
                    assert_eq!(call.args_digest, "cargo build");
                    assert_eq!(
                        call.output_digest,
                        "error[E0425]: cannot find value `x` in this scope"
                    );
                    assert_eq!(call.duration_ms, Some(500));
                    assert!(call.is_error);
                    assert_eq!(call.exit_code, None);
                }
            }
            EventKind::TurnEnd => {
                assert_eq!(event.stopped_by, Some(StopReason::OwnStop));
            }
            _ => {}
        }
    }

    assert_eq!(adapter.host(), Host::Cursor);
    assert_eq!(
        adapter.prune_target(),
        PruneTarget::SettingsDenyList {
            path: PathBuf::from(".cursor/cli.json"),
            key: "permissions.deny".to_owned(),
        }
    );
}

#[test]
fn cli_empty_conversation_id_falls_back_to_session_id_then_generation_id() {
    let adapter = CursorAdapter::new();
    let fixtures_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/cursor/hooks");
    let path = format!("{fixtures_dir}/cli-empty-conversation-id.json");
    let raw = fs::read(&path).unwrap();
    let payload: Value = serde_json::from_slice(&raw).unwrap();

    let event = adapter
        .map_event(&request(payload))
        .unwrap()
        .expect("mapped event");
    assert_eq!(event.session_id, "sess-cli-001");

    // Fall back to generation_id when both conversation_id and session_id are empty
    let payload2 = json!({
        "conversation_id": "",
        "session_id": "",
        "generation_id": "gen-fallback-42",
        "hook_event_name": "sessionStart"
    });
    let event2 = adapter
        .map_event(&request(payload2))
        .unwrap()
        .expect("mapped event");
    assert_eq!(event2.session_id, "gen-fallback-42");
}

#[test]
fn stop_event_maps_all_status_variants() {
    let adapter = CursorAdapter::new();
    for (status, expected_reason) in [
        ("completed", StopReason::OwnStop),
        ("aborted", StopReason::Interrupted),
        ("error", StopReason::Error),
        ("unknown_status", StopReason::Unknown),
    ] {
        let payload = json!({
            "conversation_id": "conv-1",
            "generation_id": "gen-1",
            "hook_event_name": "stop",
            "status": status,
        });
        let event = adapter
            .map_event(&request(payload))
            .unwrap()
            .expect("mapped event");
        assert_eq!(event.stopped_by, Some(expected_reason));
    }
}

#[test]
fn every_event_action_pair_has_exact_stdout_bytes() {
    let adapter = CursorAdapter::new();
    let events = [
        "sessionStart",
        "beforeSubmitPrompt",
        "postToolUse",
        "postToolUseFailure",
        "stop",
        "sessionEnd",
        "unrecognizedEvent",
    ];

    for event in events {
        let req = request(json!({
            "conversation_id": "c1",
            "hook_event_name": event,
            "prompt": "test prompt",
            "tool_name": "Bash",
            "tool_input": {},
            "status": "completed",
        }));

        let can_inject = matches!(event, "sessionStart" | "postToolUse" | "postToolUseFailure");
        let injected_stdout = "{\"additional_context\":\"injected facts\"}";
        let noop_stdout = "{}";

        // ReplyAction::Nothing
        assert_eq!(
            adapter.render_reply(&req, &ReplyAction::Nothing),
            noop_stdout,
            "{event}: Nothing"
        );

        // ReplyAction::Notice (Cursor has no notice surface; returns no-op {})
        assert_eq!(
            adapter.render_reply(
                &req,
                &ReplyAction::Notice {
                    text: "some notice".to_owned()
                }
            ),
            noop_stdout,
            "{event}: Notice"
        );

        // ReplyAction::Inject
        let inject_action = ReplyAction::Inject {
            text: "injected facts".to_owned(),
        };
        assert_eq!(
            adapter.render_reply(&req, &inject_action),
            if can_inject {
                injected_stdout
            } else {
                noop_stdout
            },
            "{event}: Inject"
        );

        // ReplyAction::InjectAndNotice (uses only inject part on injecting events)
        let inject_and_notice = ReplyAction::InjectAndNotice {
            inject: "injected facts".to_owned(),
            notice: "some notice".to_owned(),
        };
        assert_eq!(
            adapter.render_reply(&req, &inject_and_notice),
            if can_inject {
                injected_stdout
            } else {
                noop_stdout
            },
            "{event}: InjectAndNotice"
        );
    }
}

#[test]
fn before_submit_prompt_never_injects() {
    let adapter = CursorAdapter::new();
    let req = request(json!({
        "conversation_id": "c1",
        "hook_event_name": "beforeSubmitPrompt",
        "prompt": "hello",
    }));

    let action = ReplyAction::Inject {
        text: "injected context".to_owned(),
    };
    assert_eq!(adapter.render_reply(&req, &action), "{}");
}

#[test]
fn reply_caps_context_at_10000_chars_and_escapes_json() {
    let adapter = CursorAdapter::new();
    let req = request(json!({
        "conversation_id": "c1",
        "hook_event_name": "sessionStart",
    }));

    let raw_text = format!("\"\n\t{}", "界🦀".repeat(6000));
    let capped: String = raw_text.chars().take(10_000).collect();
    let expected_escaped = serde_json::to_string(&capped).unwrap();
    let expected_stdout = format!("{{\"additional_context\":{expected_escaped}}}");

    let action = ReplyAction::Inject { text: raw_text };
    assert_eq!(adapter.render_reply(&req, &action), expected_stdout);
}

#[test]
fn is_cursor_payload_recognizes_cursor_version() {
    assert!(is_cursor_payload(&json!({ "cursor_version": "0.45.0" })));
    assert!(is_cursor_payload(&json!({ "cursor_version": "" })));
    assert!(!is_cursor_payload(&json!({ "conversation_id": "abc" })));
    assert!(!is_cursor_payload(&json!({ "cursor_version": 42 })));
    assert!(!is_cursor_payload(&json!({ "cursor_version": null })));
    assert!(!is_cursor_payload(&json!([])));
    assert!(!is_cursor_payload(&Value::Null));
}

#[test]
fn malformed_payloads_fail_without_panicking_and_render_noop() {
    let adapter = CursorAdapter::new();

    // Wrong protocol version
    let mut req_bad_version =
        request(json!({ "conversation_id": "c1", "hook_event_name": "sessionStart" }));
    req_bad_version.version = 999;
    assert!(matches!(
        adapter.map_event(&req_bad_version),
        Err(AdapterError::InvalidPayload { .. })
    ));
    assert_eq!(
        adapter.render_reply(&req_bad_version, &ReplyAction::Inject { text: "t".into() }),
        "{}"
    );

    // Wrong host
    let mut req_bad_host =
        request(json!({ "conversation_id": "c1", "hook_event_name": "sessionStart" }));
    req_bad_host.host = Host::Claude;
    assert!(matches!(
        adapter.map_event(&req_bad_host),
        Err(AdapterError::InvalidPayload { .. })
    ));
    assert_eq!(
        adapter.render_reply(&req_bad_host, &ReplyAction::Inject { text: "t".into() }),
        "{}"
    );

    // Non-object payload
    let req_array = request(json!(["not", "an", "object"]));
    assert!(matches!(
        adapter.map_event(&req_array),
        Err(AdapterError::InvalidPayload { .. })
    ));
    assert_eq!(
        adapter.render_reply(&req_array, &ReplyAction::Inject { text: "t".into() }),
        "{}"
    );

    // Missing all session identifiers
    let req_no_id = request(json!({ "hook_event_name": "sessionStart" }));
    assert!(matches!(
        adapter.map_event(&req_no_id),
        Err(AdapterError::InvalidPayload { .. })
    ));

    // Native event conflict between payload and request.host_event
    let mut req_conflict = request(json!({
        "conversation_id": "c1",
        "hook_event_name": "sessionStart"
    }));
    req_conflict.host_event = Some("stop".to_owned());
    assert!(matches!(
        adapter.map_event(&req_conflict),
        Err(AdapterError::InvalidPayload { .. })
    ));

    // Missing native event
    let req_no_event = request(json!({ "conversation_id": "c1" }));
    assert!(matches!(
        adapter.map_event(&req_no_event),
        Err(AdapterError::InvalidPayload { .. })
    ));

    // beforeSubmitPrompt with non-string prompt
    let req_bad_prompt = request(json!({
        "conversation_id": "c1",
        "hook_event_name": "beforeSubmitPrompt",
        "prompt": 12_345
    }));
    assert!(matches!(
        adapter.map_event(&req_bad_prompt),
        Err(AdapterError::InvalidPayload { .. })
    ));

    // postToolUse with non-string tool_name
    let req_bad_tool = request(json!({
        "conversation_id": "c1",
        "hook_event_name": "postToolUse",
        "tool_name": 123
    }));
    assert!(matches!(
        adapter.map_event(&req_bad_tool),
        Err(AdapterError::InvalidPayload { .. })
    ));
}
