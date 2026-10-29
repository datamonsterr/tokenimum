#![forbid(unsafe_code)]

use std::{
    collections::BTreeMap,
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};
use tkm_core::model::RepoKey;
use tkm_integrations::{
    EscalationEvent, Escalator, SkillSource,
    detect::{AdapterHealth, AdapterState, Clock, Mode, Probe},
    otherskill::{OtherskillSource, SkillEvidence, ranking_prior, reuse_key},
    superorchestrator::{
        InstallOutcome, RunDetector, Secret, SpendReporter, SpendSink, SuperorchestratorEscalator,
        account_homes, install_account_homes,
    },
};
use tkm_proto::Host;

#[derive(Default)]
struct FakeClock(AtomicU64);
impl Clock for FakeClock {
    fn now(&self) -> Duration {
        Duration::from_secs(self.0.load(Ordering::SeqCst))
    }
}
fn script(dir: &Path, name: &str, body: &str) {
    let path = dir.join(name);
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}
fn health() -> Arc<AdapterHealth> {
    Arc::new(AdapterHealth::new(Arc::new(FakeClock::default())))
}

#[test]
fn detection_modes_and_health_clock() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    fs::create_dir_all(home.join(".config/superorchestrator")).unwrap();
    let env = BTreeMap::new();
    let probe = Probe::new(
        dir.path().as_os_str().to_owned(),
        home,
        dir.path().to_path_buf(),
        env,
    );
    assert!(matches!(
        probe.detect("superorchestrator", Mode::Auto).state,
        AdapterState::Disabled { .. }
    ));
    script(dir.path(), "otherskill", "exit 0");
    assert_eq!(
        probe.detect("otherskill", Mode::Auto).reason.as_deref(),
        Some("config missing")
    );
    assert!(matches!(
        probe.detect("otherskill", Mode::On).state,
        AdapterState::Enabled
    ));
    script(dir.path(), "superorchestrator", "/bin/sleep 3");
    let start = Instant::now();
    assert!(
        probe
            .detect("superorchestrator", Mode::Auto)
            .reason
            .unwrap()
            .contains("timed out")
    );
    assert!(start.elapsed() < Duration::from_millis(1500));
    script(dir.path(), "superorchestrator", "exit 0");
    assert!(matches!(
        probe.detect("superorchestrator", Mode::Auto).state,
        AdapterState::Enabled
    ));
    assert!(matches!(
        probe.detect("superorchestrator", Mode::Off).state,
        AdapterState::Disabled { .. }
    ));
    let clock = Arc::new(FakeClock::default());
    let h = AdapterHealth::new(clock.clone());
    assert!(h.should_redetect());
    assert!(!h.should_redetect());
    clock.0.store(600, Ordering::SeqCst);
    assert!(h.should_redetect());
    h.report_failure("failed");
    assert!(h.in_backoff());
    clock.0.store(1500, Ordering::SeqCst);
    assert!(!h.in_backoff());
}

#[test]
fn otherskill_literal_prompt_dedup_and_backoff() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("args");
    script(
        dir.path(),
        "otherskill",
        &format!(
            "printf '%s\\n' \"$@\" > '{}'; printf '%s' '{{\"status\":\"ok\",\"selected\":[{{\"name\":\"a\",\"description\":\"first\"}},{{\"name\":\"a\",\"description\":\"second\"}}]}}'",
            log.display()
        ),
    );
    let h = health();
    let source = OtherskillSource::new(dir.path().as_os_str().to_owned(), h.clone());
    let prompt = "$(touch pwned); rm -rf / ; \"quote\" 'single' `bt` | & > <";
    let got = source.candidates(&RepoKey::default(), Host::Claude, prompt);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].text, "a: first");
    assert!(
        fs::read_to_string(log)
            .unwrap()
            .lines()
            .any(|s| s == prompt)
    );
    assert!(!dir.path().join("pwned").exists());
    assert!(
        source
            .candidates(&RepoKey::default(), Host::Cursor, prompt)
            .is_empty()
    );
    script(dir.path(), "otherskill", "printf invalid");
    assert!(
        source
            .candidates(&RepoKey::default(), Host::Claude, prompt)
            .is_empty()
    );
    assert!(h.in_backoff());
}

#[test]
fn otherskill_timeout_and_prior() {
    let dir = tempfile::tempdir().unwrap();
    script(dir.path(), "otherskill", "/bin/sleep 3");
    let h = health();
    let source = OtherskillSource::new(dir.path().as_os_str().to_owned(), h.clone());
    let start = Instant::now();
    assert!(
        source
            .candidates(&RepoKey::default(), Host::Codex, "p")
            .is_empty()
    );
    assert!(start.elapsed() < Duration::from_millis(1500));
    assert!(h.in_backoff());
    let marker = dir.path().join("spawned");
    script(
        dir.path(),
        "otherskill",
        &format!("printf spawned > '{}'", marker.display()),
    );
    assert!(
        source
            .candidates(&RepoKey::default(), Host::Codex, "p")
            .is_empty()
    );
    assert!(!marker.exists());
    let v = ranking_prior(&[
        SkillEvidence {
            name: "z".into(),
            loaded: 2,
            used: 1,
        },
        SkillEvidence {
            name: "a".into(),
            loaded: 1,
            used: 0,
        },
    ]);
    assert_eq!(v["skills"][0]["name"], "a");
    assert!(reuse_key(false, dir.path()).is_none());
}

#[test]
fn accounts_runs_and_spend() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("accounts")).unwrap();
    fs::write(
        dir.path().join("accounts/a.toml"),
        "name = 'a'\nharness = 'claude'\nhome = '/fake/a'\nenabled = true\n",
    )
    .unwrap();
    fs::write(
        dir.path().join("accounts/b.toml"),
        "name = 'b'\nharness = 'unknown'\nhome = '/fake/b'\nenabled = true\n",
    )
    .unwrap();
    let homes = account_homes(dir.path());
    assert_eq!(homes.len(), 2);
    let report = install_account_homes(&homes, |_, _| Ok::<_, ()>(InstallOutcome::AlreadyPresent));
    assert_eq!(report.already_present, 1);
    assert_eq!(report.skipped_unsupported, 1);
    let mut env = BTreeMap::new();
    env.insert("SUPERORCHESTRATOR_RUN_ID".into(), "id".into());
    env.insert("SUPERORCHESTRATOR_RUN_TOKEN".into(), "secret-value".into());
    let run = RunDetector::from_env(&env).unwrap();
    assert!(!format!("{run:?}").contains("secret-value"));
    assert_eq!(format!("{}", Secret::new("x".into())), "***");
    script(
        dir.path(),
        "superorchestrator",
        "printf '%s' '[{\"id\":\"one\",\"state\":\"running\",\"cwd\":\"/repo\",\"worktree\":\"/repo/nested\"},{\"id\":\"done\",\"state\":\"finished\",\"cwd\":\"/repo/nested/deep\"}]'",
    );
    let detector = RunDetector::new(Arc::new(FakeClock::default()));
    detector.refresh_runs(dir.path().as_os_str()).unwrap();
    assert_eq!(
        detector
            .match_cwd(Path::new("/repo/nested/deep"))
            .unwrap()
            .run_id,
        "one"
    );
    let file = dir.path().join("spend/report");
    let spend = SpendReporter::new(file.clone());
    spend.record("p", "m", 0.1, None);
    spend.record("p", "m", 0.2, Some("id"));
    let s = fs::read_to_string(file).unwrap();
    assert_eq!(s.lines().count(), 2);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(s.lines().next().unwrap()).unwrap()["account"],
        "tokenimum-decisions"
    );
}

#[test]
fn escalation_sends_mcp_and_times_out() {
    let dir = tempfile::tempdir().unwrap();
    let log = dir.path().join("log");
    script(
        dir.path(),
        "superorchestrator",
        &format!(
            "[ -n \"$SUPERORCHESTRATOR_RUN_TOKEN\" ] || exit 1; [ -z \"$UNRELATED_SECRET\" ] || exit 1; while IFS= read -r line; do printf '%s\\n' \"$line\" >> '{}'; case \"$line\" in *'\"id\":1'*) echo '{{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{{}}}}';; *'\"id\":2'*) echo '{{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{{}}}}';; esac; done",
            log.display()
        ),
    );
    let h = health();
    let e = SuperorchestratorEscalator::new(
        dir.path().as_os_str().to_owned(),
        dir.path().to_path_buf(),
        h.clone(),
    );
    let mut env = BTreeMap::new();
    env.insert("SUPERORCHESTRATOR_RUN_ID".into(), "id".into());
    env.insert("SUPERORCHESTRATOR_RUN_TOKEN".into(), "secret".into());
    env.insert("UNRELATED_SECRET".into(), "should-not-appear".into());
    e.observe_session(Host::Claude, "s", dir.path(), &env);
    assert!(e.handles(Host::Claude, "s"));
    e.escalate(&EscalationEvent {
        host: Host::Claude,
        session_id: "s".into(),
        ..Default::default()
    });
    let lines = fs::read_to_string(log).unwrap();
    assert!(lines.contains("notifications/initialized"));
    assert!(lines.contains("report_progress"));
    assert!(lines.contains("tokenimum.loop"));
    assert!(!lines.contains("secret"));
    script(dir.path(), "superorchestrator", "/bin/sleep 10");
    let start = Instant::now();
    e.escalate(&EscalationEvent {
        host: Host::Claude,
        session_id: "s".into(),
        ..Default::default()
    });
    assert!(start.elapsed() < Duration::from_secs(6));
    assert!(h.in_backoff());
}

#[test]
fn otherskill_uses_session_directory() {
    let dir = tempfile::tempdir().unwrap();
    let session = dir.path().join("session");
    fs::create_dir(&session).unwrap();
    let marker = dir.path().join("pwd");
    script(
        dir.path(),
        "otherskill",
        &format!(
            "pwd > '{}'; printf '%s' '{{\"status\":\"ok\",\"selected\":[]}}'",
            marker.display()
        ),
    );
    let source = OtherskillSource::new(dir.path().as_os_str().to_owned(), health());
    source.candidates_in(&RepoKey::default(), Host::Claude, "p", &session);
    assert_eq!(
        fs::read_to_string(marker).unwrap().trim(),
        session.to_str().unwrap()
    );
}

#[test]
fn otherskill_rejects_oversized_stdout() {
    let dir = tempfile::tempdir().unwrap();
    script(
        dir.path(),
        "otherskill",
        "/usr/bin/head -c 1100000 /dev/zero",
    );
    let source = OtherskillSource::new(dir.path().as_os_str().to_owned(), health());
    let start = Instant::now();
    assert!(
        source
            .candidates(&RepoKey::default(), Host::Claude, "p")
            .is_empty()
    );
    assert!(start.elapsed() < Duration::from_secs(2));
}

#[test]
fn otherskill_timeout_with_inherited_stdout_returns_promptly() {
    let dir = tempfile::tempdir().unwrap();
    script(dir.path(), "otherskill", "/bin/sleep 30 & wait");
    let source = OtherskillSource::new(dir.path().as_os_str().to_owned(), health());
    let start = Instant::now();
    assert!(
        source
            .candidates(&RepoKey::default(), Host::Claude, "p")
            .is_empty()
    );
    assert!(start.elapsed() < Duration::from_millis(1700));
}

#[test]
fn escalation_rejects_null_initialize_result() {
    let dir = tempfile::tempdir().unwrap();
    script(
        dir.path(),
        "superorchestrator",
        "read line; printf '%s\\n' '{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":null}'",
    );
    let h = health();
    let escalator = SuperorchestratorEscalator::new(
        dir.path().as_os_str().to_owned(),
        dir.path().to_path_buf(),
        h.clone(),
    );
    let env = BTreeMap::from([
        ("SUPERORCHESTRATOR_RUN_ID".into(), "run".into()),
        ("SUPERORCHESTRATOR_RUN_TOKEN".into(), "secret".into()),
    ]);
    escalator.observe_session(Host::Claude, "s", dir.path(), &env);
    escalator.escalate(&EscalationEvent {
        session_id: "s".into(),
        ..Default::default()
    });
    assert!(h.in_backoff());
}
