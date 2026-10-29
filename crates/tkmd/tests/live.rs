//! E4.3 live layer: gate, judge queue, delivery on a later `tool_done`, shadow mode, escalation.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]

mod common;

use std::sync::Arc;

use common::*;
use tkm_decide::calibrate::{CalibrationKey, CalibrationStore as _, Thresholds};
use tkm_decide::client::{NoDecisionReason, Outcome};
use tkm_proto::Host;
use tkmd::settings::{LoopMode, Settings};

fn calibrate(daemon: &tkmd::daemon::Daemon) {
    daemon.calibration().put(
        CalibrationKey {
            model_version: MODEL.into(),
            purpose: "loop_guard".into(),
        },
        Thresholds::default(),
    );
}

fn redirect_settings() -> Settings {
    Settings {
        loop_mode: LoopMode::Redirect,
        ..Settings::default()
    }
}

/// Runs `n` failing calls and returns the replies.
fn run_calls(daemon: &tkmd::daemon::Daemon, n: usize) -> Vec<tkm_proto::Reply> {
    (0..n)
        .map(|_| daemon.handle_tool_done(&failing_tool_done()))
        .collect()
}

#[tokio::test]
async fn undelivered_redirect_does_not_leak_into_next_turn() {
    let env = Env::new();
    env.seed_envelope(FIXTURE_REPO, Host::Claude, &[5; 5]);
    let decider = ScriptedDecider::new(stuck_decision());
    let daemon = daemon_with(&env, redirect_settings(), decider);
    daemon
        .adapters()
        .register(Box::new(ToolBlind(tkm_hosts::claude::ClaudeAdapter::new())));
    daemon.start_workers();
    calibrate(&daemon);
    daemon.handle(session_start()).await;
    daemon.handle(prompt()).await;
    run_calls(&daemon, 18);
    daemon.settle().await;
    assert_eq!(daemon.handle_tool_done(&failing_tool_done()).stdout, "");
    daemon.handle(stop()).await;
    assert_eq!(daemon.handle(prompt()).await.stdout, "");
}

#[tokio::test]
async fn stuck_sequence_opens_gate_judges_once_and_redirects_on_a_later_call() {
    let env = Env::new();
    env.seed_envelope(FIXTURE_REPO, Host::Claude, &[5; 5]);
    let decider = ScriptedDecider::new(stuck_decision());
    let daemon = daemon_with(&env, redirect_settings(), decider.clone());
    daemon.start_workers();
    calibrate(&daemon);
    daemon.handle(session_start()).await;
    daemon.handle(prompt()).await;

    let early = run_calls(&daemon, 17);
    assert!(
        early.iter().all(|r| r.stdout.is_empty()),
        "gate closed before 18 calls"
    );
    daemon.settle().await;
    assert_eq!(decider.calls(), 0);

    // Call 18 opens the gate: one judge request, the reply stays empty (no network wait).
    let opening = daemon.handle_tool_done(&failing_tool_done());
    assert_eq!(opening.stdout, "");
    daemon.settle().await;
    assert_eq!(
        decider.calls(),
        1,
        "one decision in flight, at most one per 5 calls"
    );

    // The verdict is delivered on the next tool_done.
    let delivered = daemon.handle_tool_done(&failing_tool_done());
    let context = additional_context(&delivered).expect("redirect injected as additionalContext");
    assert!(context.starts_with("tokenimum: the last"), "{context}");
    // Only one redirect per turn: later calls inject nothing more.
    let rest = run_calls(&daemon, 3);
    assert!(rest.iter().all(|r| additional_context(r).is_none()));
    daemon.flush_now().await;
    let store = tkm_store::open_read_only(&env.paths.db).unwrap();
    let acted: i64 = store
        .query_row(
            "SELECT COUNT(*) FROM decisions WHERE shadow = 0 AND purpose = 'loop_guard'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(acted, 1);
    let redirects: i64 = store
        .query_row(
            "SELECT COUNT(*) FROM events WHERE kind = 'loop.redirect'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(redirects, 1);
    let (at, ids): (Option<i64>, String) = store
        .query_row(
            "SELECT created_at, evidence_ids FROM events WHERE kind='loop.redirect'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .unwrap();
    assert!(at.is_some());
    let ids: Vec<i64> = serde_json::from_str(&ids).unwrap();
    assert!(!ids.is_empty());
    let (repo, session, digest): (Option<String>, Option<i64>, Option<String>) = store
        .query_row(
            "SELECT repo_key, session_id, digest FROM decisions WHERE id=?1",
            [ids[0]],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .unwrap();
    assert_eq!(repo.as_deref(), Some(FIXTURE_REPO));
    assert!(session.is_some() && digest.is_some());
}

#[tokio::test]
async fn verdict_is_dropped_when_the_turn_ends_first() {
    let env = Env::new();
    env.seed_envelope(FIXTURE_REPO, Host::Claude, &[5; 5]);
    let (decider, gate) = ScriptedDecider::gated(stuck_decision());
    let daemon = daemon_with(&env, redirect_settings(), decider.clone());
    daemon.start_workers();
    calibrate(&daemon);
    daemon.handle(session_start()).await;
    daemon.handle(prompt()).await;
    run_calls(&daemon, 18);
    assert!(
        eventually(5, || decider.calls() == 1).await,
        "judge started"
    );

    // The turn ends while the decision is in flight; then the verdict arrives.
    daemon.handle(stop()).await;
    gate.notify_waiters();
    gate.notify_one();
    daemon.settle().await;

    // A new turn starts: nothing from the old verdict is injected.
    daemon.handle(prompt()).await;
    let replies = run_calls(&daemon, 3);
    assert!(
        replies.iter().all(|r| r.stdout.is_empty()),
        "dropped verdict must not surface"
    );
}

#[tokio::test]
async fn shadow_mode_records_decision_and_event_and_injects_nothing() {
    let env = Env::new();
    env.seed_envelope(FIXTURE_REPO, Host::Claude, &[5; 5]);
    let decider = ScriptedDecider::new(stuck_decision());
    let settings = Settings {
        loop_mode: LoopMode::Shadow,
        ..Settings::default()
    };
    let daemon = daemon_with(&env, settings, decider.clone());
    daemon.start_workers();
    calibrate(&daemon); // calibrated, but the configured mode is shadow
    daemon.handle(session_start()).await;
    daemon.handle(prompt()).await;
    let first = run_calls(&daemon, 18);
    daemon.settle().await;
    let rest = run_calls(&daemon, 4);
    assert!(
        first.iter().chain(rest.iter()).all(|r| r.stdout.is_empty()),
        "nothing injected"
    );
    assert_eq!(decider.calls(), 1);

    daemon.flush_now().await;
    let store = tkm_store::open_read_only(&env.paths.db).unwrap();
    let shadow: i64 = store
        .query_row(
            "SELECT COUNT(*) FROM decisions WHERE shadow = 1 AND purpose = 'loop_guard'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(shadow, 1);
    let events: i64 = store
        .query_row(
            "SELECT COUNT(*) FROM events WHERE kind LIKE 'loop.%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(events >= 1);
}

#[tokio::test]
async fn balanced_preset_is_shadow_until_the_model_is_calibrated() {
    let env = Env::new();
    env.seed_envelope(FIXTURE_REPO, Host::Claude, &[5; 5]);
    let decider = ScriptedDecider::new(stuck_decision());
    // Redirect mode, but no calibration row for the answering model.
    let daemon = daemon_with(&env, redirect_settings(), decider.clone());
    daemon.start_workers();
    daemon.handle(session_start()).await;
    daemon.handle(prompt()).await;
    run_calls(&daemon, 18);
    daemon.settle().await;
    let rest = run_calls(&daemon, 3);
    assert!(rest.iter().all(|r| r.stdout.is_empty()));
    daemon.flush_now().await;
    let store = tkm_store::open_read_only(&env.paths.db).unwrap();
    let shadow: i64 = store
        .query_row("SELECT COUNT(*) FROM decisions WHERE shadow = 1", [], |r| {
            r.get(0)
        })
        .unwrap();
    assert_eq!(shadow, 1);
}

#[tokio::test]
async fn no_decision_releases_the_job_and_nothing_is_injected() {
    let env = Env::new();
    env.seed_envelope(FIXTURE_REPO, Host::Claude, &[5; 5]);
    let decider = ScriptedDecider::new(Outcome::NoDecision(NoDecisionReason::AllFailed));
    let daemon = daemon_with(&env, redirect_settings(), decider.clone());
    daemon.start_workers();
    daemon.handle(session_start()).await;
    daemon.handle(prompt()).await;
    run_calls(&daemon, 18);
    daemon.settle().await;
    let rest = run_calls(&daemon, 3);
    assert!(rest.iter().all(|r| r.stdout.is_empty()));
    assert!(decider.calls() >= 1);
}

#[tokio::test]
async fn without_an_envelope_the_gate_never_asks_for_a_judge() {
    let env = Env::new();
    let decider = ScriptedDecider::new(stuck_decision());
    let daemon = daemon_with(&env, redirect_settings(), decider.clone());
    daemon.start_workers();
    daemon.handle(session_start()).await;
    daemon.handle(prompt()).await;
    run_calls(&daemon, 30);
    daemon.settle().await;
    assert_eq!(decider.calls(), 0);
}

#[tokio::test]
async fn turn_end_feeds_the_envelope_for_good_turns() {
    let env = Env::new();
    let daemon = daemon_with(
        &env,
        redirect_settings(),
        ScriptedDecider::new(stuck_decision()),
    );
    daemon.start_workers();
    daemon.handle(session_start()).await;
    for _ in 0..5 {
        daemon.handle(prompt()).await;
        for _ in 0..3 {
            daemon.handle_tool_done(&tool_done());
        }
        daemon.handle(stop()).await;
    }
    daemon.flush_now().await;
    let samples = env
        .store()
        .envelope_samples(&tkm_core::model::RepoKey(FIXTURE_REPO.into()), Host::Claude)
        .unwrap();
    assert_eq!(samples, vec![3, 3, 3, 3, 3]);
    let turns: i64 = tkm_store::open_read_only(&env.paths.db)
        .unwrap()
        .query_row("SELECT COUNT(*) FROM turns", [], |r| r.get(0))
        .unwrap();
    assert_eq!(turns, 5);
}

#[tokio::test]
async fn second_stuck_segment_after_the_redirect_budget_escalates_with_a_notice() {
    let env = Env::new();
    env.seed_envelope(FIXTURE_REPO, Host::Claude, &[5; 5]);
    let decider = ScriptedDecider::new(stuck_decision());
    let daemon = daemon_with(&env, redirect_settings(), decider.clone());
    daemon.start_workers();
    calibrate(&daemon);
    daemon.handle(session_start()).await;
    daemon.handle(prompt()).await;
    let mut all = String::new();
    for _ in 0..80 {
        let reply = daemon.handle_tool_done(&failing_tool_done());
        all.push_str(&reply.stdout);
        daemon.settle().await;
    }
    assert!(
        all.contains("tokenimum: the last"),
        "first stuck verdict redirects"
    );
    assert!(
        all.contains("systemMessage") && all.contains("Esc"),
        "second one escalates with a notice naming the stop key: {all}"
    );
    daemon.flush_now().await;
    let store = tkm_store::open_read_only(&env.paths.db).unwrap();
    let escalations: i64 = store
        .query_row(
            "SELECT COUNT(*) FROM events WHERE kind = 'loop.escalation'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(escalations, 1);
    let _ = Arc::strong_count(&decider);
}
