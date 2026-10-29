//! SPEC 16: all native hook fixtures -> adapters -> fold and mocked loop decisions.
//! Expected actions are reviewable JSON. TKM_BLESS=1 regenerates expectations from inputs;
//! fixture discovery never reads the expectations (and can bootstrap a missing snapshot).
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]

mod common;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use common::*;
use serde_json::{Value, json};
use tkm_core::facts::{FactsConfig, FactsState};
use tkm_core::loopguard::{
    EscalationMode, JudgeScores, JudgeThresholds, JudgeVerdict, LoopAction, LoopConfig,
    RedirectKind, TurnGuard,
};
use tkm_core::model::{Session, Turn};
use tkm_proto::{EventKind, Host, PROTOCOL_VERSION, ReplyAction, Request};
use tkmd::fold::replay_fold;
use tkmd::registry::AdapterRegistry;
use tkmd::settings::Settings;

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
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
        _ => panic!("unknown host: {host}"),
    }
}

fn native(payload: &Value, name: &str) -> String {
    payload
        .get("hook_event_name")
        .or_else(|| payload.get("hook"))
        .and_then(Value::as_str)
        .or_else(|| payload.get("event").and_then(Value::as_str))
        .unwrap_or_else(|| name.split('-').next().unwrap())
        .to_owned()
}

fn request_for(host: Host, path: &Path) -> Request {
    let payload: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
    let name = path.file_stem().unwrap().to_str().unwrap();
    let host_event = if host == Host::Agy && name == "loop" {
        "PostToolUse".to_owned()
    } else if name == "session_idle.synthetic" {
        "session_idle".to_owned()
    } else {
        native(&payload, name)
    };
    let event_name = if host_event == "event" {
        payload
            .pointer("/event/type")
            .and_then(Value::as_str)
            .unwrap()
    } else {
        &host_event
    };
    let event = match event_name {
        "SessionStart" | "sessionStart" | "session_start" | "session.created" => {
            EventKind::SessionStart
        }
        "UserPromptSubmit" | "PreInvocation" | "beforeSubmitPrompt" | "message_submit"
        | "chat.message" | "before_agent_start" => EventKind::Prompt,
        "Stop" | "stop" | "turn_end" | "agent_end" | "session.idle" | "session_idle" => {
            EventKind::TurnEnd
        }
        "SessionEnd" | "sessionEnd" | "session_end" | "session_shutdown" | "session.deleted" => {
            EventKind::SessionEnd
        }
        _ => EventKind::ToolDone,
    };
    // session_idle's captured transition uses environment markers rather than an id in stdin.
    let env = if name == "session_idle.synthetic" {
        BTreeMap::from([("CODEWHALE_SESSION_ID".into(), "golden-idle".into())])
    } else {
        BTreeMap::new()
    };
    Request {
        version: PROTOCOL_VERSION,
        host,
        event,
        host_event: Some(host_event),
        payload,
        env,
        cwd: Some("/synthetic/repo".into()),
    }
}

async fn replay(host_name: &str) -> Value {
    let host = host_variant(host_name);
    let adapter = AdapterRegistry::standard().get(host);
    let mut paths: Vec<_> =
        std::fs::read_dir(root().join("fixtures").join(host_name).join("hooks"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| path.extension().is_some_and(|ext| ext == "json"))
            .collect();
    paths.sort();
    assert!(!paths.is_empty(), "no fixtures for {host_name}");
    let mut hooks = Vec::new();
    for path in paths {
        let request = request_for(host, &path);
        let mapped = adapter.map_event(&request).unwrap();
        let mut fixture_fold_ids = Vec::new();
        if let Some(event) = &mapped {
            let mut session = Session::new(host);
            session.session_id.clone_from(&event.session_id);
            session.turns.push(Turn {
                calls: event.call.iter().cloned().collect(),
                ..Turn::default()
            });
            let fold = replay_fold(
                &session,
                FactsState::default(),
                &BTreeSet::new(),
                &FactsConfig::default(),
            );
            fixture_fold_ids = fold.facts.facts.keys().cloned().collect();
            assert_eq!(fold.facts.processed.len(), 1);
        }
        let env = Env::new();
        let mock = ScriptedDecider::new(stuck_decision());
        let daemon = daemon_with(&env, Settings::default(), mock);
        let reply = daemon.handle(request.clone()).await;
        hooks.push(json!({"fixture": path.file_name().unwrap().to_str().unwrap(), "event": request.event.as_str(), "mapped_event": mapped.as_ref().map(|event| event.kind.as_str()), "fold_fact_ids": fixture_fold_ids, "action": reply.action, "stdout": reply.stdout}));
        daemon.shutdown().await;
    }

    let request = request_for(
        host,
        &root()
            .join("fixtures")
            .join(host_name)
            .join("replay/loop.json"),
    );
    let event = adapter
        .map_event(&request)
        .unwrap()
        .expect("loop fixture must map");
    let call = event.call.expect("loop fixture must contain a call");
    let mut guard = TurnGuard::new(EscalationMode::DelegatedNoAdapter);
    let mut actions = Vec::new();
    let mut calls = Vec::new();
    for index in 1..=19 {
        calls.push(call.clone());
        for action in guard.on_tool_done(&call, &LoopConfig::default(), Some(100.0)) {
            match action {
                LoopAction::RequestJudge { request } => {
                    // Mock a calibrated stuck answer, retaining real correlation and delivery.
                    let verdict = guard.on_judge_verdict(
                        request,
                        JudgeScores {
                            stuck: 0.95,
                            progress: 0.1,
                        },
                        JudgeThresholds {
                            t_stuck: 0.85,
                            t_progress: 0.3,
                            t_low: 0.3,
                        },
                        RedirectKind::RereadError,
                    );
                    assert_eq!(verdict, JudgeVerdict::Stuck);
                    actions.push(
                        json!({"call": index, "action": "request_judge", "verdict": verdict}),
                    );
                }
                LoopAction::Redirect {
                    redirect_kind,
                    text,
                } => {
                    let reply_action = ReplyAction::Inject { text };
                    actions.push(json!({"call": index, "action": reply_action, "redirect": redirect_kind, "stdout": adapter.render_reply(&request, &reply_action)}));
                }
                other => panic!("unexpected action: {other:?}"),
            }
        }
    }
    assert_eq!(
        actions.len(),
        2,
        "positive mocked decision coverage for {host_name}"
    );
    let mut session = Session::new(host);
    session.session_id = event.session_id;
    session.turns.push(Turn {
        calls,
        ..Turn::default()
    });
    let folded = replay_fold(
        &session,
        FactsState::default(),
        &BTreeSet::new(),
        &FactsConfig::default(),
    );
    json!({"hooks": hooks, "mocked_loop": actions, "fold_fact_ids": folded.facts.facts.keys().collect::<Vec<_>>()})
}

async fn check(host: &str) {
    let actual = replay(host).await;
    let path = root().join("fixtures/golden").join(format!("{host}.json"));
    if std::env::var("TKM_BLESS").is_ok_and(|value| value == "1") {
        std::fs::write(path, serde_json::to_string_pretty(&actual).unwrap() + "\n").unwrap();
    } else {
        let expected: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(actual, expected, "{host}: review changes with TKM_BLESS=1");
    }
}

#[tokio::test]
async fn golden_replay_claude() {
    check("claude").await;
}
#[tokio::test]
async fn golden_replay_codex() {
    check("codex").await;
}
#[tokio::test]
async fn golden_replay_agy() {
    check("agy").await;
}
#[tokio::test]
async fn golden_replay_cursor() {
    check("cursor").await;
}
#[tokio::test]
async fn golden_replay_kimi() {
    check("kimi").await;
}
#[tokio::test]
async fn golden_replay_codewhale() {
    check("codewhale").await;
}
#[tokio::test]
async fn golden_replay_opencode() {
    check("opencode").await;
}
#[tokio::test]
async fn golden_replay_pi() {
    check("pi").await;
}
