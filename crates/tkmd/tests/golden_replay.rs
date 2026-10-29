//! SPEC 16 Golden: replay fixtures per harness (all eight); expected actions committed as JSON.
//!
//! For each host, every fixture file is fed through the adapter and daemon handle pipeline.
//! The reply action and stdout are compared against the committed JSON in
//! `fixtures/golden/<host>.json`. Run with `TKM_BLESS=1` to update the committed JSON.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use common::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tkm_proto::{EventKind, Host, PROTOCOL_VERSION, ReplyAction, Request};
use tkmd::decider::NullDecider;
use tkmd::settings::Settings;

/// One entry in the committed golden JSON.
#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct GoldenEntry {
    fixture: String,
    event: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    host_event: Option<String>,
    action: String,
    stdout: String,
}

/// Root of the workspace (the directory containing Cargo.toml).
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

fn golden_path(host: &str) -> PathBuf {
    workspace_root().join("fixtures/golden").join(format!("{host}.json"))
}

fn fixtures_dir(host: &str) -> PathBuf {
    match host {
        "opencode" | "pi" => workspace_root()
            .join("crates/tkm-hosts/tests/fixtures")
            .join(host)
            .join("hooks"),
        _ => workspace_root().join(format!("fixtures/{host}/hooks")),
    }
}

fn load_golden(host: &str) -> Vec<GoldenEntry> {
    let path = golden_path(host);
    serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_else(|_| "[]".into()))
        .unwrap_or_default()
}

fn save_golden(host: &str, entries: &[GoldenEntry]) {
    let path = golden_path(host);
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, serde_json::to_string_pretty(entries).unwrap() + "\n").unwrap();
}

fn event_kind(event: &str) -> EventKind {
    match event {
        "session_start" => EventKind::SessionStart,
        "prompt" => EventKind::Prompt,
        "tool_before" => EventKind::ToolBefore,
        "tool_done" => EventKind::ToolDone,
        "turn_end" => EventKind::TurnEnd,
        "session_end" => EventKind::SessionEnd,
        other => panic!("unknown event kind: {other}"),
    }
}

fn host_variant(host: &str) -> Host {
    match host {
        "claude" => Host::Claude,
        "codex" => Host::Codex,
        "agy" => Host::Agy,
        "cursor" => Host::Cursor,
        "kimi" => Host::Kimi,
        "codewhale" => Host::Deepseek,
        "opencode" => Host::Opencode,
        "pi" => Host::Pi,
        other => panic!("unknown host: {other}"),
    }
}

fn action_label(action: &ReplyAction) -> &'static str {
    match action {
        ReplyAction::Nothing => "Nothing",
        ReplyAction::Inject { .. } => "Inject",
        ReplyAction::Notice { .. } => "Notice",
        ReplyAction::InjectAndNotice { .. } => "InjectAndNotice",
        _ => "Unknown",
    }
}

/// Replays all fixtures for one host and returns the actual entries.
///
/// Each fixture is replayed against a fresh daemon (NullDecider, default settings)
/// with the host forced to the given variant.
#[tokio::main]
async fn replay_host(host: &str, golden: &[GoldenEntry]) -> Vec<GoldenEntry> {
    let host_variant = host_variant(host);
    let fixtures_dir = fixtures_dir(host);

    let mut actual: Vec<GoldenEntry> = Vec::new();

    for entry in golden {
        let fixture_path = fixtures_dir.join(format!("{}.json", entry.fixture));
        let payload: Value =
            serde_json::from_str(&std::fs::read_to_string(&fixture_path).unwrap_or_else(|_| {
                panic!("fixture not found: {}", fixture_path.display())
            }))
            .unwrap();

        let kind = event_kind(&entry.event);
        let request = Request {
            version: PROTOCOL_VERSION,
            host: host_variant,
            event: kind,
            host_event: entry.host_event.clone(),
            payload,
            env: BTreeMap::new(),
            cwd: None,
        };

        let env = Env::new();
        let daemon = daemon_with(&env, Settings::default(), Arc::new(NullDecider));

        let reply = if kind == EventKind::ToolDone {
            daemon.handle_tool_done(&request)
        } else {
            daemon.handle(request).await
        };

        actual.push(GoldenEntry {
            fixture: entry.fixture.clone(),
            event: entry.event.clone(),
            host_event: entry.host_event.clone(),
            action: action_label(&reply.action).to_owned(),
            stdout: reply.stdout,
        });
    }

    actual
}

fn run_golden_test(host: &str) {
    let bless = std::env::var("TKM_BLESS").is_ok_and(|v| v == "1");
    let golden = load_golden(host);
    assert!(
        !golden.is_empty(),
        "no golden entries for {host}; committed file is missing or empty"
    );

    let actual = replay_host(host, &golden);

    if bless {
        save_golden(host, &actual);
        return;
    }

    for (i, (expected, got)) in golden.iter().zip(actual.iter()).enumerate() {
        assert_eq!(
            got, expected,
            "golden mismatch for {host} fixture {} (index {i})",
            expected.fixture
        );
    }
    assert_eq!(
        actual.len(),
        golden.len(),
        "golden entry count mismatch for {host}"
    );
}

#[test]
fn golden_replay_claude() {
    run_golden_test("claude");
}

#[test]
fn golden_replay_codex() {
    run_golden_test("codex");
}

#[test]
fn golden_replay_agy() {
    run_golden_test("agy");
}

#[test]
fn golden_replay_cursor() {
    run_golden_test("cursor");
}

#[test]
fn golden_replay_kimi() {
    run_golden_test("kimi");
}

#[test]
fn golden_replay_codewhale() {
    run_golden_test("codewhale");
}

#[test]
fn golden_replay_opencode() {
    run_golden_test("opencode");
}

#[test]
fn golden_replay_pi() {
    run_golden_test("pi");
}
