#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]
mod support_ops;
use tkm_core::model::Host;

fn seed(s: &support_ops::Sandbox) {
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
            display_name: "test".into(),
            first_seen: tkm_core::model::Timestamp(0),
            last_seen: tkm_core::model::Timestamp(0),
            delegated_runs: 0,
        })
        .unwrap();
    for name in ["Skill", "mcp__example__unused"] {
        for _ in 0..10 {
            store
                .record_tool_unused_prompt(&repo.key, Host::Claude, name)
                .unwrap();
        }
        for _ in 0..2 {
            store
                .record_tool_unused_session(&repo.key, Host::Claude, name)
                .unwrap();
        }
    }
}
#[test]
fn prune_never_proposes_protected() {
    let s = support_ops::Sandbox::new();
    seed(&s);
    let out = s.run(&["prune"]);
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("mcp__example__unused"));
    assert!(!text.contains("Skill"));
}
#[test]
fn prune_apply_then_restore_leaves_foreign_entries() {
    let s = support_ops::Sandbox::new();
    seed(&s);
    std::fs::create_dir_all(s.repo.join(".claude")).unwrap();
    let settings = s.repo.join(".claude/settings.local.json");
    std::fs::write(&settings, r#"{"permissions":{"deny":["Foreign"]}}"#).unwrap();
    assert!(s.run(&["prune", "apply"]).status.success());
    assert!(
        std::fs::read_to_string(&settings)
            .unwrap()
            .contains("mcp__example__unused")
    );
    assert!(
        s.run(&["prune", "restore", "mcp__example__unused"])
            .status
            .success()
    );
    let contents = std::fs::read_to_string(settings).unwrap();
    assert!(contents.contains("Foreign"));
    assert!(!contents.contains("mcp__example__unused"));
    assert!(s.run(&["prune", "restore"]).status.success());
}
#[test]
fn prune_unsupported_host_reports_proposal_only() {
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
            display_name: "test".into(),
            first_seen: tkm_core::model::Timestamp(0),
            last_seen: tkm_core::model::Timestamp(0),
            delegated_runs: 0,
        })
        .unwrap();
    for _ in 0..5 {
        store
            .record_tool_unused_session(&repo.key, Host::Codex, "example")
            .unwrap();
    }
    let out = s.run(&["prune", "apply"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("proposal only"));
    assert!(store.list_denylist(&s.repo).unwrap().is_empty());
}

#[test]
fn prune_apply_one_and_restore_all_four_hosts() {
    let s = support_ops::Sandbox::new();
    seed(&s);
    let paths = tkm_config::paths::Paths::from_dirs(
        s.home.path(),
        Some(&s.home.path().join("data")),
        Some(&s.home.path().join("config")),
        Some(&s.home.path().join("run")),
    );
    let store = tkm_store::Store::open(paths.db).unwrap();
    let repo = tkm_config::repo::repo_identity(&s.repo, "", Some(s.home.path()));
    for host in [Host::Claude, Host::Cursor, Host::Opencode, Host::Pi] {
        for tool in ["mcp__example__unused", "mcp__example__other"] {
            for _ in 0..5 {
                store
                    .record_tool_unused_session(&repo.key, host, tool)
                    .unwrap();
            }
        }
    }
    let out = s.run(&["prune", "apply", "mcp__example__unused"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    for (relative, token) in [
        (".claude/settings.local.json", "mcp__example__unused"),
        (".cursor/cli.json", "Mcp(example:unused)"),
        ("opencode.json", "mcp__example__unused"),
        (".pi/settings.json", "-mcp__example__unused"),
    ] {
        let path = s.repo.join(relative);
        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.contains(token));
        assert!(!contents.contains("other"));
    }
    assert_eq!(
        store
            .list_denylist(&s.repo)
            .unwrap()
            .iter()
            .filter(|entry| !entry.restored)
            .count(),
        4
    );
    assert!(
        s.run(&["prune", "restore", "mcp__example__unused"])
            .status
            .success()
    );
    assert!(
        store
            .list_denylist(&s.repo)
            .unwrap()
            .iter()
            .all(|entry| entry.restored)
    );
    assert!(s.run(&["prune", "apply"]).status.success());
    assert!(s.run(&["prune", "restore"]).status.success());
    assert!(
        store
            .list_denylist(&s.repo)
            .unwrap()
            .iter()
            .all(|entry| entry.restored)
    );
}

#[test]
fn prune_honors_user_protection_and_disabled_orchestrator() {
    let s = support_ops::Sandbox::new();
    seed(&s);
    std::fs::write(
        s.repo.join(".tokenimum.toml"),
        "[prune]\nprotect = ['mcp__example__*']\n[integrations]\nsuperorchestrator = 'off'\n",
    )
    .unwrap();
    let paths = tkm_config::paths::Paths::from_dirs(
        s.home.path(),
        Some(&s.home.path().join("data")),
        Some(&s.home.path().join("config")),
        Some(&s.home.path().join("run")),
    );
    let store = tkm_store::Store::open(paths.db).unwrap();
    let repo = tkm_config::repo::repo_identity(&s.repo, "", Some(s.home.path()));
    for _ in 0..5 {
        store
            .record_tool_unused_session(
                &repo.key,
                Host::Claude,
                "mcp__superorchestrator__spawn_agent",
            )
            .unwrap();
    }
    let out = s.run(&["prune"]);
    assert!(out.status.success());
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(!text.contains("mcp__example__unused"));
    assert!(text.contains("mcp__superorchestrator__spawn_agent"));
}
