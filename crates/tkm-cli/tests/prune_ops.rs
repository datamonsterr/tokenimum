#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
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
