#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::PathBuf};
use tkm_core::model::{Call, HookEvent, StopReason};
use tkm_hosts::{
    AdapterError, HostAdapter, PruneTarget,
    kimi::{ENV_MARKERS, EXPECTED_EVENTS, KimiAdapter},
};
use tkm_proto::{EventKind, Host, PROTOCOL_VERSION, ReplyAction, Request};

fn request(payload: Value) -> Request {
    Request {
        version: PROTOCOL_VERSION,
        host: Host::Kimi,
        event: EventKind::Prompt,
        host_event: None,
        payload,
        env: BTreeMap::new(),
        cwd: None,
    }
}

#[test]
fn kimi_host_adapter_identity() {
    let adapter = KimiAdapter::new();
    assert_eq!(adapter.host(), Host::Kimi);
    assert_eq!(adapter.prune_target(), PruneTarget::ReportOnly);
    assert_eq!(ENV_MARKERS, ["KIMI_SHARE_DIR"]);

    assert_eq!(EXPECTED_EVENTS.len(), 6);
    let events = EXPECTED_EVENTS;
    assert_eq!(events[0].native, "SessionStart");
    assert_eq!(events[0].kind, EventKind::SessionStart);
    assert!(events[0].required);
    assert_eq!(
        events[0].note,
        "stdout injection ignored by kimi-cli 1.52.0"
    );

    assert_eq!(events[1].native, "UserPromptSubmit");
    assert_eq!(events[1].kind, EventKind::Prompt);
    assert!(events[1].required);
    assert_eq!(
        events[1].note,
        "stdout injection ignored by kimi-cli 1.52.0"
    );

    assert_eq!(events[2].native, "PostToolUse");
    assert_eq!(events[2].kind, EventKind::ToolDone);
    assert!(events[2].required);

    assert_eq!(events[3].native, "PostToolUseFailure");
    assert_eq!(events[3].kind, EventKind::ToolDone);
    assert!(!events[3].required);
    assert_eq!(events[3].note, "only on tool exceptions");

    assert_eq!(events[4].native, "Stop");
    assert_eq!(events[4].kind, EventKind::TurnEnd);
    assert!(events[4].required);

    assert_eq!(events[5].native, "SessionEnd");
    assert_eq!(events[5].kind, EventKind::SessionEnd);
    assert!(events[5].required);
}

#[test]
fn every_kimi_hook_fixture_maps_to_expected_event() {
    let adapter = KimiAdapter::new();
    let fixtures_dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/kimi/hooks");

    for entry in fs::read_dir(fixtures_dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().is_some_and(|ext| ext == "json") {
            let p: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
            let native = p["hook_event_name"].as_str().unwrap();
            let kind = match native {
                "SessionStart" => EventKind::SessionStart,
                "UserPromptSubmit" => EventKind::Prompt,
                "PostToolUse" | "PostToolUseFailure" => EventKind::ToolDone,
                "Stop" => EventKind::TurnEnd,
                "SessionEnd" => EventKind::SessionEnd,
                other => panic!("unexpected fixture event: {other}"),
            };

            let mut expected =
                HookEvent::new(Host::Kimi, kind, "9a1b2c3d-4e5f-6a7b-8c9d-0e1f2a3b4c5d");
            expected.cwd = Some(PathBuf::from("/home/user/project"));

            if kind == EventKind::TurnEnd {
                // Kimi `Stop` fires when the agent ends its own turn.
                expected.stopped_by = Some(StopReason::OwnStop);
            }

            if kind == EventKind::Prompt {
                expected.prompt = Some(p["prompt"].as_str().unwrap().to_owned());
            }

            if kind == EventKind::ToolDone {
                let tool = p["tool_name"].as_str().unwrap();
                let is_failure = native == "PostToolUseFailure";
                let output = if is_failure {
                    p["error"].as_str().unwrap().to_owned()
                } else {
                    p["tool_output"].as_str().unwrap().to_owned()
                };

                expected.call = Some(Call {
                    tool: tool.to_owned(),
                    args_digest: p["tool_input"]["command"].as_str().unwrap().to_owned(),
                    output_digest: output,
                    exit_code: None,
                    is_error: is_failure,
                    duration_ms: None,
                    edits: Vec::new(),
                });
            }

            let mapped = adapter.map_event(&request(p)).unwrap();
            assert_eq!(
                mapped,
                Some(expected),
                "failed mapping for {}",
                path.display()
            );
        }
    }
}

#[test]
fn render_reply_plain_text_for_session_start_and_user_prompt_submit() {
    let adapter = KimiAdapter::new();

    // SessionStart with Inject
    let req_start = request(json!({
        "hook_event_name": "SessionStart",
        "session_id": "9a1b2c3d-4e5f-6a7b-8c9d-0e1f2a3b4c5d",
    }));
    let reply = adapter.render_reply(
        &req_start,
        &ReplyAction::Inject {
            text: "Initial instructions".into(),
        },
    );
    assert_eq!(reply, "Initial instructions");

    // UserPromptSubmit with InjectAndNotice
    let req_prompt = request(json!({
        "hook_event_name": "UserPromptSubmit",
        "session_id": "9a1b2c3d-4e5f-6a7b-8c9d-0e1f2a3b4c5d",
        "prompt": "hi",
    }));
    let reply_prompt = adapter.render_reply(
        &req_prompt,
        &ReplyAction::InjectAndNotice {
            inject: "Prompt injected context".into(),
            notice: "Notice text ignored".into(),
        },
    );
    assert_eq!(reply_prompt, "Prompt injected context");

    // Long text is capped at 10,000 characters
    let long_text = "a".repeat(15_000);
    let reply_long = adapter.render_reply(&req_start, &ReplyAction::Inject { text: long_text });
    assert_eq!(reply_long.len(), 10_000);
    assert_eq!(reply_long, "a".repeat(10_000));

    // Notice only renders empty string
    let reply_notice = adapter.render_reply(
        &req_start,
        &ReplyAction::Notice {
            text: "Notice only".into(),
        },
    );
    assert_eq!(reply_notice, "");

    // Nothing action renders empty string
    let reply_nothing = adapter.render_reply(&req_start, &ReplyAction::Nothing);
    assert_eq!(reply_nothing, "");
}

#[test]
fn render_reply_empty_for_tool_and_end_events() {
    let adapter = KimiAdapter::new();
    let action = ReplyAction::Inject {
        text: "Some context".into(),
    };

    for native in ["PostToolUse", "PostToolUseFailure", "Stop", "SessionEnd"] {
        let req = request(json!({
            "hook_event_name": native,
            "session_id": "9a1b2c3d-4e5f-6a7b-8c9d-0e1f2a3b4c5d",
            "tool_name": "bash",
            "tool_input": {},
            "tool_output": "out",
            "error": "err",
        }));
        let reply = adapter.render_reply(&req, &action);
        assert_eq!(reply, "", "expected empty reply for {native}");
    }
}

#[test]
fn fault_injection_malformed_requests_fail_open() {
    let adapter = KimiAdapter::new();

    // Unsupported version
    let mut bad_req = request(json!({
        "hook_event_name": "SessionStart",
        "session_id": "9a1b2c3d-4e5f-6a7b-8c9d-0e1f2a3b4c5d",
    }));
    bad_req.version = 999;
    assert!(matches!(
        adapter.map_event(&bad_req),
        Err(AdapterError::InvalidPayload { .. })
    ));
    assert_eq!(
        adapter.render_reply(&bad_req, &ReplyAction::Inject { text: "x".into() }),
        ""
    );

    // Host mismatch
    let mut bad_host = bad_req.clone();
    bad_host.version = PROTOCOL_VERSION;
    bad_host.host = Host::Claude;
    assert!(matches!(
        adapter.map_event(&bad_host),
        Err(AdapterError::InvalidPayload { .. })
    ));
    assert_eq!(
        adapter.render_reply(&bad_host, &ReplyAction::Inject { text: "x".into() }),
        ""
    );

    // Payload not an object
    let bad_payload = request(Value::Null);
    assert!(matches!(
        adapter.map_event(&bad_payload),
        Err(AdapterError::InvalidPayload { .. })
    ));
    assert_eq!(
        adapter.render_reply(&bad_payload, &ReplyAction::Inject { text: "x".into() }),
        ""
    );

    // Missing session_id
    let missing_sess = request(json!({
        "hook_event_name": "SessionStart",
    }));
    assert!(matches!(
        adapter.map_event(&missing_sess),
        Err(AdapterError::InvalidPayload { .. })
    ));

    // Conflicting host_event and hook_event_name
    let mut conflict = request(json!({
        "hook_event_name": "SessionStart",
        "session_id": "123",
    }));
    conflict.host_event = Some("UserPromptSubmit".into());
    assert!(matches!(
        adapter.map_event(&conflict),
        Err(AdapterError::InvalidPayload { .. })
    ));

    // Unknown native event maps to None and renders empty string
    let unknown_event = request(json!({
        "hook_event_name": "UnknownCustomEvent",
        "session_id": "123",
    }));
    assert_eq!(adapter.map_event(&unknown_event).unwrap(), None);
    assert_eq!(
        adapter.render_reply(&unknown_event, &ReplyAction::Inject { text: "x".into() }),
        ""
    );
}

#[test]
fn render_reply_json_object_text_renders_empty() {
    let adapter = KimiAdapter::new();
    let req = request(json!({
        "hook_event_name": "UserPromptSubmit",
        "session_id": "9a1b2c3d-4e5f-6a7b-8c9d-0e1f2a3b4c5d",
        "prompt": "hi",
    }));
    let deny = r#"{"hookSpecificOutput":{"permissionDecision":"deny"}}"#;
    assert_eq!(
        adapter.render_reply(&req, &ReplyAction::Inject { text: deny.into() }),
        ""
    );
    assert_eq!(
        adapter.render_reply(
            &req,
            &ReplyAction::Inject {
                text: format!("  \n{deny}")
            }
        ),
        ""
    );
}
