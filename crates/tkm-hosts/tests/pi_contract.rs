#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use serde_json::Value;
use std::{collections::BTreeMap, fs, path::PathBuf};
use tkm_core::model::{Call, EditKind, EditVia, FileEdit, HookEvent};
use tkm_hosts::{AdapterError, HostAdapter, PruneTarget, pi::PiAdapter};
use tkm_proto::{EventKind, Host, PROTOCOL_VERSION, ReplyAction, Request};

const SESSION: &str = "00000000-0000-4000-8000-000000000001";
const PROJECT: &str = "/home/user/project";

fn fixture(name: &str) -> Value {
    let path = format!(
        "{}/tests/fixtures/pi/hooks/{name}.json",
        env!("CARGO_MANIFEST_DIR")
    );
    serde_json::from_slice(&fs::read(path).unwrap()).unwrap()
}

fn request(name: &str, kind: EventKind) -> Request {
    let payload = fixture(name);
    Request {
        version: PROTOCOL_VERSION,
        host: Host::Pi,
        event: kind,
        host_event: payload["hook"].as_str().map(str::to_owned),
        payload,
        env: BTreeMap::new(),
        cwd: None,
    }
}

fn base(kind: EventKind) -> HookEvent {
    let mut event = HookEvent::new(Host::Pi, kind, SESSION);
    event.cwd = Some(PathBuf::from(PROJECT));
    event
}

fn map(name: &str, kind: EventKind) -> Option<HookEvent> {
    PiAdapter::new().map_event(&request(name, kind)).unwrap()
}

fn reply(name: &str, kind: EventKind, action: &ReplyAction) -> String {
    PiAdapter::new().render_reply(&request(name, kind), action)
}

#[test]
fn adapter_reports_pi() {
    assert_eq!(PiAdapter::new().host(), Host::Pi);
}

#[test]
fn session_start_maps() {
    assert_eq!(
        map("session_start", EventKind::SessionStart),
        Some(base(EventKind::SessionStart))
    );
}

#[test]
fn before_agent_start_maps_to_prompt_and_custom_runs_are_ignored() {
    let mut expected = base(EventKind::Prompt);
    expected.prompt = Some("Fix the failing test".into());
    assert_eq!(map("before_agent_start", EventKind::Prompt), Some(expected));
    assert_eq!(map("before_agent_start_custom", EventKind::Prompt), None);
}

#[test]
fn tool_result_maps_to_call() {
    let mut expected = base(EventKind::ToolDone);
    expected.call = Some(Call {
        tool: "bash".into(),
        args_digest: "ls -la".into(),
        output_digest: "b.txt".into(),
        ..Call::default()
    });
    assert_eq!(map("tool_result_bash", EventKind::ToolDone), Some(expected));
    let failed = map("tool_result_bash_error", EventKind::ToolDone)
        .unwrap()
        .call
        .unwrap();
    assert!(failed.is_error);
    assert_eq!(failed.output_digest, "Command exited with code 1");
    assert!(failed.edits.is_empty());
}

#[test]
fn write_and_edit_tools_map_to_repo_relative_edits() {
    let write = map("tool_result_write", EventKind::ToolDone)
        .unwrap()
        .call
        .unwrap();
    assert_eq!(
        write.edits,
        vec![FileEdit {
            path: "src/new.rs".into(),
            kind: EditKind::Modify,
            via: EditVia::Tool
        }]
    );
    assert_eq!(write.args_digest, r#"{"content":"x","path":"src/new.rs"}"#);
    let edit = map("tool_result_edit", EventKind::ToolDone)
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
    assert!(
        map("tool_result_edit_outside", EventKind::ToolDone)
            .unwrap()
            .call
            .unwrap()
            .edits
            .is_empty()
    );
}

#[test]
fn agent_end_is_the_turn_end_and_turn_end_is_ignored() {
    assert_eq!(
        map("agent_end", EventKind::TurnEnd),
        Some(base(EventKind::TurnEnd))
    );
    assert_eq!(map("turn_end", EventKind::TurnEnd), None);
}

#[test]
fn session_shutdown_maps_to_session_end() {
    assert_eq!(
        map("session_shutdown", EventKind::SessionEnd),
        Some(base(EventKind::SessionEnd))
    );
}

#[test]
fn every_fixture_maps_without_error() {
    for entry in fs::read_dir(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/pi/hooks"
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
        PiAdapter::new()
            .map_event(&request(&name, EventKind::Prompt))
            .unwrap();
    }
}

#[test]
fn request_cwd_is_the_fallback_when_payload_has_none() {
    let mut request = request("session_start", EventKind::SessionStart);
    request.payload.as_object_mut().unwrap().remove("cwd");
    request.cwd = Some("/home/user/other".into());
    let event = PiAdapter::new().map_event(&request).unwrap().unwrap();
    assert_eq!(event.cwd, Some(PathBuf::from("/home/user/other")));
}

#[test]
fn invalid_requests_are_rejected() {
    let adapter = PiAdapter::new();
    let good = request("before_agent_start", EventKind::Prompt);
    let mut version = good.clone();
    version.version = PROTOCOL_VERSION + 1;
    let mut host = good.clone();
    host.host = Host::Opencode;
    let mut conflict = good.clone();
    conflict.host_event = Some("tool_result".into());
    let mut no_session = good.clone();
    no_session
        .payload
        .as_object_mut()
        .unwrap()
        .remove("session_id");
    let mut not_object = good.clone();
    not_object.payload = Value::Null;
    let mut no_prompt = good;
    no_prompt.payload["event"]["prompt"] = Value::Null;
    for bad in [version, host, conflict, no_session, not_object, no_prompt] {
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
fn before_agent_start_reply_carries_context_and_notice_in_fixed_order() {
    let inject = ReplyAction::Inject { text: "ctx".into() };
    let notice = ReplyAction::Notice { text: "n".into() };
    let both = ReplyAction::InjectAndNotice {
        inject: "ctx".into(),
        notice: "n".into(),
    };
    assert_eq!(
        reply("before_agent_start", EventKind::Prompt, &inject),
        r#"{"context":"ctx"}"#
    );
    assert_eq!(
        reply("before_agent_start", EventKind::Prompt, &notice),
        r#"{"notice":"n"}"#
    );
    assert_eq!(
        reply("before_agent_start", EventKind::Prompt, &both),
        r#"{"context":"ctx","notice":"n"}"#
    );
    assert_eq!(
        reply(
            "before_agent_start",
            EventKind::Prompt,
            &ReplyAction::Nothing
        ),
        ""
    );
}

#[test]
fn context_is_capped_at_ten_thousand_chars() {
    let text = "é".repeat(12_000);
    let out = reply(
        "before_agent_start",
        EventKind::Prompt,
        &ReplyAction::Inject { text },
    );
    let value: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(value["context"].as_str().unwrap().chars().count(), 10_000);
}

#[test]
fn injection_is_only_allowed_on_before_agent_start() {
    let both = ReplyAction::InjectAndNotice {
        inject: "ctx".into(),
        notice: "n".into(),
    };
    for (name, kind) in [
        ("session_start", EventKind::SessionStart),
        ("tool_result_bash", EventKind::ToolDone),
        ("agent_end", EventKind::TurnEnd),
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
fn session_end_and_ignored_events_never_carry_a_reply() {
    let both = ReplyAction::InjectAndNotice {
        inject: "ctx".into(),
        notice: "n".into(),
    };
    assert_eq!(reply("session_shutdown", EventKind::SessionEnd, &both), "");
    assert_eq!(reply("turn_end", EventKind::TurnEnd, &both), "");
    assert_eq!(
        reply("before_agent_start_custom", EventKind::Prompt, &both),
        ""
    );
}

#[test]
fn prune_target_is_project_settings_default_tools() {
    assert_eq!(
        PiAdapter::new().prune_target(),
        PruneTarget::ConfigFileKey {
            path: ".pi/settings.json".into(),
            key: "defaultTools".into()
        }
    );
}
