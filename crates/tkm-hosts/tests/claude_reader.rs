#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use serde_json::json;
use std::{
    fs,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};
use tkm_core::model::{EditKind, RepoKey, StopReason, Timestamp, ToolKind, Usage};
use tkm_hosts::{HostAdapter, SessionSource, claude::ClaudeAdapter};

fn source(path: PathBuf) -> SessionSource {
    SessionSource {
        path,
        cwd: "~/dev/example".into(),
        session_id: "s".into(),
        repo_key: RepoKey("repo".into()),
        agent_home: "/synthetic/agent".into(),
    }
}
fn temporary_log(text: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let p = std::env::temp_dir().join(format!(
        "tkm-claude-reader-{}-{}.jsonl",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&p, text).unwrap();
    p
}

#[test]
fn fixture_reader_recovers_two_turns_calls_catalog_and_deduplicated_usage() {
    let src = source(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/claude/sessions/e928a2cf-e31b-4154-805b-919991d042ea.jsonl"
        )
        .into(),
    );
    let session = ClaudeAdapter::new().read_session(&src).unwrap();
    assert_eq!(session.turns.len(), 2);
    assert_eq!(
        session.turns[0].prompt,
        "Run the bash command 'cat a.txt', then run the bash command 'false'. Then reply with the single word done."
    );
    assert_eq!(
        session.turns[1].prompt,
        "Use the Write tool to create b.txt containing x. Then use the Edit tool to replace hello with bye in a.txt. Reply done."
    );
    assert_eq!(
        session.turns[0]
            .calls
            .iter()
            .map(|c| c.tool.as_str())
            .collect::<Vec<_>>(),
        ["Bash", "Bash"]
    );
    assert_eq!(
        session.turns[1]
            .calls
            .iter()
            .map(|c| c.tool.as_str())
            .collect::<Vec<_>>(),
        ["Write", "Read", "Edit"]
    );
    let calls = &session.turns[0].calls;
    assert!(!calls[0].is_error);
    assert_eq!(calls[0].args_digest, "cat a.txt");
    assert_eq!(calls[0].output_digest, "hello");
    assert_eq!(calls[0].exit_code, None);
    assert!(calls[1].is_error);
    assert_eq!(calls[1].exit_code, Some(1));
    assert_eq!(calls[1].output_digest, "Exit code 1");
    assert_eq!(session.turns[1].calls[0].edits[0].kind, EditKind::Create);
    assert_eq!(
        session.turns[1].calls[0].edits[0].path,
        PathBuf::from("b.txt")
    );
    assert_eq!(session.turns[1].calls[2].edits[0].kind, EditKind::Modify);
    assert_eq!(
        session.usage,
        Usage {
            input: 10,
            output: 903,
            cache_read: 182_364,
            cache_write: 19_529
        }
    );
    assert_eq!(session.usage_by_model["claude-haiku-5-5"], session.usage);
    assert!(
        session
            .turns
            .iter()
            .all(|t| t.model.as_deref() == Some("claude-haiku-5-5")
                && t.stopped_by == StopReason::OwnStop)
    );
    assert_eq!(
        session.turns[0].usage + session.turns[1].usage,
        session.usage
    );
    assert!(session.tool_catalog.iter().any(|t| t.name == "Bash"));
    assert!(
        session
            .tool_catalog
            .iter()
            .any(|t| t.kind == ToolKind::Mcp && t.server.as_deref() == Some("headroom"))
    );
    assert_eq!(session.started, Timestamp(1_793_239_172_310));
    assert_eq!(session.ended, Some(Timestamp(1_793_239_309_413)));
    assert_eq!(session.skipped_records, 0);
    assert_eq!(session.total_records, 82);
    assert!(!session.partial);
}

#[test]
fn garbage_threshold_and_truncated_last_record_are_tolerated() {
    let path =
        temporary_log("{\"type\":\"system\"}\nunknown\n{\"type\":\"new-record\"}\n{\"type\":");
    let session = ClaudeAdapter::new()
        .read_session(&source(path.clone()))
        .unwrap();
    assert_eq!(session.total_records, 4);
    assert_eq!(session.skipped_records, 3);
    assert!(session.partial);
    fs::remove_file(path).unwrap();
    let path = temporary_log("{\"type\":\"system\"}\n".repeat(4).as_str());
    fs::write(
        &path,
        format!(
            "{}{{\"type\":\"new\"}}\n",
            fs::read_to_string(&path).unwrap()
        ),
    )
    .unwrap();
    let s = ClaudeAdapter::new()
        .read_session(&source(path.clone()))
        .unwrap();
    assert!(!s.partial);
    fs::remove_file(path).unwrap();
}

#[test]
fn reader_ignores_meta_commands_matches_ids_and_marks_interrupts() {
    let records = [
        json!({"type":"user","isMeta":true,"message":{"content":"meta"}}),
        json!({"type":"user","message":{"content":"<command-name>/help</command-name>"}}),
        json!({"type":"user","timestamp":"1970-01-01T01:00:00.125+01:00","message":{"content":[{"type":"text","text":"actual prompt"}]}}),
        json!({"type":"assistant","message":{"id":"m","model":"model","content":[{"type":"tool_use","id":"one","name":"Bash","input":{"command":"false"}}],"usage":{"input_tokens":1,"output_tokens":2}}}),
        json!({"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"one","is_error":true,"content":"Exit code 2"}]}}),
        json!({"type":"user","message":{"content":"[Request interrupted by user]"}}),
    ];
    let text = records.iter().map(|v| format!("{v}\n")).collect::<String>();
    let path = temporary_log(&text);
    let s = ClaudeAdapter::new()
        .read_session(&source(path.clone()))
        .unwrap();
    assert_eq!(s.turns.len(), 1);
    assert_eq!(s.turns[0].prompt, "actual prompt");
    assert_eq!(s.turns[0].stopped_by, StopReason::Interrupted);
    assert_eq!(s.turns[0].calls[0].exit_code, Some(2));
    assert_eq!(s.started, Timestamp(125));
    fs::remove_file(path).unwrap();
}
