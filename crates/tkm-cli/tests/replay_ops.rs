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
