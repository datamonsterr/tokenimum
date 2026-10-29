#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod support_ops;
use support_ops::Sandbox;

#[test]
fn config_set_get_roundtrip() {
    let s = Sandbox::new();
    assert!(
        s.run(&["config", "set", "loop.mode", "off", "--global"])
            .status
            .success()
    );
    let output = s.run(&["config", "get", "loop.mode"]);
    assert!(output.status.success());
    assert!(String::from_utf8_lossy(&output.stdout).contains("off"));
    assert!(
        s.run(&["config", "unset", "loop.mode", "--global"])
            .status
            .success()
    );
    assert!(
        !String::from_utf8_lossy(&s.run(&["config", "get", "loop.mode"]).stdout).contains("off")
    );
}

#[test]
fn config_show_explain_preset_diff_and_edit() {
    let s = Sandbox::new();
    assert!(s.run(&["config", "preset", "offline"]).status.success());
    let show = s.run(&["config", "show", "--effective"]);
    assert!(show.status.success());
    assert!(String::from_utf8_lossy(&show.stdout).contains("decide.enabled"));
    assert!(s.run(&["config", "explain", "loop.mode"]).status.success());
    assert!(s.run(&["config", "diff"]).status.success());
    assert!(s.run(&["config", "edit", "--global"]).status.success());
}

#[test]
fn config_repo_layer_untrusted_keys_ignored() {
    let s = Sandbox::new();
    std::fs::write(s.repo.join(".tokenimum.toml"), "[prune]\nmode = 'apply'\n").unwrap();
    let out = s.run(&["config", "show", "--effective"]);
    assert!(out.status.success());
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(combined.contains("ignored") && combined.contains("prune.mode"));
}
