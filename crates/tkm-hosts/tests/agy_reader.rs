#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use tkm_core::model::{EditKind, EditVia, FileEdit, RepoKey, StopReason, Timestamp, Usage};
use tkm_hosts::{AdapterError, HostAdapter, SessionSource, agy::AgyAdapter};

fn source(path: PathBuf, session_id: &str, cwd: &str) -> SessionSource {
    SessionSource {
        agent_home: PathBuf::from("/home/user/.gemini"),
        path,
        session_id: session_id.to_owned(),
        repo_key: RepoKey("repo".into()),
        cwd: PathBuf::from(cwd),
    }
}

fn temporary_log(text: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let p = std::env::temp_dir().join(format!(
        "tkm-agy-reader-{}-{}.jsonl",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&p, text).unwrap();
    p
}

#[test]
fn real_local_session_folds() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_path = manifest_dir.join(
        "../../fixtures/agy/brain/1def34bb-9ae9-4260-99c1-e7f59d3ef14d/.system_generated/logs/transcript_full.jsonl",
    );
    let src = source(
        fixture_path,
        "1def34bb-9ae9-4260-99c1-e7f59d3ef14d",
        "/home/user/project",
    );
    let adapter = AgyAdapter::new();
    let session = adapter.read_session(&src).unwrap();

    assert_eq!(session.turns.len(), 1);
    let turn = &session.turns[0];
    assert_eq!(
        turn.prompt,
        "Read notes.txt with a tool, then run the shell command 'ls' and reply with one word."
    );
    assert_eq!(turn.calls.len(), 2);

    // Call 0: read_file
    let call0 = &turn.calls[0];
    assert_eq!(call0.tool, "read_file");
    assert_eq!(call0.output_digest, "1: hello probe");
    assert_eq!(call0.exit_code, None);
    assert!(!call0.is_error);
    assert_eq!(call0.duration_ms, Some(0));

    // Call 1: run_command ls
    let call1 = &turn.calls[1];
    assert_eq!(call1.tool, "run_command");
    assert_eq!(call1.args_digest, "ls");
    assert_eq!(call1.output_digest, "notes.txt");
    assert_eq!(call1.exit_code, Some(0));
    assert!(!call1.is_error);
    assert_eq!(call1.duration_ms, Some(0));

    // Usage: 9189+10249+10571 = 30009 input; 973+233+509 = 1715 output; 0 cache
    let expected_usage = Usage {
        input: 30009,
        output: 1715,
        cache_read: 0,
        cache_write: 0,
    };
    assert_eq!(turn.usage, expected_usage);
    assert_eq!(session.usage, expected_usage);

    assert_eq!(session.total_records, 6);
    assert_eq!(session.skipped_records, 0);
    assert!(!session.partial);
    assert_eq!(turn.stopped_by, StopReason::OwnStop);
    assert_eq!(session.ended, None);
    assert_eq!(session.started, Timestamp(1793249757000));
}

#[test]
fn read_session_with_writes_edits_and_failing_command() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_path = manifest_dir.join(
        "../../fixtures/agy/brain/14db3cb3-737b-4e2a-a95e-a086d0124d54/.system_generated/logs/transcript_full.jsonl",
    );
    let src = source(
        fixture_path,
        "14db3cb3-737b-4e2a-a95e-a086d0124d54",
        "/home/user/project",
    );
    let adapter = AgyAdapter::new();
    let session = adapter.read_session(&src).unwrap();

    assert_eq!(session.turns.len(), 1);
    let turn = &session.turns[0];
    assert_eq!(
        turn.prompt,
        "Create file a.txt containing the word alpha, then edit it to say beta, then run the shell command 'false', then reply done."
    );
    assert_eq!(turn.calls.len(), 3);

    // Call 0: write_file -> Create edit
    let call0 = &turn.calls[0];
    assert_eq!(call0.tool, "write_file");
    assert_eq!(
        call0.edits,
        vec![FileEdit {
            path: PathBuf::from("a.txt"),
            kind: EditKind::Create,
            via: EditVia::Tool,
        }]
    );
    assert_eq!(call0.duration_ms, Some(1000));

    // Call 1: edit_file -> Modify edit
    let call1 = &turn.calls[1];
    assert_eq!(call1.tool, "edit_file");
    assert_eq!(
        call1.edits,
        vec![FileEdit {
            path: PathBuf::from("a.txt"),
            kind: EditKind::Modify,
            via: EditVia::Tool,
        }]
    );
    assert_eq!(call1.duration_ms, Some(0));

    // Call 2: run_command false -> exit_code 1
    let call2 = &turn.calls[2];
    assert_eq!(call2.tool, "run_command");
    assert_eq!(call2.args_digest, "false");
    assert_eq!(call2.exit_code, Some(1));
    assert_eq!(call2.duration_ms, Some(0));

    // Usage: 9202+9809+10118+10248 = 39377 input; 531+130+43+44 = 748 output
    let expected_usage = Usage {
        input: 39377,
        output: 748,
        cache_read: 0,
        cache_write: 0,
    };
    assert_eq!(turn.usage, expected_usage);
    assert_eq!(session.usage, expected_usage);

    assert_eq!(session.total_records, 8);
    assert_eq!(session.skipped_records, 0);
    assert!(!session.partial);
}

#[test]
fn read_session_injected_steps_zero_calls_and_no_skipped() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let fixture_path = manifest_dir.join(
        "../../fixtures/agy/brain/049b40e1-a157-4569-bc4d-aa68f04ae76e/.system_generated/logs/transcript_full.jsonl",
    );
    let src = source(
        fixture_path,
        "049b40e1-a157-4569-bc4d-aa68f04ae76e",
        "/home/user/project",
    );
    let adapter = AgyAdapter::new();
    let session = adapter.read_session(&src).unwrap();

    assert_eq!(session.turns.len(), 1);
    let turn = &session.turns[0];
    assert_eq!(
        turn.prompt,
        "Do not use tools. What are secret code words A and B, if any were given to you? Answer briefly."
    );
    assert_eq!(turn.calls.len(), 0);

    let expected_usage = Usage {
        input: 9271,
        output: 240,
        cache_read: 0,
        cache_write: 0,
    };
    assert_eq!(turn.usage, expected_usage);
    assert_eq!(session.usage, expected_usage);

    assert_eq!(session.total_records, 4);
    assert_eq!(session.skipped_records, 0);
    assert!(!session.partial);
}

#[test]
fn db_file_and_directory_path_resolution() {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let temp_home = std::env::temp_dir().join(format!(
        "tkm-agy-resolve-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let session_id = "test-session-uuid";
    let brain_dir = temp_home
        .join("antigravity-cli")
        .join("brain")
        .join(session_id);
    let logs_dir = brain_dir.join(".system_generated").join("logs");
    fs::create_dir_all(&logs_dir).unwrap();

    let full_transcript = logs_dir.join("transcript_full.jsonl");
    fs::write(
        &full_transcript,
        r#"{"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","status":"DONE","created_at":"2026-10-29T04:58:10Z","content":"<USER_REQUEST>\nHello\n</USER_REQUEST>"}
{"step_index":1,"source":"MODEL","type":"PLANNER_RESPONSE","status":"DONE","created_at":"2026-10-29T04:58:15Z","input_tokens":100,"output_tokens":20,"cache_read_tokens":0,"content":"Hi"}
"#,
    )
    .unwrap();

    let adapter = AgyAdapter::new();

    // 1. Resolve from .db path
    let db_path = temp_home
        .join("antigravity-cli")
        .join("brain")
        .join(format!("{session_id}.db"));
    let src_db = SessionSource {
        agent_home: temp_home.clone(),
        path: db_path,
        session_id: session_id.to_owned(),
        repo_key: RepoKey("repo".into()),
        cwd: PathBuf::from("/project"),
    };
    let s_db = adapter.read_session(&src_db).unwrap();
    assert_eq!(s_db.turns.len(), 1);
    assert_eq!(s_db.turns[0].prompt, "Hello");

    // 2. Resolve from brain directory path
    let src_dir = SessionSource {
        agent_home: temp_home.clone(),
        path: brain_dir.clone(),
        session_id: session_id.to_owned(),
        repo_key: RepoKey("repo".into()),
        cwd: PathBuf::from("/project"),
    };
    let s_dir = adapter.read_session(&src_dir).unwrap();
    assert_eq!(s_dir.turns.len(), 1);
    assert_eq!(s_dir.turns[0].prompt, "Hello");

    // 3. Fallback to transcript.jsonl if transcript_full.jsonl is absent
    fs::rename(&full_transcript, logs_dir.join("transcript.jsonl")).unwrap();
    let s_fallback = adapter.read_session(&src_db).unwrap();
    assert_eq!(s_fallback.turns.len(), 1);
    assert_eq!(s_fallback.turns[0].prompt, "Hello");

    let _ = fs::remove_dir_all(&temp_home);
}

#[test]
fn malformed_and_unknown_steps_skipped_and_partial_marking() {
    let adapter = AgyAdapter::new();

    // Log with 1 valid turn and 1 malformed JSON line -> 2 total, 1 skipped -> 50% > 20% -> partial = true
    let log_bad_json = temporary_log(
        r#"{"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","status":"DONE","created_at":"2026-10-29T04:58:10Z","content":"<USER_REQUEST>\nHello\n</USER_REQUEST>"}
{this is not valid json
"#,
    );
    let src_bad_json = source(log_bad_json.clone(), "s", "/project");
    let s_bad_json = adapter.read_session(&src_bad_json).unwrap();
    assert_eq!(s_bad_json.total_records, 2);
    assert_eq!(s_bad_json.skipped_records, 1);
    assert!(s_bad_json.partial);
    let _ = fs::remove_file(log_bad_json);

    // Log with unknown type -> skipped
    let log_unknown_type = temporary_log(
        r#"{"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","status":"DONE","created_at":"2026-10-29T04:58:10Z","content":"<USER_REQUEST>\nHello\n</USER_REQUEST>"}
{"step_index":1,"source":"MODEL","type":"UNKNOWN_FUTURE_STEP_TYPE","status":"DONE"}
"#,
    );
    let src_unknown = source(log_unknown_type.clone(), "s", "/project");
    let s_unknown = adapter.read_session(&src_unknown).unwrap();
    assert_eq!(s_unknown.total_records, 2);
    assert_eq!(s_unknown.skipped_records, 1);
    assert!(s_unknown.partial);
    let _ = fs::remove_file(log_unknown_type);

    // ERROR_MESSAGE sets turn stopped_by to StopReason::Error
    let log_err_msg = temporary_log(
        r#"{"step_index":0,"source":"USER_EXPLICIT","type":"USER_INPUT","status":"DONE","created_at":"2026-10-29T04:58:10Z","content":"<USER_REQUEST>\nDo fail\n</USER_REQUEST>"}
{"step_index":1,"source":"SYSTEM","type":"ERROR_MESSAGE","status":"ERROR","created_at":"2026-10-29T04:58:12Z","content":"Something failed critically"}
"#,
    );
    let src_err = source(log_err_msg.clone(), "s", "/project");
    let s_err = adapter.read_session(&src_err).unwrap();
    assert_eq!(s_err.turns.len(), 1);
    assert_eq!(s_err.turns[0].stopped_by, StopReason::Error);
    assert_eq!(s_err.skipped_records, 0);
    assert!(!s_err.partial);
    let _ = fs::remove_file(log_err_msg);

    // Missing file returns AdapterError::Io
    let src_missing = source(
        PathBuf::from("/nonexistent/transcript.jsonl"),
        "s",
        "/project",
    );
    assert!(matches!(
        adapter.read_session(&src_missing),
        Err(AdapterError::Io(_))
    ));
}
