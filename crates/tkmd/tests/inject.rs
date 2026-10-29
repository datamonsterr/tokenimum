//! E4.4 inject layer: candidates, relevance scoring, timeout fallback, deferral, cache.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]

mod common;

use std::collections::BTreeMap;
use std::sync::Arc;

use common::*;
use tkm_decide::api::Answer;
use tkm_decide::client::{Decision, Outcome};
use tkm_proto::{EventKind, ReplyAction};
use tkmd::decider::NullDecider;
use tkmd::settings::Settings;

fn relevance(scores: &[(&str, f64)]) -> Outcome {
    let answers: BTreeMap<String, Answer> = scores
        .iter()
        .map(|(id, score)| {
            (
                format!("rel_{id}"),
                Answer::Score {
                    score: *score,
                    confidence: None,
                    legend: None,
                    probabilities: None,
                },
            )
        })
        .collect();
    Outcome::Decision(Decision {
        model_version: MODEL.into(),
        requested_model: MODEL.into(),
        provider: None,
        answers,
        usage: tkm_decide::api::Usage::default(),
        latency_ms: 5,
        cached: false,
    })
}

fn calibrated_relevance(env: &Env) -> Settings {
    env.store().upsert_calibration(&tkm_store::CalibrationRecord {
        model_version: MODEL.into(), purpose: "relevance".into(),
        thresholds: serde_json::json!({"t_stuck":0.85,"t_progress":0.3,"t_low":1.0/3.0,"t_high":0.5}),
        sample_count:200, precision:0.95, recall:0.6,
    }).unwrap();
    Settings {
        shadow_days: 0,
        shadow_turns: 0,
        ..Settings::default()
    }
}

#[tokio::test]
async fn prompt_injects_the_default_block_when_no_decision_is_available() {
    let env = Env::new();
    seed_fact(&env, "example.com");
    let daemon = daemon_with(&env, Settings::default(), Arc::new(NullDecider));
    let start = daemon.handle(session_start()).await;
    assert_eq!(start.stdout, "", "session_start has nothing pending");
    let reply = daemon.handle(prompt()).await;
    let text = additional_context(&reply).expect("block injected");
    assert_eq!(text, format!("# Context\n{}\n", fact_line("example.com")));
    // Same session: the block is not repeated.
    let again = daemon.handle(prompt()).await;
    assert_eq!(again.stdout, "");
}

#[tokio::test]
async fn relevance_scores_drop_candidates_below_the_threshold() {
    let env = Env::new();
    seed_fact(&env, "example.com");
    seed_fact(&env, "other.org");
    let ids_scores = relevance(&[("boundary:example.com", 2.5), ("boundary:other.org", 0.5)]);
    let decider = ScriptedDecider::new(ids_scores);
    let daemon = daemon_with(&env, calibrated_relevance(&env), decider.clone());
    let reply = daemon.handle(prompt()).await;
    let text = additional_context(&reply).unwrap();
    assert!(text.contains("example.com"));
    assert!(!text.contains("other.org"));
    assert_eq!(decider.calls(), 1);
}

#[tokio::test]
async fn timeout_falls_back_to_the_cached_default_block() {
    let env = Env::new();
    seed_fact(&env, "example.com");
    // The decider never answers: the gate is never released.
    let (decider, _gate) = ScriptedDecider::gated(relevance(&[]));
    let settings = Settings {
        relevance_timeout_ms: 150,
        ..Settings::default()
    };
    let daemon = daemon_with(&env, settings, decider.clone());
    let started = std::time::Instant::now();
    let reply = daemon.handle(prompt()).await;
    assert!(
        started.elapsed() < std::time::Duration::from_millis(1_400),
        "prompt answered inside its read timeout"
    );
    let text = additional_context(&reply).expect("default block on timeout");
    assert_eq!(text, format!("# Context\n{}\n", fact_line("example.com")));
    assert_eq!(decider.calls(), 1);
}

#[tokio::test]
async fn identical_prompt_and_candidates_hit_the_cache() {
    let env = Env::new();
    seed_fact(&env, "example.com");
    let decider = ScriptedDecider::new(relevance(&[("boundary:example.com", 3.0)]));
    let daemon = daemon_with(&env, Settings::default(), decider.clone());
    daemon.handle(prompt()).await;
    daemon.handle(session_end()).await; // new session state, same repo and prompt
    daemon.handle(prompt()).await;
    assert_eq!(
        decider.calls(),
        1,
        "second identical request served from the 10-minute cache"
    );
}

#[tokio::test]
async fn memory_pointer_lines_join_the_candidates_when_enabled() {
    let env = Env::new();
    let repo = env.dir.path().join("repo");
    std::fs::create_dir_all(repo.join("memory")).unwrap();
    std::fs::write(
        repo.join("MEMORY.md"),
        "- [Build](build.md) — use `make ci` before pushing\n",
    )
    .unwrap();
    let mut payload = fixture("UserPromptSubmit");
    payload["cwd"] = serde_json::json!(repo.to_string_lossy());
    let daemon = daemon_with(&env, Settings::default(), Arc::new(NullDecider));
    let reply = daemon
        .handle(request(EventKind::Prompt, payload.clone()))
        .await;
    assert!(additional_context(&reply).unwrap().contains("make ci"));

    let env2 = Env::new();
    let mut off = Settings::default();
    off.inject.memory = false;
    let daemon = daemon_with(&env2, off, Arc::new(NullDecider));
    let reply = daemon.handle(request(EventKind::Prompt, payload)).await;
    assert_eq!(reply.stdout, "");
}

#[tokio::test]
async fn block_moves_to_the_next_injectable_event_when_the_prompt_cannot_inject() {
    // Cursor's beforeSubmitPrompt cannot inject: its adapter renders `{}`.
    // Claude is used here with a Cursor-shaped request, so the registry gets a prompt-blind adapter.
    let env = Env::new();
    seed_fact(&env, "example.com");
    let daemon = daemon_with(&env, Settings::default(), Arc::new(NullDecider));
    daemon.adapters().register(Box::new(PromptBlind(
        tkm_hosts::claude::ClaudeAdapter::new(),
    )));
    let prompt_reply = daemon.handle(prompt()).await;
    assert_eq!(prompt_reply.stdout, "", "prompt cannot inject");
    // The pending block rides on the next tool_done (PostToolUse can inject).
    let reply = daemon.handle_tool_done(&tool_done());
    let text = additional_context(&reply).expect("pending block delivered on tool_done");
    assert!(text.contains("example.com"));
    assert!(matches!(reply.action, ReplyAction::Inject { .. }));
    // Delivered once.
    assert_eq!(daemon.handle_tool_done(&tool_done()).stdout, "");
}

#[tokio::test]
async fn cursor_repeats_session_start_context_on_next_tool_result() {
    let env = Env::new();
    seed_fact(&env, "example.com");
    let daemon = daemon_with(&env, Settings::default(), Arc::new(NullDecider));
    let cursor = |name: &str, kind: EventKind| {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/cursor/hooks")
            .join(format!("{name}.json"));
        let mut payload: serde_json::Value =
            serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        payload["workspace_roots"] = serde_json::json!(["~/dev/example"]);
        let mut req = request(kind, payload);
        req.host = tkm_proto::Host::Cursor;
        req
    };
    daemon
        .handle(cursor("beforeSubmitPrompt", EventKind::Prompt))
        .await;
    let start = daemon
        .handle(cursor("sessionStart", EventKind::SessionStart))
        .await;
    let first: serde_json::Value = serde_json::from_str(&start.stdout).unwrap();
    assert!(
        first["additional_context"]
            .as_str()
            .unwrap()
            .contains("example.com")
    );
    let tool = cursor("postToolUse", EventKind::ToolDone);
    let repeated = daemon.handle_tool_done(&tool);
    let second: serde_json::Value = serde_json::from_str(&repeated.stdout).unwrap();
    assert_eq!(first["additional_context"], second["additional_context"]);
    assert_eq!(daemon.handle_tool_done(&tool).stdout, "{}");
}

#[tokio::test]
async fn relevance_stays_shadowed_without_its_own_qualifying_calibration() {
    for precision in [0.0, 0.899] {
        let env = Env::new();
        seed_fact(&env, "example.com");
        seed_fact(&env, "other.org");
        env.store().upsert_calibration(&tkm_store::CalibrationRecord {
            model_version:MODEL.into(), purpose:"relevance".into(),
            thresholds:serde_json::json!({"t_stuck":0.85,"t_progress":0.3,"t_low":0.3,"t_high":0.8}),
            sample_count:200, precision, recall:0.6,
        }).unwrap();
        let daemon = daemon_with(
            &env,
            Settings {
                shadow_days: 0,
                shadow_turns: 0,
                ..Settings::default()
            },
            ScriptedDecider::new(relevance(&[
                ("boundary:example.com", 2.5),
                ("boundary:other.org", 0.0),
            ])),
        );
        let reply = daemon.handle(prompt()).await;
        let text = additional_context(&reply).unwrap();
        assert!(text.contains("other.org"), "shadow uses code-only context");
        daemon.flush_now().await;
        let rows = env.store().list_recent_decisions(-1).unwrap();
        assert!(
            rows.iter()
                .any(|row| row.purpose == "relevance" && row.shadow)
        );
    }
}
