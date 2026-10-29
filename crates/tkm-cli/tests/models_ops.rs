#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod support_ops;
#[test]
fn models_probe_offline_skips() {
    let s = support_ops::Sandbox::new();
    s.seed_db();
    let out = s.run(&["models", "probe"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("skipped (offline)"));
}
#[test]
fn models_lists_chain() {
    let s = support_ops::Sandbox::new();
    s.seed_db();
    let out = s.run(&["models"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("mercury"));
}
