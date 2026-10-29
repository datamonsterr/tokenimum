#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

#[test]
fn top_is_not_available() {
    assert_unavailable(&["top"], "top");
}

#[test]
fn dashboard_is_not_available() {
    assert_unavailable(&["dashboard"], "dashboard");
}

#[test]
fn skills_is_not_available_without_json() {
    assert_unavailable(&["skills"], "skills");
}

#[test]
fn skills_json_is_an_empty_array() {
    let output = common::tkm(&["skills", "--json"]);
    assert!(output.status.success(), "{output:?}");
    assert_eq!(output.stdout, b"[]\n");
    assert!(output.stderr.is_empty());
}

#[test]
fn other_commands_dispatch_to_stubs() {
    for (args, name) in [
        (vec!["init", "--yes", "--dry-run"], "init"),
        (vec!["uninstall", "--dry-run"], "uninstall"),
        (vec!["status", "--json"], "status"),
        (vec!["why", "last"], "why"),
        (vec!["facts"], "facts"),
        (vec!["facts", "forget", "f-1"], "facts"),
        (vec!["prune"], "prune"),
        (vec!["prune", "apply"], "prune"),
        (vec!["prune", "restore", "shell"], "prune"),
        (vec!["config", "show"], "config"),
        (vec!["models"], "models"),
        (vec!["models", "probe"], "models"),
        (vec!["calibrate"], "calibrate"),
        (vec!["replay", "fixture.json"], "replay"),
        (vec!["doctor", "--probe"], "doctor"),
        (vec!["export", "--show-digests"], "export"),
    ] {
        assert_unavailable(&args, name);
    }
}

#[test]
fn usage_error_exits_two() {
    let output = common::tkm(&["calibrate", "--purpose", "unknown"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(!output.stderr.is_empty());
}

fn assert_unavailable(args: &[&str], name: &str) {
    let output = common::tkm(args);
    assert_eq!(output.status.code(), Some(2), "{args:?}: {output:?}");
    assert!(output.stdout.is_empty(), "{args:?}: {output:?}");
    assert_eq!(
        String::from_utf8(output.stderr).unwrap(),
        format!("tkm {name}: not available in this build\n")
    );
}
