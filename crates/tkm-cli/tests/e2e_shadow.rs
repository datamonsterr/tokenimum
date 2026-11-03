//! The installed Claude hooks reach the live shadow judge using only a fake HOME and local IPC.
#![forbid(unsafe_code)]
#![cfg(target_os = "linux")] // Match the init fixture's Linux XDG semantics.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]
#![allow(clippy::too_many_lines)] // Keep the installed session lifecycle in one test.

mod support_init;
mod support_shadow;

use std::fs;

use serde_json::Value;
use support_init::{snapshot, stdout};
use support_shadow::{EVENTS, LiveDaemon, SESSION, ShadowFixture, installed_hooks, payload};

#[test]
fn init_then_live_session_logs_shadow_decisions() {
    let fixture = ShadowFixture::new();
    let before = snapshot(&fixture.init.home);
    let hooks = fixture.install();
    let mut daemon = LiveDaemon::start(&fixture);

    daemon.event(&fixture, &hooks, "SessionStart");
    // Learn the ordinary five-sample envelope through the installed hooks, without DB seeding.
    for _ in 0..5 {
        daemon.event(&fixture, &hooks, "UserPromptSubmit");
        for _ in 0..5 {
            daemon.event(&fixture, &hooks, "PostToolUse");
        }
        daemon.event(&fixture, &hooks, "Stop");
    }
    daemon.wait_for_envelope(&fixture);
    daemon.event(&fixture, &hooks, "UserPromptSubmit");
    // Default gate opens at call 18. Both Claude tool events report the same failing action.
    for call in 0..18 {
        let native = if call % 2 == 0 {
            "PostToolUse"
        } else {
            "PostToolUseFailure"
        };
        let mut failure = payload(&fixture, native);
        failure["tool_input"]["command"] = "python -m pytest".into();
        if native == "PostToolUse" {
            failure["tool_response"] = serde_json::json!({
                "stdout": "", "stderr": "ValueError: boom", "exit_code": 1
            });
        } else {
            failure["error"] = "ValueError: boom".into();
        }
        daemon.hook(&fixture, &hooks[native], &failure);
    }
    // Observe persisted output before ending the turn, so a late verdict cannot be discarded.
    let evidence = daemon.wait_for_shadow(&fixture);
    assert_eq!(
        evidence.repo,
        format!(
            "dir:{}",
            fs::canonicalize(&fixture.init.home).unwrap().display()
        )
    );
    let request = daemon.backend_request();
    assert_eq!(request["model"], support_shadow::MODEL);
    for question in ["stuck", "progress", "redirect"] {
        assert!(request["questions"].get(question).is_some(), "{request}");
    }
    assert!(
        request["state"]
            .as_str()
            .unwrap()
            .contains("ValueError: boom")
    );

    // A completed shadow verdict must also stay silent on the next injectable event.
    daemon.event(&fixture, &hooks, "PostToolUse");
    daemon.event(&fixture, &hooks, "Stop");
    daemon.event(&fixture, &hooks, "SessionEnd");
    daemon.wait_for_end(&fixture);

    let status = fixture.init.run(&["status", "--json"]);
    let why = fixture.init.run(&["why", "last"]);
    // Check reversal even when an assertion about the captured reports would fail.
    daemon.shutdown();
    let uninstall = fixture.init.run(&["uninstall"]);
    assert!(uninstall.status.success(), "{uninstall:?}");
    assert_eq!(snapshot(&fixture.init.home), before);
    assert!(fixture.init.installs().is_empty());

    assert!(status.status.success(), "{status:?}");
    let report: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(report["daemon"]["state"], "running", "{report}");
    assert_eq!(report["preset"], "observe", "{report}");
    let repositories = report["repositories"].as_array().unwrap();
    assert_eq!(repositories.len(), 1, "{report}");
    let repository = &repositories[0];
    assert_eq!(repository["repo_key"], evidence.repo, "{report}");
    assert_eq!(repository["loop_guard"]["mode"], "shadow", "{report}");
    let envelopes = repository["loop_guard"]["envelopes"].as_array().unwrap();
    let envelope = envelopes
        .iter()
        .find(|envelope| envelope["host"] == "claude")
        .expect("status must report the learned Claude envelope");
    assert!(envelope["samples"].as_u64().unwrap() >= 5, "{report}");
    assert!(envelope["calls"].as_f64().unwrap() > 0.0, "{report}");
    let actions = repository["last_actions"].as_array().unwrap();
    let action = actions
        .iter()
        .find(|action| action["event_id"].as_i64() == Some(evidence.event_id))
        .expect("status must show the stored loop.shadow action");
    assert!(
        action["explanation"]
            .as_str()
            .unwrap()
            .starts_with("tokenimum redirected the agent:"),
        "{report}"
    );
    assert!(why.status.success(), "{why:?}");
    let explanation = stdout(&why);
    assert!(
        explanation.starts_with(&format!("Event {}, ", evidence.event_id)),
        "{explanation}"
    );
    for expected in [
        "Claude Code, repo dir:".into(),
        "What happened:".into(),
        "Why: the loop guard saw repeated failures in this turn".into(),
        format!("Model checks: {} (loop_guard)", support_shadow::MODEL),
        "Cost of those checks: $0.000010".into(),
    ] {
        assert!(explanation.contains(&expected), "{explanation}");
    }
    assert_eq!(evidence.session, SESSION);
}

#[test]
fn daemon_down_hooks_fail_open_after_init() {
    let fixture = ShadowFixture::new();
    let before = snapshot(&fixture.init.home);
    let hooks = fixture.install();
    fixture.install_hook_binary();
    assert!(!fixture.init.paths.socket.exists());

    for (native, _) in EVENTS {
        let lines_before = fixture.error_lines().len();
        let output = fixture.hook_process(&hooks[native], &payload(&fixture, native));
        assert_eq!(output.status.code(), Some(0), "{native}: {output:?}");
        assert_eq!(output.stdout, b"", "Claude no-op stdout: {native}");
        assert!(output.stderr.is_empty(), "{native}: {output:?}");
        let lines = fixture.error_lines();
        assert_eq!(lines.len(), lines_before + 1, "{native}: {lines:?}");
        assert!(
            lines
                .last()
                .unwrap()
                .contains("tkm-hook: daemon not running; starting tkmd.service through systemd")
        );
        assert!(!fixture.init.paths.socket.exists());
    }

    let settings = fs::read(fixture.init.home.join(".claude/settings.json")).unwrap();
    assert_eq!(installed_hooks(&settings).len(), EVENTS.len());
    let uninstall = fixture.init.run(&["uninstall"]);
    assert!(uninstall.status.success(), "{uninstall:?}");
    assert_eq!(snapshot(&fixture.init.home), before);
}
