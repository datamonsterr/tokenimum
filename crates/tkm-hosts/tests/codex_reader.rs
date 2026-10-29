#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use serde_json::Value;
use tkm_core::model::{
    EditKind, EditVia, FileEdit, RepoKey, Session, StopReason, Timestamp, Usage,
};
use tkm_hosts::codex::CodexAdapter;
use tkm_hosts::codex::reader::decompress_zstd;
use tkm_hosts::{AdapterError, HostAdapter, SessionSource};
use tkm_proto::Host;

const DIR: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/codex/sessions/2026/10/29"
);
const PLAIN: &str = "rollout-2026-10-29T09-00-20-01a18562-fb1c-7522-9939-dda89fa83662.jsonl";
const ZST: &str = "rollout-2026-10-29T08-00-00-01a18562-fb1c-7522-9939-dda89fa83662.jsonl.zst";
const MODEL: &str = "gpt-6.1-sol";

fn source(path: &Path) -> SessionSource {
    SessionSource {
        agent_home: "/synthetic/codex".into(),
        path: path.to_path_buf(),
        session_id: "01a18562-fb1c-7522-9939-dda89fa83662".into(),
        repo_key: RepoKey("synthetic-origin".into()),
        cwd: "~/dev/example".into(),
    }
}

fn read(path: &Path) -> Result<Session, AdapterError> {
    CodexAdapter::new().read_session(&source(path))
}

fn plain_path() -> PathBuf {
    Path::new(DIR).join(PLAIN)
}

fn scratch(name: &str) -> PathBuf {
    static COUNTER: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "tkm-codex-reader-{}-{}-{name}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// Sums `token_count.info.last_token_usage` straight from the fixture JSON.
fn expected_usage(turn_ids: &[&str]) -> Vec<Usage> {
    let mut sums = vec![Usage::default(); turn_ids.len()];
    let mut current = 0;
    for line in fs::read_to_string(plain_path()).unwrap().lines() {
        let record: Value = serde_json::from_str(line).unwrap();
        let payload = &record["payload"];
        if record["type"] == "event_msg" && payload["type"] == "task_started" {
            let id = payload["turn_id"].as_str().unwrap();
            current = turn_ids.iter().position(|t| *t == id).unwrap();
        }
        if record["type"] == "event_msg" && payload["type"] == "token_count" {
            let last = &payload["info"]["last_token_usage"];
            let n = |key: &str| last[key].as_u64().unwrap();
            sums[current] = sums[current]
                + Usage {
                    input: n("input_tokens") - n("cached_input_tokens"),
                    output: n("output_tokens"),
                    cache_read: n("cached_input_tokens"),
                    cache_write: n("cache_write_input_tokens"),
                };
        }
    }
    sums
}

#[test]
fn reads_plain_rollout_into_two_turns_with_prompts_and_calls() {
    let session = read(&plain_path()).unwrap();
    assert_eq!(session.host, Host::Codex);
    assert_eq!(session.agent_home, PathBuf::from("/synthetic/codex"));
    assert_eq!(session.session_id, "01a18562-fb1c-7522-9939-dda89fa83662");
    assert_eq!(session.repo_key, RepoKey("synthetic-origin".into()));
    assert_eq!(session.cwd, PathBuf::from("~/dev/example"));
    assert_eq!(session.started, Timestamp(1_793_239_220_493));
    assert_eq!(session.ended, Some(Timestamp(1_793_239_379_777)));
    assert_eq!(session.total_records, 48);
    assert_eq!(session.skipped_records, 0);
    assert!(!session.partial);

    assert_eq!(session.turns.len(), 2);
    let (first, second) = (&session.turns[0], &session.turns[1]);
    assert_eq!(
        first.prompt,
        "Run the shell command 'cat a.txt', then 'false'. Reply done."
    );
    assert_eq!(
        second.prompt,
        "Create file c.txt containing the word x using apply_patch, then run ls. Reply done."
    );
    assert_eq!(first.stopped_by, StopReason::OwnStop);
    assert_eq!(second.stopped_by, StopReason::OwnStop);

    let summary = |turn: &tkm_core::model::Turn| -> Vec<(String, String, Option<i32>, bool)> {
        turn.calls
            .iter()
            .map(|c| {
                (
                    c.tool.clone(),
                    c.args_digest.clone(),
                    c.exit_code,
                    c.is_error,
                )
            })
            .collect()
    };
    assert_eq!(
        summary(first),
        vec![
            ("Bash".into(), "cat a.txt".into(), Some(0), false),
            ("Bash".into(), "false".into(), Some(1), true),
        ]
    );
    assert_eq!(first.calls[0].output_digest, "hello");
    assert_eq!(first.calls[1].output_digest, "");
    assert_eq!(first.calls[0].duration_ms, Some(0));
    assert_eq!(
        summary(second),
        vec![
            (
                "apply_patch".into(),
                r#"[["add","c.txt","x\n"]]"#.into(),
                None,
                false
            ),
            ("Bash".into(), "ls".into(), Some(0), false),
        ]
    );
    assert_eq!(
        second.calls[0].edits,
        vec![FileEdit {
            path: "c.txt".into(),
            kind: EditKind::Create,
            via: EditVia::Tool,
        }]
    );
    assert_eq!(second.calls[1].edits, [] as [FileEdit; 0]);
    assert_eq!(second.calls[1].output_digest, "c.txt");
}

#[test]
fn usage_comes_from_token_count_last_usage_only() {
    let session = read(&plain_path()).unwrap();
    let expected = expected_usage(&[
        "01a18563-0161-7532-ad25-1247eddeda68",
        "01a18565-4b80-7293-aec5-d044b6c35623",
    ]);
    assert!(expected[0].input > 0 && expected[0].cache_read > 0 && expected[0].output > 0);
    assert_eq!(session.turns[0].usage, expected[0]);
    assert_eq!(session.turns[1].usage, expected[1]);
    let total = expected[0] + expected[1];
    assert_eq!(session.usage, total);
    assert_eq!(session.turns[0].model.as_deref(), Some(MODEL));
    assert_eq!(session.turns[0].usage_by_model[MODEL], expected[0]);
    assert_eq!(session.usage_by_model[MODEL], total);
    // Pin the arithmetic once so a wrong fixture-side sum cannot hide a bug.
    assert_eq!(
        total,
        Usage {
            input: 2_577 + 1_369,
            output: 70 + 148,
            cache_read: 26_624 + 29_824,
            cache_write: 0,
        }
    );
}

#[test]
fn zst_rollout_decodes_to_identical_session() {
    let plain = read(&plain_path()).unwrap();
    let compressed = read(&Path::new(DIR).join(ZST)).unwrap();
    assert_eq!(plain, compressed);
}

#[test]
fn zstd_decompression_enforces_the_size_cap() {
    let bytes = fs::read(Path::new(DIR).join(ZST)).unwrap();
    let expected = fs::read(plain_path()).unwrap();
    assert_eq!(decompress_zstd(&bytes, expected.len()).unwrap(), expected);
    assert_eq!(decompress_zstd(&bytes, 1 << 20).unwrap(), expected);
    let error = decompress_zstd(&bytes, expected.len() - 1).unwrap_err();
    assert!(error.to_string().contains("limit"), "{error}");
    assert!(decompress_zstd(&bytes, 100).is_err());
    assert!(decompress_zstd(b"not zstd at all", 1 << 20).is_err());
    assert!(decompress_zstd(&bytes[..bytes.len() / 2], 1 << 20).is_err());
}

#[test]
fn corrupt_zst_and_missing_file_are_errors() {
    let dir = scratch("corrupt");
    let bad = dir.join("rollout-x.jsonl.zst");
    fs::write(&bad, b"definitely not zstd").unwrap();
    assert!(read(&bad).is_err());
    assert!(matches!(
        read(&dir.join("absent.jsonl")),
        Err(AdapterError::Io(_))
    ));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn truncated_last_line_is_skipped_not_an_error() {
    let text = fs::read_to_string(plain_path()).unwrap();
    let cut = &text[..text.trim_end().len() - 20];
    let dir = scratch("truncated");
    let path = dir.join("rollout-trunc.jsonl");
    fs::write(&path, cut).unwrap();
    let session = read(&path).unwrap();
    assert_eq!(session.total_records, 48);
    assert_eq!(session.skipped_records, 1);
    assert!(!session.partial);
    assert_eq!(session.turns.len(), 2);
    // The cut record was task_complete: last turn incomplete, ended = last intact record.
    assert_eq!(session.turns[1].stopped_by, StopReason::Unknown);
    assert_eq!(session.ended, Some(Timestamp(1_793_239_379_771)));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn more_than_twenty_percent_garbage_marks_the_session_partial() {
    let text = fs::read_to_string(plain_path()).unwrap();
    let mut lines: Vec<&str> = text.lines().take(10).collect();
    let junk = [
        "not json",
        "{\"type\":\"mystery\",\"payload\":{}}",
        "[1,2,3]",
    ];
    lines.extend(junk);
    let dir = scratch("garbage");
    let path = dir.join("rollout-garbage.jsonl");
    fs::write(&path, lines.join("\n") + "\n").unwrap();
    let session = read(&path).unwrap();
    assert_eq!(session.total_records, 13);
    assert_eq!(session.skipped_records, 3);
    assert!(session.partial);

    // Exactly 20% skipped is not partial: 1 junk line in 5 records.
    let mut ok: Vec<&str> = text.lines().take(4).collect();
    ok.push("not json");
    fs::write(&path, ok.join("\n") + "\n").unwrap();
    let session = read(&path).unwrap();
    assert_eq!((session.total_records, session.skipped_records), (5, 1));
    assert!(!session.partial);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn synthetic_items_cover_delete_update_aborts_outside_paths_and_unknown_events() {
    let lines = [
        r#"{"timestamp":"2026-10-29T02:00:00.000Z","type":"session_meta","payload":{"id":"s1","cwd":"/work/repo","timestamp":"2026-10-29T02:00:00.000Z","cli_version":"0.164.0"}}"#,
        r#"{"timestamp":"2026-10-29T02:00:01.000Z","type":"event_msg","payload":{"type":"task_started","turn_id":"t1"}}"#,
        r#"{"timestamp":"2026-10-29T02:00:02.000Z","type":"event_msg","payload":{"type":"item_completed","turn_id":"t1","item":{"type":"UserMessage","content":[{"type":"text","text":"do it"}]}}}"#,
        r#"{"timestamp":"2026-10-29T02:00:03.000Z","type":"event_msg","payload":{"type":"item_completed","turn_id":"t1","item":{"type":"FileChange","changes":{"/work/repo/a.rs":{"type":"update","unified_diff":"@@"},"/work/repo/b.rs":{"type":"delete"},"/elsewhere/x":{"type":"add","content":"x"}},"status":"failed","stdout":"","stderr":"boom"}}}"#,
        r#"{"timestamp":"2026-10-29T02:00:04.000Z","type":"event_msg","payload":{"type":"item_completed","turn_id":"t1","item":{"type":"CommandExecution","command":["/bin/bash","-lc","exit 3"],"status":"completed","aggregated_output":"fatal: x\nlater","exit_code":3,"duration":{"secs":1,"nanos":500000000}}}}"#,
        r#"{"timestamp":"2026-10-29T02:00:05.000Z","type":"event_msg","payload":{"type":"turn_aborted","turn_id":"t1","reason":"interrupted"}}"#,
        r#"{"timestamp":"2026-10-29T02:00:06.000Z","type":"event_msg","payload":{"type":"brand_new_event"}}"#,
    ];
    let dir = scratch("synthetic");
    let path = dir.join("rollout-synth.jsonl");
    fs::write(&path, lines.join("\n") + "\n").unwrap();
    let mut src = source(&path);
    src.cwd = "/ignored".into();
    let session = CodexAdapter::new().read_session(&src).unwrap();
    assert_eq!(session.cwd, PathBuf::from("/work/repo"));
    assert_eq!(session.session_id, "s1");
    assert_eq!(session.total_records, 7);
    assert_eq!(session.skipped_records, 1);
    let turn = &session.turns[0];
    assert_eq!(turn.stopped_by, StopReason::Interrupted);
    assert_eq!(turn.prompt, "do it");
    let applied = &turn.calls[0];
    assert_eq!(applied.tool, "apply_patch");
    assert!(applied.is_error);
    let mut edits: Vec<(String, EditKind)> = applied
        .edits
        .iter()
        .map(|e| (e.path.to_string_lossy().into_owned(), e.kind))
        .collect();
    edits.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(
        edits,
        vec![
            ("a.rs".to_owned(), EditKind::Modify),
            ("b.rs".to_owned(), EditKind::Delete)
        ]
    );
    let shell = &turn.calls[1];
    assert_eq!(shell.args_digest, "exit 3");
    assert_eq!(shell.exit_code, Some(3));
    assert!(shell.is_error);
    assert_eq!(shell.duration_ms, Some(1_500));
    assert_eq!(shell.output_digest, "fatal: x");
    fs::remove_dir_all(dir).unwrap();
}

fn read_records(name: &str, records: &[Value]) -> Session {
    let dir = scratch(name);
    let path = dir.join("rollout.jsonl");
    fs::write(
        &path,
        records
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n"),
    )
    .unwrap();
    let session = read(&path).unwrap();
    fs::remove_dir_all(dir).unwrap();
    session
}

#[test]
fn identical_token_count_snapshots_are_counted_once_across_turns() {
    let usage = serde_json::json!({"input_tokens":100,"cached_input_tokens":30,"cache_write_input_tokens":0,"output_tokens":5});
    let record = serde_json::json!({"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":usage,"last_token_usage":usage}}});
    let session = read_records(
        "duplicate-usage",
        &[
            record.clone(),
            serde_json::json!({"type":"event_msg","payload":{"type":"task_complete"}}),
            serde_json::json!({"type":"event_msg","payload":{"type":"task_started"}}),
            record,
        ],
    );
    assert_eq!(
        session.usage,
        Usage {
            input: 70,
            output: 5,
            cache_read: 30,
            cache_write: 0
        }
    );
    assert_eq!(session.turns[1].usage, Usage::default());
}

#[test]
fn cache_writes_are_excluded_from_uncached_input_with_saturation() {
    for (input, expected) in [(100, 50), (40, 0)] {
        let usage = serde_json::json!({"input_tokens":input,"cached_input_tokens":30,"cache_write_input_tokens":20,"output_tokens":5});
        let session = read_records(
            "cache-write",
            &[
                serde_json::json!({"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":usage,"last_token_usage":usage}}}),
            ],
        );
        assert_eq!(
            session.usage,
            Usage {
                input: expected,
                output: 5,
                cache_read: 30,
                cache_write: 20
            }
        );
    }
}

#[test]
fn malformed_and_unknown_completed_items_count_as_skipped() {
    let records = [Value::Null, serde_json::json!(7), serde_json::json!([]), serde_json::json!({}), serde_json::json!({"type":"FutureItem"})]
        .map(|item| serde_json::json!({"type":"event_msg","payload":{"type":"item_completed","item":item}}));
    let session = read_records("malformed-items", &records);
    assert_eq!((session.total_records, session.skipped_records), (5, 5));
    assert!(session.partial);
    assert_eq!(session.turns.len(), 0);
}

#[test]
fn apply_patch_fixture_args_digest_matches_live_hook() {
    let payload: Value = serde_json::from_str(include_str!(
        "../../../fixtures/codex/hooks/PostToolUse-apply_patch.json"
    ))
    .unwrap();
    let request = tkm_proto::Request {
        version: tkm_proto::PROTOCOL_VERSION,
        host: Host::Codex,
        event: tkm_proto::EventKind::ToolDone,
        host_event: Some("PostToolUse".into()),
        payload,
        env: std::collections::BTreeMap::new(),
        cwd: None,
    };
    let live = CodexAdapter::new()
        .map_event(&request)
        .unwrap()
        .unwrap()
        .call
        .unwrap();
    let session = read(&plain_path()).unwrap();
    let call = &session.turns[1].calls[0];
    assert_eq!(call.args_digest, live.args_digest);
    assert_eq!(call.output_digest, live.output_digest);
}

#[test]
fn concatenated_zstd_frames_decode_all_records_and_share_one_cap() {
    let bytes = include_bytes!("fixtures/codex/sessions/multiframe.jsonl.zst");
    let plain = fs::read(plain_path()).unwrap();
    assert_eq!(decompress_zstd(bytes, plain.len()).unwrap(), plain);
    assert!(decompress_zstd(bytes, plain.len() - 1).is_err());
    assert!(decompress_zstd(&bytes[..bytes.len() - 3], plain.len()).is_err());
    let dir = scratch("multiframe");
    let path = dir.join("rollout.jsonl.zst");
    fs::write(&path, bytes).unwrap();
    assert_eq!(read(&path).unwrap(), read(&plain_path()).unwrap());
    fs::remove_dir_all(dir).unwrap();
}
