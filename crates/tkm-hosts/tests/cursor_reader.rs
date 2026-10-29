#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::PathBuf;

use tkm_core::model::{Host, RepoKey, Timestamp};
use tkm_hosts::cursor::CursorAdapter;
use tkm_hosts::{AdapterError, HostAdapter, SessionSource};

#[test]
fn reads_synthetic_transcript_into_turns_and_calls() {
    let adapter = CursorAdapter::new();
    let transcript_path = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/cursor/transcripts/c1a2b3c4-d5e6-7f8a-9b0c-1d2e3f4a5b6c.jsonl"
    ));

    let source = SessionSource {
        agent_home: PathBuf::from("/home/user/.cursor"),
        path: transcript_path,
        session_id: "c1a2b3c4-d5e6-7f8a-9b0c-1d2e3f4a5b6c".to_owned(),
        repo_key: RepoKey("repo-1".to_owned()),
        cwd: PathBuf::from("/workspace/tokenimum"),
    };

    let session = adapter.read_session(&source).unwrap();
    assert_eq!(session.host, Host::Cursor);
    assert_eq!(session.session_id, "c1a2b3c4-d5e6-7f8a-9b0c-1d2e3f4a5b6c");
    assert_eq!(session.cwd, PathBuf::from("/workspace/tokenimum"));
    assert_eq!(session.started, Timestamp::default());
    assert_eq!(session.ended, None);
    assert!(!session.partial);
    assert_eq!(session.skipped_records, 0);
    assert_eq!(session.total_records, 6);

    assert_eq!(session.turns.len(), 2);

    // Turn 0: user prompt stripped <user_query>
    let turn0 = &session.turns[0];
    assert_eq!(turn0.prompt, "Please check git status and run tests");
    assert_eq!(turn0.calls.len(), 2);

    assert_eq!(turn0.calls[0].tool, "Bash");
    assert_eq!(turn0.calls[0].args_digest, "git status");
    assert_eq!(turn0.calls[0].output_digest, "");
    assert_eq!(turn0.calls[0].exit_code, None);
    assert!(!turn0.calls[0].is_error);

    assert_eq!(turn0.calls[1].tool, "Bash");
    assert_eq!(turn0.calls[1].args_digest, "cargo test");
    assert_eq!(turn0.calls[1].output_digest, "");
    assert_eq!(turn0.calls[1].exit_code, None);
    assert!(!turn0.calls[1].is_error);

    // Turn 1: user prompt
    let turn1 = &session.turns[1];
    assert_eq!(turn1.prompt, "Great, now edit src/lib.rs");
    assert_eq!(turn1.calls.len(), 1);
    assert_eq!(turn1.calls[0].tool, "Edit");
    assert_eq!(turn1.calls[0].output_digest, "");
    assert_eq!(turn1.calls[0].exit_code, None);
    assert!(!turn1.calls[0].is_error);
}

#[test]
fn session_id_derived_from_file_stem_when_source_session_id_empty() {
    let adapter = CursorAdapter::new();
    let transcript_path = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/cursor/transcripts/c1a2b3c4-d5e6-7f8a-9b0c-1d2e3f4a5b6c.jsonl"
    ));

    let source = SessionSource {
        agent_home: PathBuf::from("/home/user/.cursor"),
        path: transcript_path,
        session_id: String::new(), // empty: should fall back to file stem
        repo_key: RepoKey("repo-1".to_owned()),
        cwd: PathBuf::from("/workspace/tokenimum"),
    };

    let session = adapter.read_session(&source).unwrap();
    assert_eq!(session.session_id, "c1a2b3c4-d5e6-7f8a-9b0c-1d2e3f4a5b6c");
}

#[test]
fn marks_session_partial_when_skipped_records_exceed_twenty_percent() {
    let adapter = CursorAdapter::new();
    let transcript_path = PathBuf::from(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/cursor/transcripts/partial-session.jsonl"
    ));

    let source = SessionSource {
        agent_home: PathBuf::from("/home/user/.cursor"),
        path: transcript_path,
        session_id: "partial-session".to_owned(),
        repo_key: RepoKey("repo-1".to_owned()),
        cwd: PathBuf::from("/workspace/tokenimum"),
    };

    let session = adapter.read_session(&source).unwrap();
    assert!(session.partial);
    assert_eq!(session.total_records, 3);
    assert_eq!(session.skipped_records, 2);
    assert_eq!(session.turns.len(), 1);
    assert_eq!(session.turns[0].prompt, "hello");
}

#[test]
fn non_existent_file_returns_io_error() {
    let adapter = CursorAdapter::new();
    let source = SessionSource {
        agent_home: PathBuf::from("/home/user/.cursor"),
        path: PathBuf::from("/no/such/transcript.jsonl"),
        session_id: "none".to_owned(),
        repo_key: RepoKey("repo-1".to_owned()),
        cwd: PathBuf::from("/workspace/tokenimum"),
    };

    assert!(matches!(
        adapter.read_session(&source),
        Err(AdapterError::Io(_))
    ));
}
