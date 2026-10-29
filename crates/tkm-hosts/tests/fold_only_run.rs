#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Tests for `fold_only::run` (SPEC 6.12 wrapper library, ADR 0026).

use std::ffi::OsString;
use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use tkm_core::model::Timestamp;
use tkm_hosts::AdapterError;
use tkm_hosts::fold_only::run::{Clock, RunObserver, RunPlan, execute, plan};
use tkm_proto::Host;

struct Scratch(PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "tkm-fold-run-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Ticks 1000, 2000, ... so start and end differ and order is observable.
struct Ticking(std::cell::Cell<i64>);
impl Clock for Ticking {
    fn now(&self) -> Timestamp {
        self.0.set(self.0.get() + 1000);
        Timestamp(self.0.get())
    }
}

#[derive(Default)]
struct Recorder {
    calls: Vec<String>,
}
impl RunObserver for Recorder {
    fn started(&mut self, plan: &RunPlan, at: Timestamp) {
        self.calls
            .push(format!("started {} {}", plan.session_id, at.0));
    }
    fn ended(&mut self, plan: &RunPlan, at: Timestamp, exit_code: Option<i32>) {
        self.calls
            .push(format!("ended {} {} {exit_code:?}", plan.session_id, at.0));
    }
}

fn sh(host: Host, script: &str, args: &[&str]) -> RunPlan {
    let mut all = vec!["-c", script, "sh"];
    all.extend_from_slice(args);
    plan(host, "sh", all, "sess-1")
}

#[test]
fn plan_adds_session_and_host_env_and_keeps_args() {
    let p = plan(Host::Opencode, "opencode", ["run", "--flag"], "abc");
    assert_eq!(p.program, OsString::from("opencode"));
    assert_eq!(p.args, [OsString::from("run"), OsString::from("--flag")]);
    assert_eq!(
        p.env,
        [
            ("TKM_SESSION_ID".to_owned(), "abc".to_owned()),
            ("TKM_HOST".to_owned(), "opencode".to_owned())
        ]
    );
    assert_eq!(p.host, Host::Opencode);
    assert_eq!(p.session_id, "abc");
}

#[test]
fn plan_host_env_uses_wire_name_for_every_host() {
    for host in [
        Host::Claude,
        Host::Codex,
        Host::Agy,
        Host::Cursor,
        Host::Kimi,
        Host::Deepseek,
        Host::Pi,
    ] {
        let p = plan(host, "x", Vec::<String>::new(), "s");
        assert!(
            p.env
                .contains(&("TKM_HOST".to_owned(), host.as_str().to_owned()))
        );
    }
}

#[test]
fn execute_exposes_env_to_child_and_calls_observer_in_order() {
    let scratch = Scratch::new();
    let out = scratch.0.join("env.txt");
    let p = sh(
        Host::Pi,
        "printf '%s %s' \"$TKM_SESSION_ID\" \"$TKM_HOST\" > \"$1\"",
        &[out.to_str().unwrap()],
    );
    let mut recorder = Recorder::default();
    let outcome = execute(&p, &Ticking(0.into()), &mut recorder).unwrap();
    assert_eq!(fs::read_to_string(&out).unwrap(), "sess-1 pi");
    assert_eq!(outcome.exit_code, Some(0));
    assert_eq!(
        (outcome.started_at, outcome.ended_at),
        (Timestamp(1000), Timestamp(2000))
    );
    assert_eq!(
        recorder.calls,
        ["started sess-1 1000", "ended sess-1 2000 Some(0)"]
    );
}

#[test]
fn execute_propagates_nonzero_exit_code() {
    let mut recorder = Recorder::default();
    let outcome = execute(
        &sh(Host::Pi, "exit 7", &[]),
        &Ticking(0.into()),
        &mut recorder,
    )
    .unwrap();
    assert_eq!(outcome.exit_code, Some(7));
    assert_eq!(recorder.calls.last().unwrap(), "ended sess-1 2000 Some(7)");
}

#[test]
fn execute_reports_none_exit_code_when_killed_by_signal() {
    let mut recorder = Recorder::default();
    let outcome = execute(
        &sh(Host::Pi, "kill -9 $$", &[]),
        &Ticking(0.into()),
        &mut recorder,
    )
    .unwrap();
    assert_eq!(outcome.exit_code, None);
    assert_eq!(recorder.calls.last().unwrap(), "ended sess-1 2000 None");
}

#[test]
fn execute_missing_program_errors_without_ended() {
    let p = plan(
        Host::Pi,
        "tkm-definitely-missing-program",
        Vec::<String>::new(),
        "sess-1",
    );
    let mut recorder = Recorder::default();
    let err = execute(&p, &Ticking(0.into()), &mut recorder).unwrap_err();
    assert!(
        matches!(&err, AdapterError::Io(e) if e.kind() == std::io::ErrorKind::NotFound),
        "{err:?}"
    );
    assert!(recorder.calls.is_empty(), "{:?}", recorder.calls);
}

#[test]
fn execute_overrides_inherited_tkm_env() {
    let scratch = Scratch::new();
    let out = scratch.0.join("host.txt");
    let mut p = sh(
        Host::Codex,
        "printf '%s' \"$TKM_HOST\" > \"$1\"",
        &[out.to_str().unwrap()],
    );
    p.env.push(("TKM_EXTRA".to_owned(), "1".to_owned()));
    execute(&p, &Ticking(0.into()), &mut Recorder::default()).unwrap();
    assert_eq!(fs::read_to_string(&out).unwrap(), "codex");
}
