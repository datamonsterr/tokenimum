//! End-to-end: the real `tkmd` binary driven by the real `tkm-hook` binary with Claude Code
//! fixture payloads (EPICS E4 DoD). Needs `tkm-hook` built next to `tkmd`
//! (`cargo nextest run --workspace` builds every binary).
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]

mod common;

use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::Duration;

use common::*;
use serde_json::Value;
use tkm_proto::Host;

fn tkmd_bin() -> PathBuf {
    PathBuf::from(env!("CARGO_BIN_EXE_tkmd"))
}

fn hook_bin() -> PathBuf {
    let sibling = tkmd_bin().with_file_name("tkm-hook");
    assert!(
        sibling.exists(),
        "tkm-hook binary not found at {}; build the whole workspace (cargo nextest run --workspace)",
        sibling.display()
    );
    sibling
}

struct Daemon {
    child: Child,
    env: Env,
}

impl Daemon {
    fn spawn(env: Env, extra: &[(&str, &str)]) -> Self {
        let mut command = Command::new(tkmd_bin());
        apply_env(&mut command, &env);
        for (key, value) in extra {
            command.env(key, value);
        }
        let child = command.stdin(Stdio::null()).spawn().unwrap();
        let this = Self { child, env };
        let socket = this.env.paths.socket.clone();
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        while !socket.exists() {
            assert!(
                std::time::Instant::now() < deadline,
                "tkmd socket never appeared"
            );
            std::thread::sleep(Duration::from_millis(20));
        }
        this
    }

    fn hook(&self, event: &str, fixture_name: &str) -> String {
        let payload = std::fs::read(
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../fixtures/claude/hooks")
                .join(format!("{fixture_name}.json")),
        )
        .unwrap();
        self.hook_raw(event, &payload)
    }

    fn hook_raw(&self, event: &str, payload: &[u8]) -> String {
        let mut command = Command::new(hook_bin());
        apply_env(&mut command, &self.env);
        let mut child = command
            .args(["--host", "claude", "--event", event])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(payload).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(output.status.success(), "hook must exit 0: {output:?}");
        String::from_utf8(output.stdout).unwrap()
    }

    /// SIGTERM for a graceful shutdown with a final flush.
    fn stop(mut self) -> Env {
        let status = Command::new("kill")
            .args(["-TERM", &self.child.id().to_string()])
            .status()
            .unwrap();
        assert!(status.success());
        let exit = self.child.wait().unwrap();
        assert!(exit.success(), "tkmd exits 0 on SIGTERM: {exit:?}");
        self.env
    }
}

fn apply_env(command: &mut Command, env: &Env) {
    let root = env.dir.path();
    command
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", root.join("home"))
        .env("XDG_DATA_HOME", root.join("data"))
        .env("XDG_CONFIG_HOME", root.join("config"))
        .env("XDG_RUNTIME_DIR", root.join("run"))
        .env("TKM_DECIDE_ENABLED", "false");
}

#[test]
fn claude_fixture_session_end_to_end() {
    let env = Env::new();
    seed_fact(&env, "example.com");
    let daemon = Daemon::spawn(env, &[]);

    let block =
        serde_json::to_string(&format!("# Context\n{}\n", fact_line("example.com"))).unwrap();
    let injected = format!(
        "{{\"hookSpecificOutput\":{{\"hookEventName\":\"UserPromptSubmit\",\"additionalContext\":{block}}}}}"
    );

    // First pass of the session.
    assert_eq!(daemon.hook("session_start", "SessionStart"), "");
    assert_eq!(daemon.hook("prompt", "UserPromptSubmit"), injected);
    assert_eq!(daemon.hook("tool_done", "PostToolUse-bash"), "");
    assert_eq!(daemon.hook("tool_done", "PostToolUseFailure-bash"), "");
    assert_eq!(daemon.hook("turn_end", "Stop"), "");
    assert_eq!(daemon.hook("session_end", "SessionEnd"), "");
    // Resume of the same session: second round of fixtures.
    assert_eq!(daemon.hook("session_start", "SessionStart-2"), "");
    assert_eq!(daemon.hook("prompt", "UserPromptSubmit-2"), injected);
    assert_eq!(daemon.hook("tool_done", "PostToolUse-write"), "");
    assert_eq!(daemon.hook("tool_done", "PostToolUse-edit"), "");
    assert_eq!(daemon.hook("tool_done", "PostToolUse-read"), "");
    assert_eq!(daemon.hook("turn_end", "Stop-2"), "");
    assert_eq!(daemon.hook("session_end", "SessionEnd-2"), "");

    let env = daemon.stop();
    let db = tkm_store::open_read_only(&env.paths.db).unwrap();
    let count = |sql: &str| -> i64 { db.query_row(sql, [], |r| r.get(0)).unwrap() };
    assert_eq!(count("SELECT COUNT(*) FROM sessions"), 1);
    let (host, session, ended): (String, String, Option<i64>) = db
        .query_row("SELECT host, session_id, ended_at FROM sessions", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?))
        })
        .unwrap();
    assert_eq!(host, "claude");
    assert_eq!(session, "e928a2cf-e31b-4154-805b-919991d042ea");
    assert!(ended.is_some());
    let mut turns = db
        .prepare("SELECT calls FROM turns ORDER BY turn_idx")
        .unwrap();
    let calls: Vec<i64> = turns
        .query_map([], |r| r.get(0))
        .unwrap()
        .map(Result::unwrap)
        .collect();
    assert_eq!(calls, vec![2, 3]);
    assert!(count("SELECT COUNT(*) FROM repos WHERE repo_key = 'dir:~/dev/example'") == 1);
    assert_eq!(
        count("SELECT COUNT(*) FROM decisions"),
        0,
        "decide.enabled=false: no decisions"
    );
}

#[test]
fn stuck_loop_records_shadow_decisions_through_the_real_hook() {
    let env = Env::new();
    env.seed_envelope(FIXTURE_REPO, Host::Claude, &[5; 5]);

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .unwrap();
    let (url, hits) = runtime.block_on(fake_openrouter(200, decision_body(), Duration::ZERO));
    let daemon = Daemon::spawn(
        env,
        &[
            ("TKM_DECIDE_ENABLED", "true"),
            ("OPENROUTER_API_KEY", "sk-or-test"),
            ("TKM_DECIDE_ENDPOINT", &url),
        ],
    );
    assert_eq!(daemon.hook("session_start", "SessionStart"), "");
    daemon.hook("prompt", "UserPromptSubmit");
    let failing: Value = fixture("PostToolUseFailure-bash");
    let payload = serde_json::to_vec(&failing).unwrap();
    for _ in 0..20 {
        assert_eq!(
            daemon.hook_raw("tool_done", &payload),
            "",
            "shadow mode injects nothing"
        );
    }
    // Wait for the decision, then stop (final flush).
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    while hits.load(std::sync::atomic::Ordering::SeqCst) == 0 {
        assert!(
            std::time::Instant::now() < deadline,
            "fake OpenRouter never called; errors.log: {:?}",
            std::fs::read_to_string(&daemon.env.paths.errors_log)
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    std::thread::sleep(Duration::from_millis(500));
    daemon.hook("turn_end", "Stop");
    let env = daemon.stop();
    let db = tkm_store::open_read_only(&env.paths.db).unwrap();
    let shadow: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM decisions WHERE shadow = 1 AND purpose = 'loop_guard'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(shadow >= 1);
    let events: i64 = db
        .query_row(
            "SELECT COUNT(*) FROM events WHERE kind LIKE 'loop.%'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert!(events >= 1);
}

#[test]
fn second_daemon_exits_zero_and_leaves_the_first_serving() {
    let env = Env::new();
    let first = Daemon::spawn(env, &[]);
    let mut command = Command::new(tkmd_bin());
    apply_env(&mut command, &first.env);
    let status = command.stdin(Stdio::null()).status().unwrap();
    assert!(status.success(), "second instance exits 0");
    assert_eq!(first.hook("session_start", "SessionStart"), "");
    first.stop();
}
