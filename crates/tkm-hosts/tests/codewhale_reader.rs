#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::PathBuf;

use serde_json::json;
use tkm_core::model::{Host, RepoKey, Timestamp, Usage};
use tkm_hosts::codewhale::CodewhaleAdapter;
use tkm_hosts::{AdapterError, HostAdapter, SessionSource};

const FIXTURES: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/codewhale/sessions"
);

#[test]
fn synthetic_journal_session_walks_active_branch_and_excludes_off_branch() {
    let adapter = CodewhaleAdapter::new();
    let session_file = format!("{FIXTURES}/550e8400-e29b-41d4-a716-446655440000.json");
    let source = SessionSource {
        agent_home: PathBuf::from("/home/user/.codewhale"),
        path: PathBuf::from(&session_file),
        session_id: "550e8400-e29b-41d4-a716-446655440000".to_owned(),
        repo_key: RepoKey("repo-123".to_owned()),
        cwd: PathBuf::from("/home/user/project"),
    };

    let session = adapter.read_session(&source).unwrap();
    assert_eq!(session.host, Host::Deepseek);
    assert_eq!(session.session_id, "550e8400-e29b-41d4-a716-446655440000");
    assert_eq!(session.cwd, PathBuf::from("/home/user/project"));
    assert_eq!(session.repo_key, RepoKey("repo-123".to_owned()));
    assert_eq!(
        session.started,
        Timestamp(1_793_268_000_000) // 2026-10-29T10:00:00Z
    );
    assert_eq!(
        session.ended,
        Some(Timestamp(1_793_268_300_000)) // 2026-10-29T10:05:00Z
    );
    assert!(!session.partial);
    assert_eq!(session.skipped_records, 0);

    // Turns verification
    assert_eq!(session.turns.len(), 1);
    let turn = &session.turns[0];
    assert_eq!(turn.prompt, "run cargo check");
    assert_ne!(turn.prompt, "this is an abandoned branch");
    assert_eq!(turn.model.as_deref(), Some("deepseek-chat"));
    // Per-turn usage in file is zero (documented to come from live turn_end)
    assert_eq!(turn.usage, Usage::default());
    assert_eq!(session.usage, Usage::default());

    // Calls verification
    assert_eq!(turn.calls.len(), 1);
    let call = &turn.calls[0];
    assert_eq!(call.tool, "exec_shell");
    assert_eq!(call.args_digest, "cargo check");
    assert_eq!(call.output_digest, "error: could not compile");
    assert!(call.is_error);

    // None of the turns or calls should contain the off-branch text
    for t in &session.turns {
        assert!(!t.prompt.contains("abandoned branch"));
        for c in &t.calls {
            assert!(!c.args_digest.contains("abandoned branch"));
        }
    }
}

#[test]
fn directory_path_resolution() {
    let adapter = CodewhaleAdapter::new();
    let source = SessionSource {
        agent_home: PathBuf::from("/home/user/.codewhale"),
        path: PathBuf::from(FIXTURES),
        session_id: "550e8400-e29b-41d4-a716-446655440000".to_owned(),
        repo_key: RepoKey::default(),
        cwd: PathBuf::default(),
    };

    let session = adapter.read_session(&source).unwrap();
    assert_eq!(session.session_id, "550e8400-e29b-41d4-a716-446655440000");
    assert_eq!(session.turns.len(), 1);
}

#[test]
fn messages_only_variant_extracts_turns_and_calls() {
    let adapter = CodewhaleAdapter::new();
    let temp_dir = std::env::temp_dir().join("tkm-codewhale-messages-only");
    fs::create_dir_all(&temp_dir).unwrap();
    let file_path = temp_dir.join("sess_msg_only.json");

    let doc = json!({
        "schema_version": 1,
        "metadata": {
            "id": "sess_msg_only",
            "created_at": "2026-10-29T11:00:00Z",
            "updated_at": "2026-10-29T11:02:00Z",
            "workspace": "/tmp/workspace",
            "model": "deepseek-coder"
        },
        "messages": [
            {
                "role": "user",
                "content": [
                    { "type": "text", "text": "build the project" }
                ]
            },
            {
                "role": "assistant",
                "content": [
                    {
                        "type": "thinking",
                        "thinking": "I need to invoke bash with cargo build"
                    },
                    {
                        "type": "tool_use",
                        "id": "call_m1",
                        "name": "bash",
                        "input": { "command": "cargo build" }
                    }
                ]
            },
            {
                "role": "user",
                "content": [
                    {
                        "type": "tool_result",
                        "tool_use_id": "call_m1",
                        "content": "Compiling...\nFinished dev target(s)",
                        "is_error": false
                    }
                ]
            }
        ]
    });

    fs::write(&file_path, serde_json::to_vec(&doc).unwrap()).unwrap();

    let source = SessionSource {
        agent_home: PathBuf::from("/tmp"),
        path: file_path.clone(),
        session_id: "sess_msg_only".to_owned(),
        repo_key: RepoKey::default(),
        cwd: PathBuf::from("/tmp/workspace"),
    };

    let session = adapter.read_session(&source).unwrap();
    assert_eq!(session.session_id, "sess_msg_only");
    assert_eq!(session.turns.len(), 1);
    let turn = &session.turns[0];
    assert_eq!(turn.prompt, "build the project");
    assert_eq!(turn.calls.len(), 1);
    let call = &turn.calls[0];
    assert_eq!(call.tool, "bash");
    assert_eq!(call.args_digest, "cargo build");
    assert_eq!(call.output_digest, "Finished dev target(s)");
    assert!(!call.is_error);

    let _ = fs::remove_file(file_path);
    let _ = fs::remove_dir(temp_dir);
}

#[test]
fn partial_session_on_excessive_unknown_blocks() {
    let adapter = CodewhaleAdapter::new();
    let temp_dir = std::env::temp_dir().join("tkm-codewhale-partial");
    fs::create_dir_all(&temp_dir).unwrap();
    let file_path = temp_dir.join("sess_partial.json");

    // 4 blocks total: 2 text blocks and 2 unknown blocks (50% unknown > 20% threshold)
    let doc = json!({
        "schema_version": 1,
        "metadata": {
            "id": "sess_partial",
            "workspace": "/tmp"
        },
        "messages": [
            {
                "role": "user",
                "content": [
                    { "type": "text", "text": "turn 1" },
                    { "type": "future_unknown_block_1", "data": 123 },
                    { "type": "future_unknown_block_2", "data": 456 }
                ]
            },
            {
                "role": "assistant",
                "content": [
                    { "type": "text", "text": "assistant reply" }
                ]
            }
        ]
    });

    fs::write(&file_path, serde_json::to_vec(&doc).unwrap()).unwrap();

    let source = SessionSource {
        agent_home: PathBuf::from("/tmp"),
        path: file_path.clone(),
        session_id: "sess_partial".to_owned(),
        repo_key: RepoKey::default(),
        cwd: PathBuf::from("/tmp"),
    };

    let session = adapter.read_session(&source).unwrap();
    assert!(session.partial, "session should be marked partial");
    assert!(session.skipped_records >= 2);
    assert!(session.total_records >= 4);

    let _ = fs::remove_file(file_path);
    let _ = fs::remove_dir(temp_dir);
}

#[test]
fn io_and_malformed_errors() {
    let adapter = CodewhaleAdapter::new();

    // Non-existent file
    let source_not_found = SessionSource {
        agent_home: PathBuf::from("/tmp"),
        path: PathBuf::from("/nonexistent/session_file.json"),
        session_id: "missing".to_owned(),
        repo_key: RepoKey::default(),
        cwd: PathBuf::default(),
    };
    assert!(matches!(
        adapter.read_session(&source_not_found),
        Err(AdapterError::Io(_))
    ));

    // Malformed JSON
    let temp_dir = std::env::temp_dir().join("tkm-codewhale-malformed");
    fs::create_dir_all(&temp_dir).unwrap();
    let file_path = temp_dir.join("malformed.json");
    fs::write(&file_path, b"not json content").unwrap();

    let source_malformed = SessionSource {
        agent_home: PathBuf::from("/tmp"),
        path: file_path.clone(),
        session_id: "malformed".to_owned(),
        repo_key: RepoKey::default(),
        cwd: PathBuf::default(),
    };
    assert!(matches!(
        adapter.read_session(&source_malformed),
        Err(AdapterError::Malformed(_))
    ));

    let _ = fs::remove_file(file_path);
    let _ = fs::remove_dir(temp_dir);
}
