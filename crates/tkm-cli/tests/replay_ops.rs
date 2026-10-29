#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod support_ops;
#[test]
fn replay_claude_fixture_prints_actions() {
    let s = support_ops::Sandbox::new();
    let fixture = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/claude/sessions/e928a2cf-e31b-4154-805b-919991d042ea.jsonl"
    );
    let out = s.run(&["replay", fixture]);
    assert!(out.status.success(), "{out:?}");
    assert!(!out.stdout.is_empty());
    assert!(!s.home.path().join("data/tokenimum/tokenimum.db").exists());
}
#[test]
fn replay_session_id_uses_recorded_claude_source() {
    use tkm_core::model::{Host, Timestamp};
    let s = support_ops::Sandbox::new();
    let paths = tkm_config::paths::Paths::from_dirs(
        s.home.path(),
        Some(&s.home.path().join("data")),
        Some(&s.home.path().join("config")),
        Some(&s.home.path().join("run")),
    );
    let store = tkm_store::Store::open(paths.db).unwrap();
    let repo = tkm_config::repo::repo_identity(&s.repo, "", Some(s.home.path()));
    store
        .upsert_repo(&tkm_store::Repo {
            repo_key: repo.key.clone(),
            display_name: "fixture".into(),
            first_seen: Timestamp(0),
            last_seen: Timestamp(0),
            delegated_runs: 0,
        })
        .unwrap();
    let agent = s.home.path().join("agent");
    let slug: String = s
        .repo
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let logs = agent.join("projects").join(slug);
    std::fs::create_dir_all(&logs).unwrap();
    std::fs::copy(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/claude/sessions/e928a2cf-e31b-4154-805b-919991d042ea.jsonl"
        ),
        logs.join("fixture.jsonl"),
    )
    .unwrap();
    store
        .upsert_session(&tkm_store::NewSession {
            key: tkm_store::SessionKey {
                host: Host::Claude,
                agent_home: agent.to_string_lossy().into_owned(),
                session_id: "fixture".into(),
            },
            repo_key: repo.key,
            cwd: s.repo.clone(),
            started: Timestamp(0),
            ended: None,
            fold_status: "pending".into(),
            shadow: false,
            integration_level: "hooks".into(),
        })
        .unwrap();
    let out = s.run(&["replay", "fixture"]);
    assert!(out.status.success(), "{out:?}");
    assert!(!out.stdout.is_empty());
}
