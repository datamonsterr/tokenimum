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
    /// Loads the redacted SQL dump into a fresh database file.
    fn database(&self) -> PathBuf {
        let sql = fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/opencode/session.sql"
        ))
        .unwrap();
        let path = self.0.join("opencode.db");
        let db = Connection::open(&path).unwrap();
        // The dump lists tables alphabetically, children before their parents.
        db.execute_batch("PRAGMA foreign_keys = OFF;").unwrap();
        db.execute_batch(&sql).unwrap();
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
    assert_eq!(calls[2].edits.len(), 0, "path outside the repository");
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

fn insert_part(path: &Path, id: &str, data: &str) {
    Connection::open(path)
        .unwrap()
        .execute(
            "INSERT INTO part VALUES (?1, 'msg_9002', 'ses_example1', 1791437100999, 1791437100999, ?2)",
            (id, data),
        )
        .unwrap();
}

#[test]
fn parts_with_valid_json_and_a_type_are_never_skipped() {
    let scratch = Scratch::new();
    let path = scratch.database();
    // The fixture already holds a `future-widget` part.
    let session = read(&path, "ses_example1");
    assert_eq!((session.total_records, session.skipped_records), (37, 0));
    insert_part(
        &path,
        "prt_s1",
        r#"{"type":"snapshot","snapshot":"0000000000000000000000000000000000000005"}"#,
    );
    insert_part(&path, "prt_s2", r#"{"type":"another-future-type"}"#);
    let session = read(&path, "ses_example1");
    assert_eq!((session.total_records, session.skipped_records), (39, 0));
    assert!(!session.partial);
}

#[test]
fn unreadable_or_untyped_parts_are_skipped() {
    let scratch = Scratch::new();
    let path = scratch.database();
    insert_part(&path, "prt_b1", "not json");
    insert_part(&path, "prt_b2", r#"{"text":"no type"}"#);
    insert_part(&path, "prt_b3", r#"{"type":7}"#);
    insert_part(&path, "prt_b4", "[1,2]");
    let session = read(&path, "ses_example1");
    assert_eq!((session.total_records, session.skipped_records), (41, 4));
    assert!(!session.partial);
}

#[test]
fn more_than_twenty_percent_skipped_marks_the_session_partial() {
    let scratch = Scratch::new();
    let path = scratch.database();
    for index in 0..20 {
        insert_part(&path, &format!("prt_x{index}"), "not json");
    }
    let session = read(&path, "ses_example1");
    assert_eq!((session.total_records, session.skipped_records), (57, 20));
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
fn legacy_json_tree_loads_session_messages_and_parts() {
    let scratch = Scratch::new();
    let storage = scratch.0.join("storage");
    for dir in [
        "session/project",
        "message/ses_legacy",
        "part/msg_1",
        "part/msg_2",
    ] {
        fs::create_dir_all(storage.join(dir)).unwrap();
    }
    fs::write(
        storage.join("session/project/ses_legacy.json"),
        r#"{"id":"ses_legacy","time":{"created":1,"updated":2}}"#,
    )
    .unwrap();
    fs::write(
        storage.join("message/ses_legacy/msg_1.json"),
        r#"{"id":"msg_1","role":"user","time":{"created":1}}"#,
    )
    .unwrap();
    fs::write(storage.join("message/ses_legacy/msg_2.json"), r#"{"id":"msg_2","role":"assistant","time":{"created":2},"modelID":"model","tokens":{"input":7,"output":3}}"#).unwrap();
    fs::write(
        storage.join("part/msg_1/prt_1.json"),
        r#"{"type":"text","text":"hello"}"#,
    )
    .unwrap();
    fs::write(storage.join("part/msg_2/prt_2.json"), r#"{"type":"tool","tool":"bash","state":{"status":"completed","input":{"command":"true"},"output":"ok"}}"#).unwrap();
    let session = read(&scratch.0, "ses_legacy");
    assert_eq!(session.turns.len(), 1);
    assert_eq!(session.turns[0].prompt, "hello");
    assert_eq!(session.turns[0].calls[0].tool, "bash");
    assert_eq!(session.usage.input, 7);
}

#[cfg(unix)]
#[test]
fn export_fallback_invokes_fake_opencode() {
    use std::os::unix::fs::PermissionsExt;
    let scratch = Scratch::new();
    let script = scratch.0.join("opencode");
    fs::write(&script, "#!/bin/sh\n[ \"$1\" = export ] && [ \"$2\" = ses_export ] || exit 2\nprintf '%s' '{\"info\":{\"id\":\"ses_export\",\"time\":{\"created\":1,\"updated\":2}},\"messages\":[{\"info\":{\"role\":\"user\"},\"parts\":[{\"type\":\"text\",\"text\":\"from export\"}]}]}'\n").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .arg("--exact")
        .arg("export_fallback_child")
        .arg("--nocapture")
        .env("PATH", &scratch.0)
        .env("TKM_TEST_EXPORT_DIR", &scratch.0)
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn export_fallback_child() {
    let Some(dir) = std::env::var_os("TKM_TEST_EXPORT_DIR") else {
        return;
    };
    let session = read(Path::new(&dir), "ses_export");
    assert_eq!(session.turns[0].prompt, "from export");
}
