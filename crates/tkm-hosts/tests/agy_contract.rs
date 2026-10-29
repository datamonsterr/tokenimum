#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::{Value, json};
use std::{collections::BTreeMap, fs, path::PathBuf};
use tkm_core::model::{Call, EditKind, EditVia, FileEdit, HookEvent, StopReason};
use tkm_hosts::{AdapterError, HostAdapter, PruneTarget, agy::AgyAdapter};
use tkm_proto::{EventKind, Host, PROTOCOL_VERSION, ReplyAction, Request};

fn request(payload: Value, host_event: Option<&str>) -> Request {
    Request {
        version: PROTOCOL_VERSION,
        host: Host::Agy,
        event: EventKind::Prompt,
        host_event: host_event.map(str::to_owned),
        payload,
        env: BTreeMap::new(),
        cwd: None,
    }
}

#[test]
fn every_agy_hook_fixture_maps_to_expected_event() {
    let adapter = AgyAdapter::new();
    let fixtures_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/agy/hooks");

    // SessionStart.json
    let session_start_bytes = fs::read(fixtures_dir.join("SessionStart.json")).unwrap();
    let p_start: Value = serde_json::from_slice(&session_start_bytes).unwrap();
    let mut req_start = request(p_start.clone(), Some("SessionStart"));
    req_start.event = EventKind::SessionStart;
    let mapped_start = adapter.map_event(&req_start).unwrap();
    let mut expected_start = HookEvent::new(
        Host::Agy,
        EventKind::SessionStart,
        "14db3cb3-737b-4e2a-a95e-a086d0124d54",
    );
    expected_start.cwd = Some(PathBuf::from("/home/user/project"));
    expected_start.prompt = Some(
        "Create file a.txt containing the word alpha, then edit it to say beta, then run the shell command 'false', then reply done.".into(),
    );
    assert_eq!(mapped_start, Some(expected_start));

    // PreInvocation.json (invocationNum: 0) -> maps to Prompt
    let pre_inv_bytes = fs::read(fixtures_dir.join("PreInvocation.json")).unwrap();
    let p_pre: Value = serde_json::from_slice(&pre_inv_bytes).unwrap();
    let mut req_pre = request(p_pre.clone(), Some("PreInvocation"));
    req_pre.event = EventKind::Prompt;
    let mapped_pre = adapter.map_event(&req_pre).unwrap();
    let mut expected_pre = HookEvent::new(
        Host::Agy,
        EventKind::Prompt,
        "14db3cb3-737b-4e2a-a95e-a086d0124d54",
    );
    expected_pre.cwd = Some(PathBuf::from("/home/user/project"));
    expected_pre.prompt = Some(
        "Create file a.txt containing the word alpha, then edit it to say beta, then run the shell command 'false', then reply done.".into(),
    );
    assert_eq!(mapped_pre, Some(expected_pre));

    // PreInvocation-2.json (invocationNum: 1) -> maps to None
    let pre_inv_2_bytes = fs::read(fixtures_dir.join("PreInvocation-2.json")).unwrap();
    let p_pre_2: Value = serde_json::from_slice(&pre_inv_2_bytes).unwrap();
    let req_pre_2 = request(p_pre_2, Some("PreInvocation"));
    assert_eq!(adapter.map_event(&req_pre_2).unwrap(), None);

    // PostInvocation.json -> maps to None
    let post_inv_bytes = fs::read(fixtures_dir.join("PostInvocation.json")).unwrap();
    let p_post: Value = serde_json::from_slice(&post_inv_bytes).unwrap();
    let req_post = request(p_post, Some("PostInvocation"));
    assert_eq!(adapter.map_event(&req_post).unwrap(), None);

    // Stop.json -> maps to TurnEnd
    let stop_bytes = fs::read(fixtures_dir.join("Stop.json")).unwrap();
    let p_stop: Value = serde_json::from_slice(&stop_bytes).unwrap();
    let mut req_stop = request(p_stop.clone(), Some("Stop"));
    req_stop.event = EventKind::TurnEnd;
    let mapped_stop = adapter.map_event(&req_stop).unwrap();
    let mut expected_stop = HookEvent::new(
        Host::Agy,
        EventKind::TurnEnd,
        "14db3cb3-737b-4e2a-a95e-a086d0124d54",
    );
    expected_stop.cwd = Some(PathBuf::from("/home/user/project"));
    expected_stop.stopped_by = Some(StopReason::OwnStop);
    assert_eq!(mapped_stop, Some(expected_stop));

    assert_eq!(adapter.host(), Host::Agy);
    assert_eq!(adapter.prune_target(), PruneTarget::ReportOnly);
}

#[test]
fn pre_invocation_invocation_num_one_maps_to_none() {
    let adapter = AgyAdapter::new();
    let p = json!({
        "conversationId": "session-1",
        "invocationNum": 1,
        "lastUserInput": "test",
        "workspacePaths": ["/project"]
    });
    let req = request(p, Some("PreInvocation"));
    assert_eq!(adapter.map_event(&req).unwrap(), None);
}

#[test]
fn post_tool_use_mapping_and_edits() {
    let adapter = AgyAdapter::new();

    // 1. run_command with CommandLine
    let p_cmd = json!({
        "conversationId": "session-1",
        "workspacePaths": ["/project"],
        "toolCall": {
            "name": "run_command",
            "args": {
                "CommandLine": "echo hello",
                "WaitMsBeforeAsync": 5000
            }
        },
        "error": ""
    });
    let mut req_cmd = request(p_cmd, Some("PostToolUse"));
    req_cmd.event = EventKind::ToolDone;
    let mapped_cmd = adapter.map_event(&req_cmd).unwrap().unwrap();
    let call_cmd = mapped_cmd.call.unwrap();
    assert_eq!(call_cmd.tool, "run_command");
    assert_eq!(call_cmd.args_digest, "echo hello");
    assert!(!call_cmd.is_error);

    // 2. run_command with error
    let p_err = json!({
        "conversationId": "session-1",
        "workspacePaths": ["/project"],
        "toolCall": {
            "name": "run_command",
            "args": {
                "CommandLine": "false"
            }
        },
        "error": "command returned non-zero exit status"
    });
    let req_err = request(p_err, Some("PostToolUse"));
    let mapped_err = adapter.map_event(&req_err).unwrap().unwrap();
    let call_err = mapped_err.call.unwrap();
    assert_eq!(call_err.tool, "run_command");
    assert_eq!(call_err.args_digest, "false");
    assert!(call_err.is_error);

    // 3. write_file with Create edit
    let p_write = json!({
        "conversationId": "session-1",
        "workspacePaths": ["/project"],
        "toolCall": {
            "name": "write_file",
            "args": {
                "Path": "/project/src/main.rs",
                "Content": "fn main() {}"
            }
        },
        "error": ""
    });
    let req_write = request(p_write, Some("PostToolUse"));
    let mapped_write = adapter.map_event(&req_write).unwrap().unwrap();
    let call_write = mapped_write.call.unwrap();
    assert_eq!(call_write.tool, "write_file");
    assert_eq!(
        call_write.edits,
        vec![FileEdit {
            path: PathBuf::from("src/main.rs"),
            kind: EditKind::Create,
            via: EditVia::Tool,
        }]
    );

    // 4. edit_file with Modify edit
    let p_edit = json!({
        "conversationId": "session-1",
        "workspacePaths": ["/project"],
        "toolCall": {
            "name": "edit_file",
            "args": {
                "Path": "/project/src/main.rs",
                "Target": "fn main() {}",
                "Replacement": "fn main() { println!(); }"
            }
        },
        "error": ""
    });
    let req_edit = request(p_edit, Some("PostToolUse"));
    let mapped_edit = adapter.map_event(&req_edit).unwrap().unwrap();
    let call_edit = mapped_edit.call.unwrap();
    assert_eq!(call_edit.tool, "edit_file");
    assert_eq!(
        call_edit.edits,
        vec![FileEdit {
            path: PathBuf::from("src/main.rs"),
            kind: EditKind::Modify,
            via: EditVia::Tool,
        }]
    );

    // 5. replace_file_content with TargetFile
    let p_replace = json!({
        "conversationId": "session-1",
        "workspacePaths": ["/project"],
        "toolCall": {
            "name": "replace_file_content",
            "args": {
                "TargetFile": "/project/README.md"
            }
        },
        "error": ""
    });
    let req_replace = request(p_replace, Some("PostToolUse"));
    let mapped_replace = adapter.map_event(&req_replace).unwrap().unwrap();
    let call_replace = mapped_replace.call.unwrap();
    assert_eq!(
        call_replace.edits,
        vec![FileEdit {
            path: PathBuf::from("README.md"),
            kind: EditKind::Modify,
            via: EditVia::Tool,
        }]
    );
}

#[test]
fn stop_with_error_maps_to_stop_reason_error() {
    let adapter = AgyAdapter::new();
    let p_err = json!({
        "conversationId": "session-1",
        "workspacePaths": ["/project"],
        "error": "internal model error",
        "fullyIdle": true
    });
    let req_err = request(p_err, Some("Stop"));
    let mapped_err = adapter.map_event(&req_err).unwrap().unwrap();
    assert_eq!(mapped_err.stopped_by, Some(StopReason::Error));

    let p_ok = json!({
        "conversationId": "session-1",
        "workspacePaths": ["/project"],
        "error": "",
        "fullyIdle": true
    });
    let req_ok = request(p_ok, Some("Stop"));
    let mapped_ok = adapter.map_event(&req_ok).unwrap().unwrap();
    assert_eq!(mapped_ok.stopped_by, Some(StopReason::OwnStop));
}

#[test]
fn exact_stdout_bytes_for_each_render_case() {
    let adapter = AgyAdapter::new();

    // SessionStart
    let p_start = json!({
        "conversationId": "session-1",
        "workspacePaths": ["/project"]
    });
    let req_start = request(p_start, Some("SessionStart"));

    assert_eq!(
        adapter.render_reply(
            &req_start,
            &ReplyAction::Inject {
                text: "hello".into()
            }
        ),
        r#"{"injectSteps":[{"systemMessage":{"systemMessage":"hello"}}]}"#
    );
    assert_eq!(
        adapter.render_reply(
            &req_start,
            &ReplyAction::InjectAndNotice {
                inject: "hello".into(),
                notice: "advisory note".into(),
            }
        ),
        r#"{"injectSteps":[{"systemMessage":{"systemMessage":"hello"}}]}"#
    );
    assert_eq!(
        adapter.render_reply(
            &req_start,
            &ReplyAction::Notice {
                text: "advisory note".into()
            }
        ),
        "{}"
    );
    assert_eq!(
        adapter.render_reply(&req_start, &ReplyAction::Nothing),
        "{}"
    );

    // PreInvocation (invocationNum: 0)
    let p_pre = json!({
        "conversationId": "session-1",
        "workspacePaths": ["/project"],
        "invocationNum": 0
    });
    let req_pre = request(p_pre, Some("PreInvocation"));

    assert_eq!(
        adapter.render_reply(
            &req_pre,
            &ReplyAction::Inject {
                text: "hello ephemeral".into()
            }
        ),
        r#"{"injectSteps":[{"ephemeralMessage":"hello ephemeral"}]}"#
    );
    assert_eq!(
        adapter.render_reply(
            &req_pre,
            &ReplyAction::InjectAndNotice {
                inject: "hello ephemeral".into(),
                notice: "advisory note".into(),
            }
        ),
        r#"{"injectSteps":[{"ephemeralMessage":"hello ephemeral"}]}"#
    );
    assert_eq!(
        adapter.render_reply(
            &req_pre,
            &ReplyAction::Notice {
                text: "advisory note".into()
            }
        ),
        "{}"
    );
    assert_eq!(adapter.render_reply(&req_pre, &ReplyAction::Nothing), "{}");

    // PreInvocation with invocationNum: 1 -> renders no-op {}
    let p_pre_1 = json!({
        "conversationId": "session-1",
        "workspacePaths": ["/project"],
        "invocationNum": 1
    });
    let req_pre_1 = request(p_pre_1, Some("PreInvocation"));
    assert_eq!(
        adapter.render_reply(
            &req_pre_1,
            &ReplyAction::Inject {
                text: "hello".into()
            }
        ),
        "{}"
    );

    // Stop -> renders no-op {}
    let p_stop = json!({
        "conversationId": "session-1",
        "workspacePaths": ["/project"]
    });
    let req_stop = request(p_stop, Some("Stop"));
    assert_eq!(
        adapter.render_reply(
            &req_stop,
            &ReplyAction::Inject {
                text: "hello".into()
            }
        ),
        "{}"
    );

    // PostToolUse -> renders no-op {}
    let p_tool = json!({
        "conversationId": "session-1",
        "workspacePaths": ["/project"],
        "toolCall": {"name": "run_command", "args": {"CommandLine": "ls"}}
    });
    let req_tool = request(p_tool, Some("PostToolUse"));
    assert_eq!(
        adapter.render_reply(
            &req_tool,
            &ReplyAction::Inject {
                text: "hello".into()
            }
        ),
        "{}"
    );

    // Escaped JSON strings in injection
    assert_eq!(
        adapter.render_reply(
            &req_start,
            &ReplyAction::Inject {
                text: "line 1\n\"quotes\" and \\slash".into()
            }
        ),
        r#"{"injectSteps":[{"systemMessage":{"systemMessage":"line 1\n\"quotes\" and \\slash"}}]}"#
    );

    // Truncation at 10,000 characters
    let long_text = "a".repeat(10_050);
    let expected_truncated = "a".repeat(10_000);
    assert_eq!(
        adapter.render_reply(&req_start, &ReplyAction::Inject { text: long_text }),
        format!(
            r#"{{"injectSteps":[{{"systemMessage":{{"systemMessage":"{expected_truncated}"}}}}]}}"#
        )
    );
}

#[test]
fn malformed_and_fault_injection() {
    let adapter = AgyAdapter::new();

    // Protocol version mismatch
    let mut req_ver = request(json!({"conversationId": "s1"}), Some("SessionStart"));
    req_ver.version = 999;
    assert!(matches!(
        adapter.map_event(&req_ver),
        Err(AdapterError::InvalidPayload { .. })
    ));
    assert_eq!(
        adapter.render_reply(&req_ver, &ReplyAction::Inject { text: "hi".into() }),
        "{}"
    );

    // Host mismatch
    let mut req_host = request(json!({"conversationId": "s1"}), Some("SessionStart"));
    req_host.host = Host::Claude;
    assert!(matches!(
        adapter.map_event(&req_host),
        Err(AdapterError::InvalidPayload { .. })
    ));
    assert_eq!(
        adapter.render_reply(&req_host, &ReplyAction::Inject { text: "hi".into() }),
        "{}"
    );

    // Payload is not an object
    let req_null = request(Value::Null, Some("SessionStart"));
    assert!(matches!(
        adapter.map_event(&req_null),
        Err(AdapterError::InvalidPayload { .. })
    ));
    assert_eq!(
        adapter.render_reply(&req_null, &ReplyAction::Inject { text: "hi".into() }),
        "{}"
    );

    // Missing conversationId and no env fallback
    let req_no_id = request(
        json!({"workspacePaths": ["/project"]}),
        Some("SessionStart"),
    );
    assert!(matches!(
        adapter.map_event(&req_no_id),
        Err(AdapterError::InvalidPayload { .. })
    ));
    assert_eq!(
        adapter.render_reply(&req_no_id, &ReplyAction::Inject { text: "hi".into() }),
        "{}"
    );

    // Missing conversationId but ANTIGRAVITY_CONVERSATION_ID in env succeeds
    let mut req_env = request(
        json!({"workspacePaths": ["/project"]}),
        Some("SessionStart"),
    );
    req_env.env.insert(
        "ANTIGRAVITY_CONVERSATION_ID".to_owned(),
        "env-fallback-id".to_owned(),
    );
    let mapped = adapter.map_event(&req_env).unwrap().unwrap();
    assert_eq!(mapped.session_id, "env-fallback-id");

    // Missing native event
    let req_no_event = request(json!({"conversationId": "s1"}), None);
    assert!(matches!(
        adapter.map_event(&req_no_no_event_helper(&req_no_event)),
        Err(AdapterError::InvalidPayload { .. })
    ));
}

fn req_no_no_event_helper(req: &Request) -> Request {
    req.clone()
}
