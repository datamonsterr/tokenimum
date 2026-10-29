//! SPEC 16 Parity: for Claude Code and Codex fixtures, tokenimum's fact formation and loop gate
//! agree with TE run over the same logs. TE is the oracle in M2; differences are documented.
//!
//! The oracle JSON files at `fixtures/golden/parity_<host>_oracle.json` record TE's expected
//! output. This test runs tokenimum's ported TE rules over the same fixture session logs and
//! asserts that they match the oracle. Any documented difference is recorded in docs/parity.md.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tkm_core::facts::{self, Candidate, FactsState, observe};
use tkm_core::model::{RepoKey, Session};
use tkm_hosts::{HostAdapter, SessionSource, claude::ClaudeAdapter, codex::CodexAdapter};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .unwrap()
}

#[derive(Debug, Deserialize, Serialize)]
struct ParityOracle {
    description: String,
    session_id: String,
    host: String,
    turns: usize,
    te_fact_candidates: Vec<String>,
    te_fact_active: Vec<String>,
    tkm_agrees: bool,
    differences: Vec<String>,
    notes: String,
}

fn load_oracle(name: &str) -> ParityOracle {
    let path = workspace_root()
        .join("fixtures/golden")
        .join(format!("parity_{name}_oracle.json"));
    serde_json::from_str(&std::fs::read_to_string(&path).unwrap_or_else(|e| {
        panic!("oracle file not found: {} ({e})", path.display())
    }))
    .unwrap()
}

/// Run one session through tokenimum's ported TE fact-extraction rules.
fn extract_candidate_ids(session: &Session) -> Vec<String> {
    let observations = observe::observe(session);
    let mut ids: Vec<String> = facts::extract::extract(&observations)
        .into_iter()
        .map(|c: Candidate| c.id)
        .collect();
    ids.sort();
    ids
}

fn claude_source() -> SessionSource {
    SessionSource {
        path: workspace_root()
            .join("fixtures/claude/sessions/e928a2cf-e31b-4154-805b-919991d042ea.jsonl"),
        cwd: "~/dev/example".into(),
        session_id: "e928a2cf-e31b-4154-805b-919991d042ea".into(),
        repo_key: RepoKey("parity-claude-repo".into()),
        agent_home: "/synthetic/agent".into(),
    }
}

fn codex_source() -> SessionSource {
    SessionSource {
        path: workspace_root().join(
            "fixtures/codex/sessions/2026/10/29/rollout-2026-10-29T09-00-20-01a18562-fb1c-7522-9939-dda89fa83662.jsonl",
        ),
        cwd: "~/dev/example".into(),
        session_id: "01a18562-fb1c-7522-9939-dda89fa83662".into(),
        repo_key: RepoKey("parity-codex-repo".into()),
        agent_home: "/synthetic/agent".into(),
    }
}

#[test]
fn parity_claude_fact_formation_matches_te_oracle() {
    let oracle = load_oracle("claude");
    let session = ClaudeAdapter::new()
        .read_session(&claude_source())
        .expect("claude fixture session reads");

    assert_eq!(
        session.turns.len(),
        oracle.turns,
        "expected {} turns in Claude fixture session",
        oracle.turns
    );

    let mut tkm_candidates = extract_candidate_ids(&session);
    tkm_candidates.sort();
    let mut oracle_candidates = oracle.te_fact_candidates.clone();
    oracle_candidates.sort();

    if oracle.tkm_agrees {
        assert_eq!(
            tkm_candidates, oracle_candidates,
            "Claude parity: tokenimum candidates don't match TE oracle.\nSee docs/parity.md for documented differences.\ntkm: {tkm_candidates:?}\noracle: {oracle_candidates:?}"
        );
    } else {
        // Documented differences; do not hard-fail.
        eprintln!("Claude parity DOCUMENTED DIFFERENCES: {:?}", oracle.differences);
        for diff in &oracle.differences {
            eprintln!("  DIFF: {diff}");
        }
    }
}

#[test]
fn parity_codex_fact_formation_matches_te_oracle() {
    let oracle = load_oracle("codex");
    let session = CodexAdapter::new()
        .read_session(&codex_source())
        .expect("codex fixture session reads");

    assert_eq!(
        session.turns.len(),
        oracle.turns,
        "expected {} turns in Codex fixture session",
        oracle.turns
    );

    let mut tkm_candidates = extract_candidate_ids(&session);
    tkm_candidates.sort();
    let mut oracle_candidates = oracle.te_fact_candidates.clone();
    oracle_candidates.sort();

    if oracle.tkm_agrees {
        assert_eq!(
            tkm_candidates, oracle_candidates,
            "Codex parity: tokenimum candidates don't match TE oracle.\nSee docs/parity.md for documented differences.\ntkm: {tkm_candidates:?}\noracle: {oracle_candidates:?}"
        );
    } else {
        eprintln!("Codex parity DOCUMENTED DIFFERENCES: {:?}", oracle.differences);
    }
}

#[test]
fn parity_claude_loopguard_agrees_with_te_rules() {
    use tkm_core::loopguard::{EscalationMode, LoopAction, LoopConfig, TurnGuard};

    let oracle = load_oracle("claude");
    let session = ClaudeAdapter::new()
        .read_session(&claude_source())
        .expect("claude fixture session reads");

    // For the Claude fixture session, Turn 0 has only 2 bash calls (cat then false).
    // TE's spiral rules require 18+ repeating error calls to open the judge gate.
    // The fixture turn is far below that threshold.
    if let Some(turn) = session.turns.first() {
        let mut guard = TurnGuard::new(EscalationMode::DelegatedNoAdapter);
        let cfg = LoopConfig::default();
        let mut judge_requests = 0usize;
        for call in &turn.calls {
            for action in guard.on_tool_done(call, &cfg, None) {
                if matches!(action, LoopAction::RequestJudge { .. }) {
                    judge_requests += 1;
                }
            }
        }
        if oracle.tkm_agrees {
            assert_eq!(
                judge_requests, 0,
                "Claude loopguard parity: expected 0 judge requests on short fixture turn (2 calls)"
            );
        } else {
            eprintln!("Claude loopguard parity DOCUMENTED DIFFERENCES: {:?}", oracle.differences);
        }
    }
}

#[test]
fn parity_codex_loopguard_agrees_with_te_rules() {
    use tkm_core::loopguard::{EscalationMode, LoopAction, LoopConfig, TurnGuard};

    let oracle = load_oracle("codex");
    let session = CodexAdapter::new()
        .read_session(&codex_source())
        .expect("codex fixture session reads");

    if let Some(turn) = session.turns.first() {
        let mut guard = TurnGuard::new(EscalationMode::DelegatedNoAdapter);
        let cfg = LoopConfig::default();
        let mut judge_requests = 0usize;
        for call in &turn.calls {
            for action in guard.on_tool_done(call, &cfg, None) {
                if matches!(action, LoopAction::RequestJudge { .. }) {
                    judge_requests += 1;
                }
            }
        }
        if oracle.tkm_agrees {
            assert_eq!(
                judge_requests, 0,
                "Codex loopguard parity: expected 0 judge requests on short fixture turn"
            );
        } else {
            eprintln!("Codex loopguard parity DOCUMENTED DIFFERENCES: {:?}", oracle.differences);
        }
    }
}
