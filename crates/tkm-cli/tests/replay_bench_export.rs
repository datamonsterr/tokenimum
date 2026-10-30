#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]
mod support_report;
use support_report::Fixture;

#[test]
fn export_activity_counts_only_current_repository_redirects_and_completed_folds() {
    let f = Fixture::new();
    let store = f.store();
    for (index, repo) in [f.repo.clone(), tkm_core::model::RepoKey("other".into())]
        .into_iter()
        .enumerate()
    {
        store
            .upsert_repo(&tkm_store::Repo {
                repo_key: repo.clone(),
                display_name: "synthetic".into(),
                first_seen: tkm_core::model::Timestamp(1),
                last_seen: tkm_core::model::Timestamp(2),
                delegated_runs: 0,
            })
            .unwrap();
        let id = store
            .upsert_session(&tkm_store::NewSession {
                key: tkm_store::SessionKey {
                    host: tkm_core::model::Host::Claude,
                    agent_home: "/synthetic".into(),
                    session_id: format!("synthetic-{index}"),
                },
                repo_key: repo,
                cwd: f.home.path().into(),
                started: tkm_core::model::Timestamp(1),
                ended: Some(tkm_core::model::Timestamp(2)),
                fold_status: "folded".into(),
                shadow: false,
                integration_level: "hooks".into(),
            })
            .unwrap();
        store
            .insert_event(&tkm_store::NewEvent {
                kind: "loop.redirect".into(),
                session_id: id,
                inputs: serde_json::json!({}),
                evidence_ids: vec![],
                explanation_template_id: "synthetic".into(),
                params: serde_json::json!({}),
            })
            .unwrap();
    }
    let result = f.run(&["export"]);
    assert!(result.status.success());
    let value: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(
        value["activity"],
        serde_json::json!({"redirects":1,"folds":1})
    );
}
