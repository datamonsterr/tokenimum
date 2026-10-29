#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use rusqlite::Connection;
use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use tkm_core::model::{
    Call, EditKind, EditVia, FileEdit, RepoKey, Session, StopReason, Timestamp, ToolKind, Usage,
};
use tkm_hosts::{AdapterError, HostAdapter, SessionSource, opencode::OpencodeAdapter};

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "tkm-opencode-reader-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    /// Loads the redacted SQL dump into a fresh SQLite file.
    fn database(&self) -> PathBuf {
        let sql = fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/opencode/session.sql"
        ))
        .unwrap();
        let path = self.0.join("opencode.db");
        Connection::open(&path)
            .unwrap()
            .execute_batch(&sql)
            .unwrap();
        path
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn source(path: &Path, session: &str) -> SessionSource {
    SessionSource {
        agent_home: "/synthetic/agent".into(),
        path: path.to_path_buf(),
        session_id: session.into(),
        repo_key: RepoKey("repo".into()),
        cwd: "/home/user/project".into(),
    }
}

fn read(path: &Path, session: &str) -> Session {
    OpencodeAdapter::new()
        .read_session(&source(path, session))
        .unwrap()
}

fn usage(input: u64, output: u64, cache_read: u64, cache_write: u64) -> Usage {
    Usage {
        input,
        output,
        cache_read,
        cache_write,
    }
}

#[test]
fn first_session_has_two_turns_with_calls_and_usage() {
    let scratch = Scratch::new();
    let session = read(&scratch.database(), "ses_example1");
    assert_eq!(session.session_id, "ses_example1");
    assert_eq!(session.repo_key, RepoKey("repo".into()));
    assert_eq!(session.cwd, PathBuf::from("/home/user/project"));
    assert_eq!(session.started, Timestamp(1_791_436_988_255));
    assert_eq!(session.ended, Some(Timestamp(1_791_437_100_900)));
    assert_eq!(session.turns.len(), 2);
    assert_eq!(session.turns[0].prompt, "Synthetic prompt.");
    assert_eq!(session.turns[1].prompt, "Synthetic follow-up prompt.");
    assert_eq!(session.turns[0].calls.len(), 9);
    assert_eq!(session.turns[0].calls[0].tool, "bash");
    assert_eq!(session.turns[0].calls[0].args_digest, "synthetic-command");
    assert_eq!(session.turns[0].model.as_deref(), Some("example-model-a"));
    // Input excludes cache; reasoning tokens are folded into output.
    assert_eq!(session.turns[0].usage, usage(38_995, 817, 73_728, 0));
    assert_eq!(session.turns[1].usage, usage(100, 25, 300, 7));
    assert_eq!(session.usage, usage(39_095, 842, 74_028, 7));
    assert_eq!(session.usage_by_model.len(), 1);
    assert_eq!(session.usage_by_model["example-model-a"], session.usage);
    assert_eq!(
        session.turns[1].usage_by_model["example-model-a"],
        session.turns[1].usage
    );
    assert_eq!(session.turns[1].stopped_by, StopReason::OwnStop);
}

#[test]
fn second_turn_calls_cover_edits_errors_and_exit_codes() {
    let scratch = Scratch::new();
    let calls = read(&scratch.database(), "ses_example1")
        .turns
        .remove(1)
        .calls;
    assert_eq!(calls.len(), 6);
    assert_eq!(
        calls[0],
        Call {
            tool: "write".into(),
            args_digest: r#"{"content":"synthetic","filePath":"/home/user/project/src/new.rs"}"#
                .into(),
            output_digest: "Wrote file successfully.".into(),
            exit_code: None,
            is_error: false,
            duration_ms: Some(50),
            edits: vec![FileEdit {
                path: "src/new.rs".into(),
                kind: EditKind::Create,
                via: EditVia::Tool
            }],
        }
    );
    assert_eq!(
        calls[1].edits,
        vec![FileEdit {
            path: "src/lib.rs".into(),
            kind: EditKind::Modify,
            via: EditVia::Tool
        }]
    );
    assert_eq!(calls[1].duration_ms, Some(60));
    assert!(calls[2].edits.is_empty(), "path outside the repository");
    assert_eq!(
        calls[3],
        Call {
            tool: "bash".into(),
            args_digest: "synthetic-failing-command".into(),
            output_digest: "Error: command failed".into(),
            exit_code: Some(2),
            is_error: true,
            duration_ms: Some(50),
            edits: vec![],
        }
    );
    assert_eq!((calls[4].exit_code, calls[4].is_error), (Some(1), false));
    assert_eq!(calls[4].output_digest, "Exit status 1");
    assert_eq!(calls[5].tool, "example_server_tool");
}

#[test]
fn synthetic_text_parts_are_not_prompts() {
    let scratch = Scratch::new();
    let session = read(&scratch.database(), "ses_example1");
    assert!(
        session
            .turns
            .iter()
            .all(|turn| !turn.prompt.contains("injected"))
    );
}

#[test]
fn tool_catalog_lists_tools_in_first_use_order() {
    let scratch = Scratch::new();
    let catalog = read(&scratch.database(), "ses_example1").tool_catalog;
    let names: Vec<_> = catalog
        .iter()
        .map(|tool| (tool.name.as_str(), tool.kind))
        .collect();
    assert_eq!(
        names,
        vec![
            ("bash", ToolKind::Builtin),
            ("read", ToolKind::Builtin),
            ("write", ToolKind::Builtin),
            ("edit", ToolKind::Builtin),
            ("example_server_tool", ToolKind::Mcp),
        ]
    );
}

#[test]
fn only_the_requested_session_is_read() {
    let scratch = Scratch::new();
    let session = read(&scratch.database(), "ses_example2");
    assert_eq!(session.turns.len(), 1);
    assert_eq!(session.turns[0].calls.len(), 6);
    assert_eq!(session.usage, usage(38_083, 2_048, 179_750, 0));
    assert_eq!(session.started, Timestamp(1_792_131_725_177));
    assert_eq!(
        (
            session.total_records,
            session.skipped_records,
            session.partial
        ),
        (34, 0, false)
    );
    assert_eq!(session.turns[0].model.as_deref(), Some("example-model-b"));
}

#[test]
fn unknown_part_types_are_counted_as_skipped_without_marking_partial() {
    let scratch = Scratch::new();
    let session = read(&scratch.database(), "ses_example1");
    assert_eq!((session.total_records, session.skipped_records), (37, 1));
    assert!(!session.partial);
}

#[test]
fn more_than_twenty_percent_skipped_marks_the_session_partial() {
    let scratch = Scratch::new();
    let path = scratch.database();
    let db = Connection::open(&path).unwrap();
    for index in 0..20 {
        db.execute(
            "INSERT INTO part VALUES (?1, 'msg_9002', 'ses_example1', 1791437100999, 1791437100999, ?2)",
            (format!("prt_x{index}"), if index % 2 == 0 { r#"{"type":"future-widget"}"# } else { "not json" }),
        )
        .unwrap();
    }
    drop(db);
    let session = read(&path, "ses_example1");
    assert_eq!((session.total_records, session.skipped_records), (57, 21));
    assert!(session.partial);
}

#[test]
fn reading_never_modifies_the_database() {
    let scratch = Scratch::new();
    let path = scratch.database();
    let before = fs::read(&path).unwrap();
    let listing = |dir: &Path| {
        let mut names: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        names.sort();
        names
    };
    let names = listing(&scratch.0);
    read(&path, "ses_example1");
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(listing(&scratch.0), names);
}

#[test]
fn a_read_only_connection_is_enough() {
    let scratch = Scratch::new();
    let path = scratch.database();
    let mut permissions = fs::metadata(&path).unwrap().permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&path, permissions).unwrap();
    assert_eq!(read(&path, "ses_example1").turns.len(), 2);
}

#[test]
fn unknown_session_is_an_error() {
    let scratch = Scratch::new();
    let error = OpencodeAdapter::new()
        .read_session(&source(&scratch.database(), "ses_missing"))
        .unwrap_err();
    assert!(
        matches!(error, AdapterError::InvalidPayload { .. }),
        "{error:?}"
    );
}

#[test]
fn missing_database_is_an_error_and_is_not_created() {
    let scratch = Scratch::new();
    let path = scratch.0.join("absent.db");
    assert!(
        OpencodeAdapter::new()
            .read_session(&source(&path, "ses_example1"))
            .is_err()
    );
    assert!(!path.exists());
}

#[test]
fn a_directory_source_uses_opencode_db_inside_it() {
    let scratch = Scratch::new();
    scratch.database();
    assert_eq!(read(&scratch.0, "ses_example1").turns.len(), 2);
}

#[test]
fn legacy_storage_directories_are_unsupported() {
    let scratch = Scratch::new();
    fs::create_dir_all(scratch.0.join("storage/session")).unwrap();
    let error = OpencodeAdapter::new()
        .read_session(&source(&scratch.0, "ses_example1"))
        .unwrap_err();
    assert!(
        matches!(error, AdapterError::Unsupported { .. }),
        "{error:?}"
    );
}
