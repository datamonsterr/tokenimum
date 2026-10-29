#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]
mod support_report;
use support_report::Fixture;

fn seed_digest(f: &Fixture) {
    let redactor = tkm_decide::redact::Redactor::new(tkm_decide::redact::RedactContext {
        repo_root: f.home.path().to_path_buf(),
        home: f.home.path().to_path_buf(),
        env: vec![],
    });
    let digest = redactor.redact("sk-or-v1-fake-token /outside/secret.txt");
    f.store()
        .insert_decision_with_context(
            &tkm_store::NewDecision {
                model: "fixture".into(),
                purpose: "loop_guard".into(),
                questions: serde_json::json!({}),
                answers: serde_json::json!({}),
                cost_usd: 0.0,
                latency_ms: 1,
                shadow: true,
            },
            Some(tkm_core::model::Timestamp(1)),
            Some(&f.repo),
            None,
            Some(&digest),
        )
        .unwrap();
}

#[test]
fn export_show_digests_redacted() {
    let f = Fixture::new();
    seed_digest(&f);
    let out = f.run(&["export", "--show-digests"]);
    assert!(out.status.success(), "{out:?}");
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("[redacted:openrouter]"));
    assert!(text.contains("[outside-repo]"));
    assert!(!text.contains("sk-or-v1-fake-token"));
    assert!(!text.contains("/outside/secret.txt"));
}
#[test]
fn export_omits_digests_by_default() {
    let f = Fixture::new();
    seed_digest(&f);
    let out = f.run(&["export"]);
    assert!(out.status.success(), "{out:?}");
    let json: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(!json.to_string().contains("digest"));
}
#[test]
fn export_never_contains_key() {
    let f = Fixture::new();
    let out = f.run(&["export", "--show-digests"]);
    assert!(out.status.success(), "{out:?}");
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(!text.contains("OPENROUTER_API_KEY"));
    assert!(!text.contains("sk-or-v1-fixture-secret-123"));
}

#[test]
fn export_redacts_legacy_raw_digest_and_excludes_other_repositories() {
    let f = Fixture::new();
    let store = f.store();
    let decision = tkm_store::NewDecision {
        model: "fixture".into(),
        purpose: "relevance".into(),
        questions: serde_json::json!({}),
        answers: serde_json::json!({}),
        cost_usd: 0.0,
        latency_ms: 1,
        shadow: false,
    };
    store
        .insert_decision_with_context(
            &decision,
            Some(tkm_core::model::Timestamp(1)),
            Some(&f.repo),
            None,
            Some("sk-or-v1-raw-fixture /outside/private.txt mail@example.invalid"),
        )
        .unwrap();
    store
        .insert_decision_with_context(
            &decision,
            Some(tkm_core::model::Timestamp(1)),
            Some(&tkm_core::model::RepoKey("fixture/other".into())),
            None,
            Some("other-repo-only"),
        )
        .unwrap();
    let out = f.run(&["export", "--show-digests"]);
    assert!(out.status.success(), "{out:?}");
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(
        !text.contains("sk-or-v1-raw-fixture")
            && !text.contains("/outside/private.txt")
            && !text.contains("other-repo-only")
    );
    assert!(
        text.contains("[redacted:openrouter]")
            && text.contains("[outside-repo]")
            && text.contains("mail@example.invalid")
    );
    let out = f.run(&["export"]);
    assert!(out.status.success(), "{out:?}");
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(!text.contains("digest"));
}
