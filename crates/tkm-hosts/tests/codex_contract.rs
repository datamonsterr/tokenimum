#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::{Value, json};
use tkm_core::model::{Call, EditKind, EditVia, FileEdit, HookEvent, StopReason};
use tkm_hosts::codex::CodexAdapter;
use tkm_hosts::{AdapterError, HostAdapter, PruneTarget};
use tkm_proto::{EventKind, Host, PROTOCOL_VERSION, ReplyAction, Request};

const SESSION: &str = "01a18562-fb1c-7522-9939-dda89fa83662";
const TURN: &str = "01a18563-0161-7532-ad25-1247eddeda68";
const CWD: &str = "~/dev/example";

fn fixture(name: &str) -> Value {
    let path = format!(
        "{}/../../fixtures/codex/hooks/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn request(event: EventKind, host_event: Option<&str>, payload: Value) -> Request {
    Request {
        version: PROTOCOL_VERSION,
        host: Host::Codex,
        event,
        host_event: host_event.map(str::to_owned),
        payload,
        env: BTreeMap::new(),
        cwd: None,
    }
}

fn map_fixture(name: &str, event: EventKind, native: &str) -> HookEvent {
    CodexAdapter::new()
        .map_event(&request(event, Some(native), fixture(name)))
        .unwrap()
        .unwrap()
}

fn base(kind: EventKind) -> HookEvent {
    let mut event = HookEvent::new(Host::Codex, kind, SESSION);
    event.cwd = Some(PathBuf::from(CWD));
    event
}

#[test]
fn adapter_identity_and_prune_target() {
    let adapter: Box<dyn HostAdapter + Send + Sync> = Box::new(CodexAdapter::new());
    assert_eq!(adapter.host(), Host::Codex);
    assert_eq!(adapter.prune_target(), PruneTarget::ReportOnly);
}

#[test]
fn maps_session_start() {
    let event = map_fixture("SessionStart", EventKind::SessionStart, "SessionStart");
    assert_eq!(event, base(EventKind::SessionStart));
}

#[test]
fn maps_user_prompt_submit() {
    let event = map_fixture("UserPromptSubmit", EventKind::Prompt, "UserPromptSubmit");
    let mut expected = base(EventKind::Prompt);
    expected.turn_id = Some(TURN.into());
    expected.prompt = Some("Run the shell command 'cat a.txt', then 'false'. Reply done.".into());
    assert_eq!(event, expected);
}

#[test]
fn maps_successful_bash() {
    let event = map_fixture("PostToolUse-bash", EventKind::ToolDone, "PostToolUse");
    let mut expected = base(EventKind::ToolDone);
    expected.turn_id = Some(TURN.into());
    expected.call = Some(Call {
        tool: "Bash".into(),
        args_digest: "cat a.txt".into(),
        output_digest: "hello".into(),
        exit_code: Some(0),
        is_error: false,
        duration_ms: None,
        edits: Vec::new(),
    });
    assert_eq!(event, expected);
}

#[test]
fn maps_failing_bash_to_error_with_exit_code_one() {
    let event = map_fixture("PostToolUse-bash-fail", EventKind::ToolDone, "PostToolUse");
    let call = event.call.unwrap();
    assert_eq!(call.tool, "Bash");
    assert_eq!(call.args_digest, "false");
    assert_eq!(call.output_digest, "");
    assert_eq!(call.exit_code, Some(1));
    assert!(call.is_error);
}

#[test]
fn maps_apply_patch_add_file_to_create_edit() {
    let event = map_fixture(
        "PostToolUse-apply_patch",
        EventKind::ToolDone,
        "PostToolUse",
    );
    let call = event.call.unwrap();
    assert_eq!(call.tool, "apply_patch");
    assert_eq!(call.exit_code, None);
    assert!(!call.is_error);
    assert_eq!(
        call.edits,
        vec![FileEdit {
            path: "c.txt".into(),
            kind: EditKind::Create,
            via: EditVia::Tool,
        }]
    );
    assert_eq!(call.args_digest, r#"[["add","c.txt","x\n"]]"#);
    assert_eq!(call.output_digest, "A ~/dev/example/c.txt");
}

#[test]
fn apply_patch_edits_cover_update_delete_and_reject_outside_paths() {
    let patch = "*** Begin Patch\n*** Update File: src/lib.rs\n@@\n-a\n+b\n\
                 *** Delete File: /work/repo/old.txt\n\
                 *** Add File: /etc/passwd\n+x\n\
                 *** Add File: ../escape.txt\n+x\n\
                 *** Add File: /work/repo/sub/new.txt\n+x\n*** End Patch\n";
    let payload = json!({
        "session_id": SESSION,
        "transcript_path": null,
        "cwd": "/work/repo",
        "hook_event_name": "PostToolUse",
        "turn_id": TURN,
        "tool_name": "apply_patch",
        "tool_input": {"command": patch},
        "tool_response": "Success. Updated the following files:\n"
    });
    let event = CodexAdapter::new()
        .map_event(&request(EventKind::ToolDone, Some("PostToolUse"), payload))
        .unwrap()
        .unwrap();
    let edits: Vec<(String, EditKind)> = event
        .call
        .unwrap()
        .edits
        .into_iter()
        .map(|edit| (edit.path.to_string_lossy().into_owned(), edit.kind))
        .collect();
    assert_eq!(
        edits,
        vec![
            ("src/lib.rs".to_owned(), EditKind::Modify),
            ("old.txt".to_owned(), EditKind::Delete),
            ("sub/new.txt".to_owned(), EditKind::Create),
        ]
    );
}

#[test]
fn maps_mcp_call_with_is_error_from_result() {
    let event = map_fixture("PostToolUse-mcp", EventKind::ToolDone, "PostToolUse");
    let call = event.call.unwrap();
    assert_eq!(call.tool, "mcp__fs__read");
    assert_eq!(call.args_digest, "{\"path\":\"a.txt\"}");
    assert_eq!(call.output_digest, "hello");
    assert_eq!(call.exit_code, None);
    assert!(!call.is_error);

    let mut payload = fixture("PostToolUse-mcp");
    payload["tool_response"] =
        json!({"content": [{"type": "text", "text": "Error: nope"}], "isError": true});
    let event = CodexAdapter::new()
        .map_event(&request(EventKind::ToolDone, Some("PostToolUse"), payload))
        .unwrap()
        .unwrap();
    let call = event.call.unwrap();
    assert!(call.is_error);
    assert_eq!(call.output_digest, "Error: nope");
}

#[test]
fn maps_stop_to_turn_end_and_session_end_with_null_transcript() {
    let stop = map_fixture("Stop", EventKind::TurnEnd, "Stop");
    let mut expected = base(EventKind::TurnEnd);
    expected.turn_id = Some(TURN.into());
    expected.stopped_by = Some(StopReason::OwnStop);
    assert_eq!(stop, expected);

    let end = map_fixture("SessionEnd", EventKind::SessionEnd, "SessionEnd");
    assert_eq!(end, base(EventKind::SessionEnd));
    let end = map_fixture(
        "SessionEnd-null-transcript",
        EventKind::SessionEnd,
        "SessionEnd",
    );
    assert_eq!(end, base(EventKind::SessionEnd));
}

#[test]
fn native_event_falls_back_to_payload_name_and_unknown_events_are_ignored() {
    let adapter = CodexAdapter::new();
    let from_payload = adapter
        .map_event(&request(
            EventKind::Prompt,
            None,
            fixture("UserPromptSubmit"),
        ))
        .unwrap();
    assert_eq!(from_payload.unwrap().kind, EventKind::Prompt);

    let mut payload = fixture("PostToolUse-bash");
    payload["hook_event_name"] = json!("PreToolUse");
    assert!(
        adapter
            .map_event(&request(EventKind::ToolBefore, None, payload))
            .unwrap()
            .is_none()
    );
}

#[test]
fn garbage_payloads_fail_open_with_errors_not_panics() {
    let adapter = CodexAdapter::new();
    let garbage = [
        Value::Null,
        json!("text"),
        json!(7),
        json!([1, 2]),
        json!({}),
        json!({"hook_event_name": "Stop"}),
        json!({"hook_event_name": "Stop", "session_id": 5}),
        json!({"hook_event_name": "Stop", "session_id": ""}),
        json!({"hook_event_name": 5, "session_id": SESSION}),
        json!({"hook_event_name": "PostToolUse", "session_id": SESSION}),
        json!({"hook_event_name": "UserPromptSubmit", "session_id": SESSION}),
    ];
    for payload in garbage {
        let result = adapter.map_event(&request(EventKind::TurnEnd, None, payload.clone()));
        assert!(
            matches!(result, Err(AdapterError::InvalidPayload { .. })),
            "payload {payload} gave {result:?}"
        );
    }

    let mut wrong_version = request(EventKind::Prompt, None, fixture("UserPromptSubmit"));
    wrong_version.version = PROTOCOL_VERSION + 1;
    assert!(adapter.map_event(&wrong_version).is_err());
    let mut wrong_host = request(EventKind::Prompt, None, fixture("UserPromptSubmit"));
    wrong_host.host = Host::Claude;
    assert!(adapter.map_event(&wrong_host).is_err());

    // Odd tool_response shapes never panic and never fabricate failures.
    for response in [
        Value::Null,
        json!(5),
        json!("not json"),
        json!([1]),
        json!({}),
    ] {
        let mut payload = fixture("PostToolUse-bash");
        payload["tool_response"] = response;
        let call = adapter
            .map_event(&request(EventKind::ToolDone, Some("PostToolUse"), payload))
            .unwrap()
            .unwrap()
            .call
            .unwrap();
        assert_eq!(call.exit_code, None);
        assert!(!call.is_error);
    }
}

fn render(event: EventKind, native: Option<&str>, action: &ReplyAction) -> String {
    CodexAdapter::new().render_reply(&request(event, native, Value::Null), action)
}

fn inject(text: &str) -> ReplyAction {
    ReplyAction::Inject { text: text.into() }
}

fn notice(text: &str) -> ReplyAction {
    ReplyAction::Notice { text: text.into() }
}

fn both(inject: &str, notice: &str) -> ReplyAction {
    ReplyAction::InjectAndNotice {
        inject: inject.into(),
        notice: notice.into(),
    }
}

#[test]
fn exact_reply_bytes_for_injectable_events() {
    let cases = [
        (EventKind::SessionStart, "SessionStart"),
        (EventKind::Prompt, "UserPromptSubmit"),
        (EventKind::ToolDone, "PostToolUse"),
    ];
    for (kind, native) in cases {
        let head = format!(
            "{{\"hookSpecificOutput\":{{\"hookEventName\":\"{native}\",\"additionalContext\":\"ctx \\\"q\\\"\"}}"
        );
        assert_eq!(
            render(kind, Some(native), &inject("ctx \"q\"")),
            format!("{head}}}")
        );
        assert_eq!(
            render(kind, Some(native), &notice("note")),
            "{\"systemMessage\":\"note\"}"
        );
        assert_eq!(
            render(kind, Some(native), &both("ctx \"q\"", "note")),
            format!("{head},\"systemMessage\":\"note\"}}")
        );
        assert_eq!(render(kind, Some(native), &ReplyAction::Nothing), "");
        // Empty parts drop out; both empty is the no-op.
        assert_eq!(
            render(kind, Some(native), &both("", "note")),
            "{\"systemMessage\":\"note\"}"
        );
        assert_eq!(render(kind, Some(native), &both("", "")), "");
        assert_eq!(render(kind, Some(native), &inject("")), "");
    }
}

#[test]
fn stop_gets_notice_only_and_session_end_gets_nothing() {
    assert_eq!(
        render(EventKind::TurnEnd, Some("Stop"), &notice("note")),
        "{\"systemMessage\":\"note\"}"
    );
    assert_eq!(
        render(EventKind::TurnEnd, Some("Stop"), &both("ctx", "note")),
        "{\"systemMessage\":\"note\"}"
    );
    assert_eq!(render(EventKind::TurnEnd, Some("Stop"), &inject("ctx")), "");
    assert_eq!(
        render(EventKind::TurnEnd, Some("Stop"), &ReplyAction::Nothing),
        ""
    );

    for action in [
        inject("ctx"),
        notice("note"),
        both("ctx", "note"),
        ReplyAction::Nothing,
    ] {
        assert_eq!(
            render(EventKind::SessionEnd, Some("SessionEnd"), &action),
            ""
        );
    }
}

#[test]
fn stop_reply_never_contains_decision_or_hook_specific_output() {
    for action in [inject("ctx"), notice("note"), both("ctx", "note")] {
        let out = render(EventKind::TurnEnd, Some("Stop"), &action);
        assert!(!out.contains("decision"), "{out}");
        assert!(!out.contains("hookSpecificOutput"), "{out}");
    }
}

#[test]
fn native_event_names_are_required_and_payload_names_are_used() {
    // Mapping and rendering both require a native name.
    assert_eq!(render(EventKind::Prompt, None, &inject("c")), "");
    assert_eq!(render(EventKind::SessionEnd, None, &notice("n")), "");
    assert_eq!(render(EventKind::TurnEnd, None, &notice("n")), "");
    let missing = request(EventKind::TurnEnd, None, json!({"session_id":SESSION}));
    assert!(matches!(
        CodexAdapter::new().map_event(&missing),
        Err(AdapterError::InvalidPayload { .. })
    ));
    assert_eq!(
        CodexAdapter::new().render_reply(&missing, &both("c", "n")),
        ""
    );
    // Events Codex tokenimum does not handle never get output.
    assert_eq!(render(EventKind::ToolBefore, None, &inject("c")), "");
    assert_eq!(
        render(EventKind::ToolDone, Some("PreToolUse"), &inject("c")),
        ""
    );
    // The payload's own event name is used when the request lacks host_event.
    let mut with_payload = request(
        EventKind::ToolDone,
        None,
        json!({"hook_event_name": "PostToolUse"}),
    );
    with_payload.event = EventKind::ToolDone;
    assert_eq!(
        CodexAdapter::new().render_reply(&with_payload, &inject("c")),
        "{\"hookSpecificOutput\":{\"hookEventName\":\"PostToolUse\",\"additionalContext\":\"c\"}}"
    );
}

#[test]
fn replies_are_capped_at_10000_chars_on_a_char_boundary() {
    let long = "é".repeat(10_001);
    let out = render(
        EventKind::Prompt,
        Some("UserPromptSubmit"),
        &both(&long, &long),
    );
    let value: Value = serde_json::from_str(&out).unwrap();
    let context = value["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .unwrap();
    let message = value["systemMessage"].as_str().unwrap();
    assert_eq!(context.chars().count(), 10_000);
    assert_eq!(message.chars().count(), 10_000);
    assert!(long.starts_with(context));
    assert!(out.starts_with("{\"hookSpecificOutput\""));
    assert!(out.find("hookSpecificOutput").unwrap() < out.find("systemMessage").unwrap());

    let exact = "é".repeat(10_000);
    let out = render(EventKind::TurnEnd, Some("Stop"), &notice(&exact));
    assert!(out.contains(&exact));
}

#[test]
fn conflicting_native_events_fail_mapping_and_render_nothing() {
    let adapter = CodexAdapter::new();
    for (native, host_event) in [
        ("SessionStart", "Stop"),
        ("Stop", "SessionStart"),
        ("SessionEnd", "SessionStart"),
    ] {
        let mut req = request(
            EventKind::TurnEnd,
            None,
            json!({"session_id":"s", "hook_event_name":native}),
        );
        req.host_event = Some(host_event.into());
        assert!(matches!(
            adapter.map_event(&req),
            Err(AdapterError::InvalidPayload { .. })
        ));
        assert_eq!(
            adapter.render_reply(
                &req,
                &ReplyAction::InjectAndNotice {
                    inject: "context".into(),
                    notice: "notice".into()
                }
            ),
            ""
        );
    }
}
