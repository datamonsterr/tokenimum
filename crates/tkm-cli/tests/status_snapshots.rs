#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::path::Path;
use tkm_cli::{
    cli::{StatusArgs, WhyArgs},
    cmd::{status, why},
    ctx::Ctx,
};
use tkm_config::{
    paths::Paths,
    repo::{IdentitySource, RepoIdentity},
};
use tkm_core::model::{Host, RepoKey, Timestamp};
use tkm_store::{NewEvent, NewSession, Repo, SessionKey, Store};

const NOW: i64 = 1_793_715_780_000; // 2026-11-03 14:23 UTC

fn snapshot(name: &str, actual: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/snapshots")
        .join(name);
    if std::env::var("TKM_UPDATE_SNAPSHOTS").as_deref() == Ok("1") {
        std::fs::write(&path, actual).unwrap();
    }
    let expected = std::fs::read_to_string(&path).unwrap();
    if expected != actual {
        let diff: String = expected
            .lines()
            .zip(actual.lines())
            .enumerate()
            .filter(|(_, (a, b))| a != b)
            .map(|(n, (a, b))| format!("line {}\n- {a}\n+ {b}\n", n + 1))
            .collect();
        panic!(
            "snapshot {name} differs (expected {} lines, actual {}):\n{diff}\nexpected:\n{expected}\nactual:\n{actual}",
            expected.lines().count(),
            actual.lines().count()
        );
    }
}

fn context<'a>(home: &Path, out: &'a mut Vec<u8>, err: &'a mut Vec<u8>) -> Ctx<'a> {
    let paths = Paths::from_dirs(home, None, None, None);
    let repo = RepoIdentity {
        root: home.into(),
        key: RepoKey("github.com/acme/api".into()),
        label: "api".into(),
        remote: None,
        how: IdentitySource::Dir,
    };
    let store = Store::open(&paths.db).unwrap();
    store
        .upsert_repo(&Repo {
            repo_key: repo.key.clone(),
            display_name: "api".into(),
            first_seen: Timestamp(NOW),
            last_seen: Timestamp(NOW),
            delegated_runs: 0,
        })
        .unwrap();
    let mut ctx = Ctx::new(paths, home.into(), repo, out, err);
    ctx.env = vec![("HOME".into(), home.display().to_string())];
    ctx
}

#[test]
fn status_text_snapshot() {
    status_snapshot(false, "status.txt");
}
#[test]
fn status_json_snapshot() {
    status_snapshot(true, "status.json");
}
fn status_snapshot(json: bool, name: &str) {
    let home = tempfile::tempdir().unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let mut ctx = context(home.path(), &mut out, &mut err);
    assert_eq!(
        status::run_at(&mut ctx, &StatusArgs { all: false, json }, NOW).unwrap(),
        std::process::ExitCode::SUCCESS
    );
    assert!(err.is_empty());
    snapshot(name, &String::from_utf8(out).unwrap());
}

#[test]
fn why_4812_snapshot() {
    let home = tempfile::tempdir().unwrap();
    let (mut out, mut err) = (Vec::new(), Vec::new());
    let mut ctx = context(home.path(), &mut out, &mut err);
    let store = Store::open(&ctx.paths.db).unwrap();
    let sid = store
        .upsert_session(&NewSession {
            key: SessionKey {
                host: Host::Claude,
                agent_home: "/fixture".into(),
                session_id: "s-1".into(),
            },
            repo_key: ctx.repo.key.clone(),
            cwd: home.path().into(),
            started: Timestamp(NOW - 120_000),
            ended: None,
            fold_status: "open".into(),
            shadow: false,
            integration_level: "hooks".into(),
        })
        .unwrap();
    let decision = store.insert_decision_with_context(&tkm_store::NewDecision { model: "inception/mercury-decide-20260930".into(), purpose: "fact_filter".into(), questions: serde_json::json!({}), answers: serde_json::json!({"stable_f-17":{"type":"noul","noul":0.94},"safe_f-17":{"type":"noul","noul":0.97}}), cost_usd: 0.000006, latency_ms: 1, shadow:false }, Some(Timestamp(NOW-60_000)), Some(&ctx.repo.key), Some(sid), None).unwrap();
    store
        .insert_candidate_fact(&tkm_store::FactRecord {
            repo_key: ctx.repo.key.clone(),
            fact_id: "f-17".into(),
            family: "form_repair".into(),
            scope: "neutral".into(),
            text_params: serde_json::json!({}),
            state: tkm_store::FactState::Candidate,
            score: 3.0,
            confirmations: 3,
            contradictions: 0,
            fire_count: 4,
            fire_sessions: 5,
            state_changed_at: None,
        })
        .unwrap();
    let mut event = NewEvent {
        kind: "hook.seen".into(),
        session_id: sid,
        inputs: serde_json::json!({}),
        evidence_ids: vec![],
        explanation_template_id: "observation".into(),
        params: serde_json::json!({}),
    };
    for _ in 1..4812 {
        store.insert_event(&event).unwrap();
    }
    event.kind = "fact.injected".into();
    event.explanation_template_id = "fact_injected".into();
    event.evidence_ids = vec![decision];
    event.inputs = serde_json::json!({"fact_ids":["f-17"]});
    event.params = serde_json::json!({"detail":"Run tests with `python -m pytest` (`pytest` fails here: ImportError).", "why":"in 4 of the last 5 sessions in this repository, `pytest` failed with an ImportError and `python -m pytest` then worked. The fact was confirmed in 3 sessions (needed: 3) and never contradicted.","model_checks":"", "cost":"", "undo":"tkm facts forget f-17", "event":""});
    assert_eq!(
        store
            .insert_event_at(&event, Timestamp(NOW - 60_000))
            .unwrap(),
        4812
    );
    why::run(
        &mut ctx,
        &WhyArgs {
            event: "4812".into(),
            session: None,
        },
    )
    .unwrap();
    assert!(err.is_empty());
    snapshot("why_4812.txt", &String::from_utf8(out).unwrap());
}
