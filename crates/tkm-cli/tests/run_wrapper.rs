#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
#![cfg(unix)]

mod support_run;
use support_run::Harness;

#[test]
fn run_passes_args_and_env() {
    let harness = Harness::new();
    harness.script(
        "claude",
        "printf '%s\\n' \"$1\" \"$2\" \"$TKM_HOST\" \"$TKM_SESSION_ID\"",
    );
    let output = harness.run(&["run", "claude", "--model", "hello world"]);
    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8(output.stdout).unwrap();
    let lines: Vec<_> = stdout.lines().collect();
    assert_eq!(&lines[..3], &["--model", "hello world", "claude"]);
    assert!(!lines[3].is_empty());
    assert_eq!(lines.len(), 4);
}

#[test]
fn run_propagates_exit_code() {
    let harness = Harness::new();
    harness.script("claude", "exit 42");
    assert_eq!(harness.run(&["run", "claude"]).status.code(), Some(42));
}

#[test]
fn run_unknown_harness_exits_2() {
    let output = Harness::new().run(&["run", "unknown"]);
    assert_eq!(output.status.code(), Some(2));
    assert!(!output.stderr.is_empty());
}

#[test]
fn run_missing_binary_exits_127() {
    let output = Harness::new().run(&["run", "claude"]);
    assert_eq!(output.status.code(), Some(127));
    assert!(!output.stderr.is_empty());
}

#[test]
fn run_survives_sigint() {
    let harness = Harness::new();
    harness.script(
        "claude",
        "trap '' INT; kill -INT $PPID; /bin/sleep 0.2; exit 3",
    );
    let output = harness.run(&["run", "claude"]);
    assert_eq!(output.status.code(), Some(3), "{output:?}");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(!stderr.contains("panic"), "{stderr}");
    assert!(stderr.contains("daemon not running"), "{stderr}");
}

#[test]
fn run_daemon_down_fails_open() {
    let harness = Harness::new();
    harness.script("claude", "exit 0");
    let output = harness.run(&["run", "claude"]);
    assert!(output.status.success(), "{output:?}");
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert_eq!(stderr.lines().count(), 1, "{stderr}");
    assert!(stderr.contains("daemon not running"), "{stderr}");
}

#[test]
fn run_propagates_signal_exit_code() {
    let harness = Harness::new();
    harness.script("claude", "kill -TERM $$");
    assert_eq!(harness.run(&["run", "claude"]).status.code(), Some(143));
}

#[test]
fn run_accepts_harness_binary_aliases() {
    for (alias, canonical) in [
        ("codewhale", "deepseek"),
        ("agent", "cursor"),
        ("cursor-agent", "cursor"),
    ] {
        let harness = Harness::new();
        harness.script(alias, "printf '%s' \"$TKM_HOST\"");
        let output = harness.run(&["run", alias]);
        assert!(output.status.success(), "{alias}: {output:?}");
        assert_eq!(output.stdout, canonical.as_bytes());
    }
}
