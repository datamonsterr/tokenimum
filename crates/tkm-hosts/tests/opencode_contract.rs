#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::Value;
use std::{collections::BTreeMap, fs, path::PathBuf};
use tkm_core::model::{Call, EditKind, EditVia, FileEdit, HookEvent, Timestamp};
use tkm_hosts::{AdapterError, HostAdapter, PruneTarget, opencode::OpencodeAdapter};
use tkm_proto::{EventKind, Host, PROTOCOL_VERSION, ReplyAction, Request};

const SESSION: &str = "ses_example1";
const PROJECT: &str = "/home/user/project";

fn fixture(name: &str) -> Value {
    let path = format!(
        "{}/tests/fixtures/opencode/hooks/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

/// The shim passes the plugin hook name, or the event type for `event` hooks.
fn request(name: &str, kind: EventKind) -> Request {
    let payload = fixture(name);
    let host_event = if payload["hook"] == "event" {
        payload["event"]["type"].as_str()
    } else {
        payload["hook"].as_str()
    }
    .map(str::to_owned);
    Request {
        version: PROTOCOL_VERSION,
        host: Host::Opencode,
        event: kind,
        host_event,
        payload,
        env: BTreeMap::new(),
        cwd: Some(PROJECT.to_owned()),
    }
}

fn base(kind: EventKind) -> HookEvent {
    let mut event = HookEvent::new(Host::Opencode, kind, SESSION);
    event.cwd = Some(PathBuf::from(PROJECT));
    event
}

fn map(name: &str, kind: EventKind) -> Option<HookEvent> {
    OpencodeAdapter::new()
        .map_event(&request(name, kind))
        .unwrap()
}

fn reply(name: &str, kind: EventKind, action: &ReplyAction) -> String {
    OpencodeAdapter::new().render_reply(&request(name, kind), action)
}

#[test]
fn adapter_reports_opencode() {
    assert_eq!(OpencodeAdapter::new().host(), Host::Opencode);
}

#[test]
fn session_created_maps_to_session_start() {
    let mut expected = base(EventKind::SessionStart);
    expected.timestamp = Some(Timestamp(1_791_436_988_255));
    assert_eq!(
        map("event-session-created", EventKind::SessionStart),
        Some(expected)
    );
}

#[test]
fn chat_message_maps_to_prompt_with_only_real_text_parts() {
    let mut expected = base(EventKind::Prompt);
    expected.turn_id = Some("msg_example1".to_owned());
    expected.timestamp = Some(Timestamp(1_791_436_988_425));
    expected.prompt = Some("Fix the failing test\nand run it".to_owned());
    assert_eq!(map("chat-message", EventKind::Prompt), Some(expected));
}

#[test]
fn bash_tool_maps_to_call_with_exit_code_from_metadata() {
    let mut expected = base(EventKind::ToolDone);
    expected.call = Some(Call {
        tool: "bash".into(),
        args_digest: "ls -la".into(),
        output_digest: "b.txt".into(),
        exit_code: Some(0),
        ..Call::default()
    });
    assert_eq!(
        map("tool-execute-after-bash", EventKind::ToolDone),
        Some(expected)
    );
    let call = map("tool-execute-after-bash-exit1", EventKind::ToolDone)
        .unwrap()
        .call
        .unwrap();
    assert_eq!((call.exit_code, call.is_error), (Some(1), false));
}

#[test]
fn write_and_edit_tools_map_to_repo_relative_edits() {
    let write = map("tool-execute-after-write", EventKind::ToolDone)
        .unwrap()
        .call
        .unwrap();
    assert_eq!(
        write.edits,
        vec![FileEdit {
            path: "src/new.rs".into(),
            kind: EditKind::Create,
            via: EditVia::Tool
        }]
    );
    assert_eq!(
        write.args_digest,
        r#"{"content":"x","filePath":"/home/user/project/src/new.rs"}"#
    );
    assert_eq!(write.output_digest, "Wrote file successfully.");
    let edit = map("tool-execute-after-edit", EventKind::ToolDone)
        .unwrap()
        .call
        .unwrap();
    assert_eq!(
        edit.edits,
        vec![FileEdit {
            path: "src/lib.rs".into(),
            kind: EditKind::Modify,
            via: EditVia::Tool
        }]
    );
    let outside = map("tool-execute-after-edit-outside", EventKind::ToolDone)
        .unwrap()
        .call
        .unwrap();
    assert_eq!(outside.edits.len(), 0);
}

#[test]
fn idle_and_deleted_events_map_to_turn_and_session_end() {
    assert_eq!(
        map("event-session-idle", EventKind::TurnEnd),
        Some(base(EventKind::TurnEnd))
    );
    let mut ended = base(EventKind::SessionEnd);
    ended.timestamp = None;
    assert_eq!(
        map("event-session-deleted", EventKind::SessionEnd),
        Some(ended)
    );
}

#[test]
fn other_events_are_ignored() {
    assert_eq!(map("event-message-updated", EventKind::TurnEnd), None);
    let mut request = request("chat-message", EventKind::Prompt);
    request.payload["hook"] = "chat.params".into();
    request.host_event = Some("chat.params".into());
    assert_eq!(OpencodeAdapter::new().map_event(&request).unwrap(), None);
}

#[test]
fn every_fixture_maps_without_error() {
    for entry in fs::read_dir(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/opencode/hooks"
    ))
    .unwrap()
    {
        let name = entry
            .unwrap()
            .path()
            .file_stem()
            .unwrap()
            .to_str()
            .unwrap()
            .to_owned();
        let request = request(&name, EventKind::Prompt);
        OpencodeAdapter::new().map_event(&request).unwrap();
    }
}

#[test]
fn invalid_requests_are_rejected() {
    let adapter = OpencodeAdapter::new();
    let good = request("chat-message", EventKind::Prompt);
    let mut version = good.clone();
    version.version = PROTOCOL_VERSION + 1;
    let mut host = good.clone();
    host.host = Host::Pi;
    let mut conflict = good.clone();
    conflict.host_event = Some("tool.execute.after".into());
    let mut no_session = good.clone();
    no_session.payload["input"]["sessionID"] = Value::Null;
    let mut not_object = good.clone();
    not_object.payload = Value::Null;
    let mut no_output = good;
    no_output.payload["output"] = Value::Null;
    for bad in [version, host, conflict, no_session, not_object, no_output] {
        assert!(
            matches!(
                adapter.map_event(&bad),
                Err(AdapterError::InvalidPayload { .. })
            ),
            "{bad:?}"
        );
        assert_eq!(
            adapter.render_reply(&bad, &ReplyAction::Inject { text: "x".into() }),
            ""
        );
    }
}

#[test]
fn chat_message_reply_carries_context_and_notice_in_fixed_order() {
    let inject = ReplyAction::Inject { text: "ctx".into() };
    let notice = ReplyAction::Notice { text: "n".into() };
    let both = ReplyAction::InjectAndNotice {
        inject: "ctx".into(),
        notice: "n".into(),
    };
    assert_eq!(
        reply("chat-message", EventKind::Prompt, &inject),
        r#"{"context":"ctx"}"#
    );
    assert_eq!(
        reply("chat-message", EventKind::Prompt, &notice),
        r#"{"notice":"n"}"#
    );
    assert_eq!(
        reply("chat-message", EventKind::Prompt, &both),
        r#"{"context":"ctx","notice":"n"}"#
    );
    assert_eq!(
        reply("chat-message", EventKind::Prompt, &ReplyAction::Nothing),
        ""
    );
    let escaped = ReplyAction::Inject {
        text: "a\"b\nc".into(),
    };
    assert_eq!(
        reply("chat-message", EventKind::Prompt, &escaped),
        r#"{"context":"a\"b\nc"}"#
    );
    let empty = ReplyAction::Inject {
        text: String::new(),
    };
    assert_eq!(reply("chat-message", EventKind::Prompt, &empty), "");
}

#[test]
fn context_is_capped_at_ten_thousand_chars_on_a_char_boundary() {
    let text = "é".repeat(12_000);
    let out = reply(
        "chat-message",
        EventKind::Prompt,
        &ReplyAction::Inject { text },
    );
    let value: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(value["context"].as_str().unwrap().chars().count(), 10_000);
}

#[test]
fn injection_is_only_allowed_on_chat_message() {
    let both = ReplyAction::InjectAndNotice {
        inject: "ctx".into(),
        notice: "n".into(),
    };
    for (name, kind) in [
        ("event-session-created", EventKind::SessionStart),
        ("tool-execute-after-bash", EventKind::ToolDone),
        ("event-session-idle", EventKind::TurnEnd),
    ] {
        assert_eq!(reply(name, kind, &both), r#"{"notice":"n"}"#, "{name}");
        assert_eq!(
            reply(name, kind, &ReplyAction::Inject { text: "ctx".into() }),
            "",
            "{name}"
        );
    }
}

#[test]
fn session_end_never_carries_a_reply() {
    let both = ReplyAction::InjectAndNotice {
        inject: "ctx".into(),
        notice: "n".into(),
    };
    assert_eq!(
        reply("event-session-deleted", EventKind::SessionEnd, &both),
        ""
    );
}

#[test]
fn ignored_events_render_nothing() {
    let notice = ReplyAction::Notice { text: "n".into() };
    assert_eq!(
        reply("event-message-updated", EventKind::TurnEnd, &notice),
        ""
    );
}

#[test]
fn prune_target_is_the_opencode_permission_object() {
    assert_eq!(
        OpencodeAdapter::new().prune_target(),
        PruneTarget::ConfigFileKey {
            path: "opencode.json".into(),
            key: "permission".into()
        }
    );
}
