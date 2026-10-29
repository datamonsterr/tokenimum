#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]
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

#[test]
fn bench_replay_matches_committed_report_without_network() {
    let s = support_ops::Sandbox::new();
    let fixtures = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/bench");
    let report = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../docs/reports/m0-decision-bench.md"
    ))
    .unwrap();
    let first = s.run(&["replay", "--bench", fixtures]);
    let second = s.run(&["replay", "--bench", fixtures]);
    assert!(first.status.success(), "{first:?}");
    assert_eq!(first.stdout, report);
    assert_eq!(first.stdout, second.stdout);
    let text = String::from_utf8(first.stdout).unwrap();
    assert!(text.contains("50 sessions"));
    assert!(text.contains("Claude Code") && text.contains("Codex"));
    assert!(text.contains("precision") && text.contains("latency") && text.contains("cost"));
}

#[test]
fn bench_rejects_missing_cached_responses() {
    let s = support_ops::Sandbox::new();
    let out = s.run(&["replay", "--bench", s.repo.to_str().unwrap()]);
    assert!(!out.status.success());
}

#[test]
fn bench_rejects_cache_after_session_changes() {
    let s = support_ops::Sandbox::new();
    let source = std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/bench"));
    let destination = s.repo.join("bench");
    std::fs::create_dir_all(destination.join("sessions")).unwrap();
    std::fs::copy(source.join("cache.json"), destination.join("cache.json")).unwrap();
    for entry in std::fs::read_dir(source.join("sessions")).unwrap() {
        let entry = entry.unwrap();
        std::fs::copy(
            entry.path(),
            destination.join("sessions").join(entry.file_name()),
        )
        .unwrap();
    }
    let path = destination.join("sessions/claude-00.jsonl");
    let content = std::fs::read_to_string(&path).unwrap();
    std::fs::write(
        path,
        content.replace("example project", "different project"),
    )
    .unwrap();
    let out = s.run(&["replay", "--bench", destination.to_str().unwrap()]);
    assert!(!out.status.success(), "{out:?}");
}
