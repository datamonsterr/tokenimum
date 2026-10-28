#![forbid(unsafe_code)]

use serde::{Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use tkm_core::model::{
    Call, EditKind, EditVia, FileEdit, HookEvent, Host, RepoKey, Session, StopReason, Timestamp,
    ToolDef, ToolKind, Turn, Usage,
};

fn round_trip<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(
    expected: &T,
) -> Result<(), serde_json::Error> {
    let bytes = serde_json::to_vec(expected)?;
    let actual: T = serde_json::from_slice(&bytes)?;
    assert_eq!(&actual, expected);
    Ok(())
}

fn call() -> Call {
    Call {
        tool: "shell".into(),
        args_digest: "patch source".into(),
        output_digest: "synthetic success".into(),
        exit_code: Some(0),
        is_error: false,
        duration_ms: Some(12),
        edits: vec![FileEdit {
            path: "src/lib.rs".into(),
            kind: EditKind::Modify,
            via: EditVia::Shell,
        }],
    }
}

#[test]
fn session_model_serde_round_trip_preserves_normalized_records() {
    let session = Session {
        host: Host::Claude,
        agent_home: "/synthetic/agent".into(),
        session_id: "synthetic-session".into(),
        repo_key: RepoKey("synthetic-origin".into()),
        cwd: "/synthetic/repo".into(),
        started: Timestamp(1_000),
        ended: Some(Timestamp(2_000)),
        turns: vec![Turn {
            prompt: "repair fixture".into(),
            calls: vec![call()],
            stopped_by: StopReason::OwnStop,
            usage: Usage {
                input: 10,
                output: 20,
                cache_read: 30,
                cache_write: 40,
            },
        }],
        tool_catalog: vec![
            ToolDef {
                name: "shell".into(),
                kind: ToolKind::Builtin,
                server: None,
            },
            ToolDef {
                name: "mcp__synthetic__lookup".into(),
                kind: ToolKind::Mcp,
                server: Some("synthetic".into()),
            },
        ],
        usage: Usage {
            input: 10,
            output: 20,
            cache_read: 30,
            cache_write: 40,
        },
        partial: true,
        skipped_records: 3,
        total_records: 10,
    };
    round_trip(&session).unwrap();
}

#[test]
fn live_hook_event_serde_round_trip_preserves_daemon_inputs() {
    let event = HookEvent {
        host: Host::Deepseek,
        kind: tkm_proto::EventKind::ToolBefore,
        session_id: "synthetic-session".into(),
        turn_id: Some("synthetic-turn".into()),
        repo_key: Some(RepoKey("synthetic-origin".into())),
        cwd: Some("/synthetic/repo".into()),
        timestamp: Some(Timestamp(1_000)),
        prompt: Some("repair fixture".into()),
        call: Some(call()),
        usage: Usage {
            input: 1,
            ..Usage::default()
        },
        stopped_by: Some(StopReason::Redirected),
        tool_catalog: vec![ToolDef {
            name: "shell".into(),
            kind: ToolKind::Builtin,
            server: None,
        }],
    };
    round_trip(&event).unwrap();
}

#[test]
fn usage_add_saturates_every_token_class() {
    let near_max = Usage {
        input: u64::MAX,
        output: u64::MAX - 1,
        cache_read: u64::MAX - 2,
        cache_write: u64::MAX - 3,
    };
    let increment = Usage {
        input: 4,
        output: 4,
        cache_read: 4,
        cache_write: 4,
    };
    let expected = Usage {
        input: u64::MAX,
        output: u64::MAX,
        cache_read: u64::MAX,
        cache_write: u64::MAX,
    };
    assert_eq!(near_max.add(increment), expected);
    assert_eq!(near_max + increment, expected);
}

#[test]
fn usage_add_preserves_independent_counts_and_zero_identity() {
    let first = Usage {
        input: 1,
        output: 2,
        cache_read: 3,
        cache_write: 4,
    };
    let second = Usage {
        input: 10,
        output: 20,
        cache_read: 30,
        cache_write: 40,
    };
    assert_eq!(
        first.add(second),
        Usage {
            input: 11,
            output: 22,
            cache_read: 33,
            cache_write: 44
        }
    );
    assert_eq!(first.add(Usage::default()), first);
    assert_eq!(Usage::default().add(first), first);
}

#[test]
fn identity_newtypes_have_transparent_json_and_preserve_negative_timestamp() {
    assert_eq!(
        serde_json::to_vec(&RepoKey("origin".into())).unwrap(),
        b"\"origin\""
    );
    assert_eq!(serde_json::to_vec(&Timestamp(-1)).unwrap(), b"-1");
    round_trip(&Timestamp(i64::MIN)).unwrap();
    round_trip(&Timestamp(i64::MAX)).unwrap();
}

fn json_value(value: &impl Serialize) -> Result<Value, serde_json::Error> {
    serde_json::from_slice(&serde_json::to_vec(value)?)
}

#[test]
fn extensible_model_enums_use_stable_snake_case_names() {
    for (reason, name) in [
        (StopReason::OwnStop, "own_stop"),
        (StopReason::Escalated, "escalated"),
        (StopReason::Redirected, "redirected"),
        (StopReason::Interrupted, "interrupted"),
        (StopReason::Error, "error"),
        (StopReason::Unknown, "unknown"),
    ] {
        assert_eq!(json_value(&reason).unwrap(), json!(name));
        round_trip(&reason).unwrap();
    }
    for (kind, name) in [
        (EditKind::Create, "create"),
        (EditKind::Modify, "modify"),
        (EditKind::Delete, "delete"),
    ] {
        assert_eq!(json_value(&kind).unwrap(), json!(name));
        round_trip(&kind).unwrap();
    }
    for (via, name) in [(EditVia::Tool, "tool"), (EditVia::Shell, "shell")] {
        assert_eq!(json_value(&via).unwrap(), json!(name));
        round_trip(&via).unwrap();
    }
    for (kind, name) in [
        (ToolKind::Builtin, "builtin"),
        (ToolKind::Mcp, "mcp"),
        (ToolKind::Skill, "skill"),
        (ToolKind::Other, "other"),
    ] {
        assert_eq!(json_value(&kind).unwrap(), json!(name));
        round_trip(&kind).unwrap();
    }
}

#[test]
fn model_defaults_and_constructors_keep_unknown_metadata_explicit() {
    let session = Session::new(Host::Pi);
    assert_eq!(session.host, Host::Pi);
    assert_eq!(session.started, Timestamp::default());
    assert_eq!(session.ended, None);
    assert!(session.turns.is_empty());
    assert!(!session.partial);
    assert_eq!(session.total_records, 0);
    assert_eq!(Turn::default().stopped_by, StopReason::Unknown);
    assert_eq!(Call::default().exit_code, None);
    assert_eq!(Call::default().duration_ms, None);
    assert_eq!(ToolDef::default().kind, ToolKind::Other);
    let event = HookEvent::new(Host::Pi, tkm_proto::EventKind::Prompt, "synthetic-session");
    assert_eq!(event.session_id, "synthetic-session");
    assert_eq!(event.prompt, None);
    assert_eq!(event.call, None);
}
