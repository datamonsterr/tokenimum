#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::PathBuf};
use tkm_core::model::{Call, EditKind, EditVia, FileEdit, HookEvent};
use tkm_hosts::{AdapterError, HostAdapter, PruneTarget, claude::ClaudeAdapter};
use tkm_proto::{EventKind, Host, PROTOCOL_VERSION, ReplyAction, Request};

fn request(payload: Value) -> Request {
    Request {
        version: PROTOCOL_VERSION,
        host: Host::Claude,
        event: EventKind::Prompt,
        host_event: None,
        payload,
        env: BTreeMap::new(),
        cwd: None,
    }
}

#[test]
fn every_claude_hook_fixture_maps_to_expected_event() {
    let adapter = ClaudeAdapter::new();
    for entry in fs::read_dir(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/claude/hooks"
    ))
    .unwrap()
    {
        let path = entry.unwrap().path();
        let p: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        let native = p["hook_event_name"].as_str().unwrap();
        let kind = match native {
            "SessionStart" => EventKind::SessionStart,
            "UserPromptSubmit" => EventKind::Prompt,
            "PostToolUse" | "PostToolUseFailure" => EventKind::ToolDone,
            "Stop" => EventKind::TurnEnd,
            "SessionEnd" => EventKind::SessionEnd,
            _ => panic!("unexpected fixture"),
        };
        let mut expected =
            HookEvent::new(Host::Claude, kind, "e928a2cf-e31b-4154-805b-919991d042ea");
        expected.cwd = Some(PathBuf::from("~/dev/example"));
        expected.turn_id = p["prompt_id"].as_str().map(str::to_owned);
        if kind == EventKind::Prompt {
            expected.prompt = p["prompt"].as_str().map(str::to_owned);
        }
        if kind == EventKind::ToolDone {
            let tool = p["tool_name"].as_str().unwrap();
            let output = match tool {
                "Bash" if native == "PostToolUseFailure" => "Exit code 1".into(),
                "Bash" | "Read" => "hello".into(),
                "Write" => "x".into(),
                _ => serde_json::to_string(&p["tool_response"]).unwrap(),
            };
            let mut call = Call {
                tool: tool.into(),
                args_digest: if tool == "Bash" {
                    p["tool_input"]["command"].as_str().unwrap().into()
                } else {
                    serde_json::to_string(&p["tool_input"]).unwrap()
                },
                output_digest: output,
                is_error: native == "PostToolUseFailure",
                exit_code: (native == "PostToolUseFailure").then_some(1),
                duration_ms: p["duration_ms"].as_u64(),
                ..Call::default()
            };
            if matches!(tool, "Write" | "Edit") {
                call.edits.push(FileEdit {
                    path: if tool == "Write" {
                        "b.txt".into()
                    } else {
                        "a.txt".into()
                    },
                    kind: if tool == "Write" {
                        EditKind::Create
                    } else {
                        EditKind::Modify
                    },
                    via: EditVia::Tool,
                });
            }
            expected.call = Some(call);
        }
        assert_eq!(
            adapter.map_event(&request(p)).unwrap(),
            Some(expected),
            "{}",
            path.display()
        );
    }
    assert_eq!(adapter.host(), Host::Claude);
    assert_eq!(
        adapter.prune_target(),
        PruneTarget::SettingsDenyList {
            path: ".claude/settings.local.json".into(),
            key: "permissions.deny".into()
        }
    );
}

#[test]
#[expect(
    clippy::default_constructed_unit_structs,
    reason = "Exercise the required Default implementation."
)]
fn every_event_action_pair_has_exact_advisory_reply_bytes() {
    let adapter = ClaudeAdapter::default();
    for event in [
        "SessionStart",
        "UserPromptSubmit",
        "PostToolUse",
        "PostToolUseFailure",
        "Stop",
        "SessionEnd",
        "FutureEvent",
    ] {
        let req = request(
            json!({"session_id":"s", "hook_event_name":event,"prompt":"p","tool_name":"Bash","tool_input":{},"error":"Exit code 1"}),
        );
        let injection = format!(
            "{{\"hookSpecificOutput\":{{\"hookEventName\":\"{event}\",\"additionalContext\":\"context\"}}}}"
        );
        let combined = format!(
            "{{\"hookSpecificOutput\":{{\"hookEventName\":\"{event}\",\"additionalContext\":\"context\"}},\"systemMessage\":\"notice\"}}"
        );
        let notice = "{\"systemMessage\":\"notice\"}";
        for (action, expected) in [
            (ReplyAction::Nothing, "".to_owned()),
            (
                ReplyAction::Inject {
                    text: "context".into(),
                },
                if matches!(event, "Stop" | "SessionEnd" | "FutureEvent") {
                    "".into()
                } else {
                    injection
                },
            ),
            (
                ReplyAction::Notice {
                    text: "notice".into(),
                },
                if matches!(event, "SessionEnd" | "FutureEvent") {
                    "".into()
                } else {
                    notice.into()
                },
            ),
            (
                ReplyAction::InjectAndNotice {
                    inject: "context".into(),
                    notice: "notice".into(),
                },
                match event {
                    "Stop" => notice.into(),
                    "SessionEnd" | "FutureEvent" => "".into(),
                    _ => combined,
                },
            ),
        ] {
            assert_eq!(
                adapter.render_reply(&req, &action),
                expected,
                "{event}: {action:?}"
            );
        }
    }
}

#[test]
fn reply_caps_multibyte_text_by_characters_and_escapes_json() {
    let req = request(json!({"session_id":"s", "hook_event_name":"UserPromptSubmit","prompt":"p"}));
    let text = format!("\"\n{}", "界🦀".repeat(6000));
    let capped: String = text.chars().take(10_000).collect();
    let quoted = serde_json::to_string(&capped).unwrap();
    assert_eq!(
        ClaudeAdapter::new().render_reply(
            &req,
            &ReplyAction::InjectAndNotice {
                inject: text.clone(),
                notice: text
            }
        ),
        format!(
            "{{\"hookSpecificOutput\":{{\"hookEventName\":\"UserPromptSubmit\",\"additionalContext\":{quoted}}},\"systemMessage\":{quoted}}}"
        )
    );
}

#[test]
fn malformed_payloads_fail_without_secret_values_and_render_empty() {
    let adapter = ClaudeAdapter::new();
    for payload in [
        Value::Null,
        json!([]),
        json!("private-secret"),
        json!({}),
        json!({"session_id":3}),
        json!({"session_id":"s","hook_event_name":false}),
        json!({"session_id":"s","hook_event_name":"UserPromptSubmit","prompt":[]}),
        json!({"session_id":"s","hook_event_name":"PostToolUse","tool_name":7}),
    ] {
        let req = request(payload);
        let error = adapter.map_event(&req).unwrap_err();
        assert!(matches!(error, AdapterError::InvalidPayload { .. }));
        assert!(!error.to_string().contains("private-secret"));
        assert_eq!(
            adapter.render_reply(
                &req,
                &ReplyAction::Notice {
                    text: "notice".into()
                }
            ),
            ""
        );
    }
    let mut wrong_version = request(json!({"session_id":"s","hook_event_name":"Stop"}));
    wrong_version.version = 0;
    assert!(adapter.map_event(&wrong_version).is_err());
    assert_eq!(
        adapter.render_reply(
            &wrong_version,
            &ReplyAction::Notice {
                text: "notice".into()
            }
        ),
        ""
    );
    let mut req = request(json!({"session_id":"s"}));
    req.host = Host::Codex;
    assert!(adapter.map_event(&req).is_err());
}

#[test]
fn fallback_event_and_edit_containment_are_pure() {
    let adapter = ClaudeAdapter::new();
    let mut req = request(json!({"session_id":"s"}));
    req.host_event = Some("SessionStart".into());
    assert_eq!(
        adapter.map_event(&req).unwrap().unwrap().kind,
        EventKind::SessionStart
    );
    req.host_event = Some("FutureEvent".into());
    assert_eq!(adapter.map_event(&req).unwrap(), None);
    for path in ["/repo/../secret", "../secret", "/repo-other/file"] {
        let req = request(
            json!({"session_id":"s","hook_event_name":"PostToolUse","cwd":"/repo","tool_name":"Write","tool_input":{"file_path":path},"tool_response":{"type":"create"}}),
        );
        assert!(
            adapter
                .map_event(&req)
                .unwrap()
                .unwrap()
                .call
                .unwrap()
                .edits
                .is_empty()
        );
    }
}

#[test]
fn timestamps_validate_calendar_offsets_and_pre_epoch_fractional_seconds() {
    for (text, expected) in [
        ("1970-01-01T00:00:00Z", Some(0)),
        ("1969-12-31T23:59:59.999Z", Some(-1)),
        ("1970-01-01T01:30:00.123456+01:30", Some(123)),
        ("2026-02-29T00:00:00Z", None),
        ("1970-01-01T00:00:00+99:00", None),
        ("private", None),
    ] {
        let req =
            request(json!({"session_id":"s","hook_event_name":"SessionStart","timestamp":text}));
        let event = ClaudeAdapter::new().map_event(&req).unwrap().unwrap();
        assert_eq!(event.timestamp.map(|value| value.0), expected, "{text}");
    }
}
