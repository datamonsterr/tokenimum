#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Offline behavioral tests; every transcript is invented.
use std::collections::BTreeMap;
use std::path::PathBuf;
use tkm_cli::replay_bench::{
    Commit, ModelPrice, Outcome, SessionSpec, Tokens, account, anchor, check_config, compare,
    extract_prompts, load_transcript,
};

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/replay-bench")
        .join(name)
}
fn spec(id: &str, name: &str, parent: Option<&str>) -> SessionSpec {
    SessionSpec {
        id: id.into(),
        path: fixture(name),
        parent: parent.map(str::to_owned),
        ..SessionSpec::default()
    }
}
fn prices() -> BTreeMap<String, ModelPrice> {
    BTreeMap::from([(
        "synthetic-model".into(),
        ModelPrice {
            input: 1.0,
            cache_creation: 2.0,
            cache_read: 0.1,
            output: 5.0,
        },
    )])
}
#[test]
fn extraction_retains_typed_text_and_drops_injections_and_tool_results() {
    let transcript = load_transcript(&spec("main", "main.jsonl", None)).unwrap();
    let prompts = extract_prompts(&transcript.records);
    assert_eq!(
        prompts.iter().map(|p| p.text.as_str()).collect::<Vec<_>>(),
        [
            "Create a synthetic widget",
            "Add a test\nKeep it small",
            "Preserve this typed request"
        ]
    );
}
#[test]
fn malformed_and_unfinished_transcripts_are_not_intact() {
    assert!(load_transcript(&spec("bad", "malformed.jsonl", None)).is_err());
    assert!(
        !load_transcript(&spec("running", "running.jsonl", None))
            .unwrap()
            .intact
    );
    assert!(
        !load_transcript(&spec("tool", "tool-pending.jsonl", None))
            .unwrap()
            .intact
    );
}
#[test]
fn accounting_counts_each_message_once_and_attributes_children_to_parent() {
    let main = load_transcript(&spec("main", "main.jsonl", None)).unwrap();
    let child = load_transcript(&spec("so/child", "agent.jsonl", Some("main"))).unwrap();
    let baseline = account(&[main, child], &prices()).unwrap();
    assert_eq!(
        baseline.total,
        Tokens {
            input: 17,
            cache_creation: 24,
            cache_read: 31,
            output: 13
        }
    );
    assert_eq!(baseline.sessions.len(), 1);
    assert_eq!(baseline.sessions["main"].tokens, baseline.total);
    assert_eq!(baseline.by_model["synthetic-model"], baseline.total);
    assert_eq!(baseline.sessions["main"].tool_calls, 1);
    assert_eq!(baseline.sessions["main"].tool_errors, 1);
    assert_eq!(baseline.sessions["main"].wall_ms, 10_000);
}
#[test]
fn accounting_rejects_orphans_cycles_and_missing_prices() {
    let child = load_transcript(&spec("so/child", "agent.jsonl", Some("missing"))).unwrap();
    assert!(account(&[child], &prices()).is_err());
    let cyclic = load_transcript(&spec("main", "main.jsonl", Some("main"))).unwrap();
    assert!(account(&[cyclic], &prices()).is_err());
    let main = load_transcript(&spec("main", "main.jsonl", None)).unwrap();
    assert!(account(&[main], &BTreeMap::new()).is_err());
}
#[test]
fn anchoring_uses_latest_commit_at_or_before_first_timestamp() {
    let commits = vec![
        Commit {
            hash: "b".repeat(40),
            timestamp_ms: 2000,
        },
        Commit {
            hash: "a".repeat(40),
            timestamp_ms: 1000,
        },
    ];
    assert_eq!(anchor(&commits, 1500), Some("a".repeat(40)));
    assert_eq!(anchor(&commits, 2000), Some("b".repeat(40)));
    assert_eq!(anchor(&commits, 999), None);
}
#[test]
fn config_diff_accepts_exact_init_hooks_and_refuses_other_changes() {
    check_config(&fixture("config-a"), &fixture("config-b")).unwrap();
    assert!(check_config(&fixture("config-a"), &fixture("config-bad")).is_err());
    assert!(check_config(&fixture("config-a"), &fixture("config-spoof")).is_err());
}
#[test]
fn comparison_requires_ten_controls_outcomes_and_saving_above_variance() {
    let outcome = Outcome {
        tests_passed: true,
        changed_file_overlap: 1.0,
    };
    assert!(compare(100.0, 50.0, &[0.1; 9], outcome).is_err());
    assert!(compare(100.0, 50.0, &[0.1; 10], outcome).unwrap().saving);
    assert!(!compare(100.0, 95.0, &[0.1; 10], outcome).unwrap().saving);
    assert!(
        compare(
            100.0,
            50.0,
            &[0.1; 10],
            Outcome {
                tests_passed: false,
                ..outcome
            }
        )
        .unwrap()
        .regression
    );
}
