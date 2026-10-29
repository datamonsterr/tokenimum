#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use tkm_core::model::{
    Call, EditKind, EditVia, FileEdit, RepoKey, Session, StopReason, Timestamp, ToolKind, Usage,
};
use tkm_hosts::{AdapterError, HostAdapter, SessionSource, pi::PiAdapter};

fn fixture() -> PathBuf {
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/pi/session.jsonl"
    )
    .into()
}

fn source(path: &Path) -> SessionSource {
    SessionSource {
        agent_home: "/synthetic/agent".into(),
        path: path.to_path_buf(),
        session_id: "00000000-0000-4000-8000-000000000001".into(),
        repo_key: RepoKey("repo".into()),
        cwd: "/home/user/project".into(),
    }
}

fn read(path: &Path) -> Session {
    PiAdapter::new().read_session(&source(path)).unwrap()
}

fn temporary(text: &str) -> PathBuf {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "tkm-pi-reader-{}-{}.jsonl",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::write(&path, text).unwrap();
    path
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
fn active_branch_yields_two_turns_and_skips_the_abandoned_one() {
    let session = read(&fixture());
    assert_eq!(session.turns.len(), 2);
    assert_eq!(session.turns[0].prompt, "List the files");
    assert_eq!(session.turns[1].prompt, "Write a file\nnow");
    assert!(
        session
            .turns
            .iter()
            .all(|turn| !turn.prompt.contains("Abandoned"))
    );
}

#[test]
fn identity_and_times_come_from_source_and_entries() {
    let session = read(&fixture());
    assert_eq!(session.session_id, "00000000-0000-4000-8000-000000000001");
    assert_eq!(session.repo_key, RepoKey("repo".into()));
    assert_eq!(session.cwd, PathBuf::from("/home/user/project"));
    assert_eq!(session.started, Timestamp(1_792_490_400_000));
    assert_eq!(session.ended, Some(Timestamp(1_792_490_586_000)));
}

#[test]
fn usage_sums_active_assistant_messages_and_usage_entries() {
    let session = read(&fixture());
    assert_eq!(session.turns[0].usage, usage(220, 50, 200, 10));
    assert_eq!(session.turns[1].usage, usage(650, 55, 1_500, 0));
    // Session total adds the standalone `usage` entry (cache warming) on the branch.
    assert_eq!(session.usage, usage(870, 105, 51_700, 10));
    assert_eq!(session.usage_by_model["example-model-a"], session.usage);
    assert_eq!(session.turns[0].model.as_deref(), Some("example-model-a"));
    assert_eq!(session.turns[1].stopped_by, StopReason::OwnStop);
}

#[test]
fn tool_calls_pair_with_their_results() {
    let session = read(&fixture());
    assert_eq!(
        session.turns[0].calls,
        vec![Call {
            tool: "bash".into(),
            args_digest: "ls".into(),
            output_digest: "a.txt".into(),
            ..Call::default()
        }]
    );
    let calls = &session.turns[1].calls;
    assert_eq!(calls.len(), 2);
    assert_eq!(calls[0].tool, "write");
    assert_eq!(calls[0].output_digest, "ok");
    assert_eq!(
        calls[0].edits,
        vec![FileEdit {
            path: "src/new.rs".into(),
            kind: EditKind::Modify,
            via: EditVia::Tool
        }]
    );
    assert!(calls[1].is_error);
    assert_eq!(calls[1].output_digest, "Error: no match");
    assert_eq!(calls[1].edits.len(), 0);
}

#[test]
fn tool_catalog_comes_from_system_message_declarations() {
    let catalog = read(&fixture()).tool_catalog;
    let names: Vec<_> = catalog
        .iter()
        .map(|tool| (tool.name.as_str(), tool.kind))
        .collect();
    assert_eq!(
        names,
        vec![
            ("read", ToolKind::Builtin),
            ("bash", ToolKind::Builtin),
            ("edit", ToolKind::Builtin),
            ("write", ToolKind::Builtin),
        ]
    );
}

#[test]
fn malformed_and_unknown_lines_are_counted_as_skipped() {
    let session = read(&fixture());
    assert_eq!((session.total_records, session.skipped_records), (18, 2));
    assert!(!session.partial);
}

#[test]
fn more_than_twenty_percent_skipped_is_partial() {
    let text = fs::read_to_string(fixture()).unwrap() + &"garbage\n".repeat(10);
    let session = read(&temporary(&text));
    assert_eq!((session.total_records, session.skipped_records), (28, 12));
    assert!(session.partial);
    assert_eq!(session.turns.len(), 2);
}

#[test]
fn the_active_branch_follows_the_last_entry() {
    // Move the abandoned branch to the end of the file: it becomes the active leaf.
    let text = fs::read_to_string(fixture()).unwrap();
    let mut lines: Vec<_> = text.lines().collect();
    let moved: Vec<_> = lines.drain(7..9).collect();
    lines.extend(moved);
    let session = read(&temporary(&(lines.join("\n") + "\n")));
    assert_eq!(session.turns.len(), 2);
    assert_eq!(session.turns[1].prompt, "Abandoned prompt");
}

#[test]
fn missing_header_is_invalid() {
    let text = fs::read_to_string(fixture()).unwrap();
    let body = text.split_once('\n').unwrap().1;
    let error = PiAdapter::new()
        .read_session(&source(&temporary(body)))
        .unwrap_err();
    assert!(
        matches!(error, AdapterError::InvalidPayload { .. }),
        "{error:?}"
    );
}

#[test]
fn legacy_version_one_sessions_are_unsupported() {
    let text = fs::read_to_string(fixture())
        .unwrap()
        .replacen("\"version\":3", "\"version\":1", 1);
    let error = PiAdapter::new()
        .read_session(&source(&temporary(&text)))
        .unwrap_err();
    assert!(
        matches!(error, AdapterError::Unsupported { .. }),
        "{error:?}"
    );
}

#[test]
fn missing_file_is_an_io_error() {
    let error = PiAdapter::new()
        .read_session(&source(Path::new("/nonexistent/tkm/session.jsonl")))
        .unwrap_err();
    assert!(matches!(error, AdapterError::Io(_)), "{error:?}");
}

fn fixture_text() -> String {
    fs::read_to_string(fixture()).unwrap()
}

fn sum_by_model(session: &Session) -> Usage {
    session
        .usage_by_model
        .values()
        .fold(Usage::default(), |total, usage| total + *usage)
}

#[test]
fn parent_links_through_unknown_entry_types_still_reach_the_root() {
    let header = fixture_text().lines().next().unwrap().to_owned();
    let text = format!(
        "{header}\n{}\n{}\n{}\n",
        r#"{"type":"future-kind","id":"z0000001","parentId":null,"timestamp":"2026-10-20T10:00:01.000Z"}"#,
        r#"{"type":"message","id":"u0000001","parentId":"z0000001","timestamp":"2026-10-20T10:00:02.000Z","message":{"role":"user","content":"Hello"}}"#,
        r#"{"type":"message","id":"u0000002","parentId":"u0000001","timestamp":"2026-10-20T10:00:03.000Z","message":{"role":"user","content":"Again"}}"#,
    );
    let session = read(&temporary(&text));
    let prompts: Vec<_> = session
        .turns
        .iter()
        .map(|turn| turn.prompt.as_str())
        .collect();
    assert_eq!(prompts, vec!["Hello", "Again"]);
    // The unknown entry is not interpreted, so it is still counted as skipped.
    assert_eq!((session.total_records, session.skipped_records), (4, 1));
}

#[test]
fn usage_entries_count_wherever_they_sit_but_abandoned_messages_do_not() {
    let usage_entry = r#"{"type":"usage","id":"x0000002","parentId":"a0000003","timestamp":"2026-10-20T10:02:30.000Z","kind":"cache_warm","provider":"example","model":"example-model-a","usage":{"input":0,"output":0,"cacheRead":7,"cacheWrite":0,"totalTokens":7}}"#;
    let session = read(&temporary(&format!("{}{usage_entry}\n", fixture_text())));
    // 5,000 input tokens of the abandoned assistant message stay out; the 7 cached tokens are in.
    assert_eq!(session.usage, usage(870, 105, 51_707, 10));
    // A usage entry is not a conversation entry: it never becomes the active leaf.
    assert_eq!(session.turns.len(), 2);
    assert_eq!(session.turns[1].prompt, "Write a file\nnow");
    assert_eq!(sum_by_model(&session), session.usage);
}

#[test]
fn nested_tool_result_usage_is_attributed_to_the_turn_and_its_model() {
    let nested = r#""isError":false,"usage":{"input":1,"output":2,"cacheRead":3,"cacheWrite":4,"totalTokens":10},"timestamp""#;
    let text = fixture_text().replacen(r#""isError":false,"timestamp""#, nested, 1);
    let session = read(&temporary(&text));
    assert_eq!(session.turns[0].usage, usage(221, 52, 203, 14));
    assert_eq!(session.usage, usage(871, 107, 51_703, 14));
    assert_eq!(sum_by_model(&session), session.usage);
    assert_eq!(sum_by_model(&read(&fixture())), read(&fixture()).usage);
}
