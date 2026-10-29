#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod support_ops;
#[test]
fn calibrate_too_few_labels() {
    let s = support_ops::Sandbox::new();
    s.seed_db();
    let out = s.run(&["calibrate", "--purpose", "loop"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("too few"));
}
