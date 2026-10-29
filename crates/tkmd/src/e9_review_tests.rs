#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]

use super::*;
use std::sync::Mutex;
use tkm_integrations::{Escalator, Suggest};

struct NoCall;
impl crate::decider::Decider for NoCall {
    fn decide(&self, _: DecideRequest) -> crate::decider::BoxFuture<'_, Outcome> {
        panic!("disabled decisions must not be called")
    }
}
#[derive(Default)]
struct Capture(Mutex<Option<EscalationEvent>>);
impl Escalator for Capture {
    fn handles(&self, _: Host, _: &str) -> bool {
        true
    }
    fn escalate(&self, event: &EscalationEvent) {
        *self.0.lock().unwrap() = Some(event.clone());
    }
}

#[tokio::test]
async fn disabled_session_decisions_skip_suggestion_request() {
    let dir = tempfile::tempdir().unwrap();
    let paths = tkm_config::paths::Paths::from_dirs(
        dir.path(),
        Some(&dir.path().join("data")),
        Some(&dir.path().join("config")),
        Some(&dir.path().join("run")),
    );
    let capture = Arc::new(Capture::default());
    let mut options = crate::daemon::DaemonOptions::new(paths);
    options.decider = Arc::new(NoCall);
    options.escalator = capture.clone();
    let daemon = Daemon::new(options);
    daemon
        .run_escalation(EscalationJob {
            host: Host::Claude,
            session_id: "s".into(),
            repo: RepoKey::default(),
            turn_calls: 1,
            envelope: 0,
            redirect_given: None,
            scores: None,
            prompt: String::new(),
            calls: Vec::new(),
            repo_root: dir.path().to_path_buf(),
            with_adapter: true,
            decide_enabled: false,
            delegated_run: None,
            command: None,
            secrets: BTreeMap::new(),
        })
        .await;
    let event = capture.0.lock().unwrap().clone().unwrap();
    assert_eq!(event.suggest, Suggest::Redirect);
    assert!(event.suggest_probs.is_empty());
}

fn fake_integrations(dir: &std::path::Path) -> Arc<tkm_integrations::runtime::Integrations> {
    use std::{fs, os::unix::fs::PermissionsExt};
    use tkm_integrations::detect::{Mode, SystemClock};
    let binary = dir.join("superorchestrator");
    let log = dir.join("mcp-env");
    fs::write(&binary, format!(r##"#!/bin/sh
if [ "$1" = --version ]; then exit 0; fi
if [ "$1" = runs ]; then printf '[]'; exit 0; fi
printf '%s' "$SUPERORCHESTRATOR_RUN_TOKEN" > '{}'
while IFS= read -r line; do
  case "$line" in
    *'"id":1'*) echo '{{"jsonrpc":"2.0","id":1,"result":{{"protocolVersion":"2025-06-18"}}}}';;
    *'"id":2'*) echo '{{"jsonrpc":"2.0","id":2,"result":{{"content":[{{"type":"text","text":"ok"}}]}}}}';;
  esac
done
"##, log.display())).unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
    let adapter = Arc::new(tkm_integrations::runtime::Integrations::new(
        Mode::On,
        Mode::Off,
        dir.as_os_str().to_owned(),
        dir.to_path_buf(),
        BTreeMap::new(),
        Arc::new(SystemClock),
    ));
    adapter.detect_now(dir);
    adapter
}

fn session_request(
    dir: &std::path::Path,
    event: tkm_proto::EventKind,
    token: &str,
) -> tkm_proto::Request {
    let (host_event, payload) = match event {
        tkm_proto::EventKind::ToolDone => (
            "PostToolUse",
            serde_json::json!({"session_id":"s", "hook_event_name":"PostToolUse", "cwd":dir, "tool_name":"Bash", "tool_input":{"command":"true"}, "tool_response":{"stdout":"ok"}, "tool_use_id":"call"}),
        ),
        _ => (
            "SessionStart",
            serde_json::json!({"session_id":"s", "hook_event_name":"SessionStart", "cwd":dir}),
        ),
    };
    tkm_proto::Request {
        version: tkm_proto::PROTOCOL_VERSION,
        host: Host::Claude,
        event,
        host_event: Some(host_event.into()),
        payload,
        env: BTreeMap::from([
            ("SUPERORCHESTRATOR_RUN_ID".into(), "run".into()),
            ("SUPERORCHESTRATOR_RUN_TOKEN".into(), token.into()),
        ]),
        cwd: None,
    }
}

fn escalation_job(dir: &std::path::Path, adapter: &dyn Escalator) -> EscalationJob {
    EscalationJob {
        host: Host::Claude,
        session_id: "s".into(),
        repo: RepoKey::default(),
        turn_calls: 1,
        envelope: 0,
        redirect_given: None,
        scores: None,
        prompt: String::new(),
        calls: Vec::new(),
        repo_root: dir.to_path_buf(),
        with_adapter: true,
        decide_enabled: false,
        delegated_run: adapter.delegated_run_for(Host::Claude, "s"),
        command: Some(format!(
            "printf fallback > '{}'",
            dir.join("fallback").display()
        )),
        secrets: BTreeMap::new(),
    }
}

fn integration_daemon(dir: &std::path::Path, adapter: Arc<dyn Escalator>) -> Arc<Daemon> {
    let paths = tkm_config::paths::Paths::from_dirs(
        dir,
        Some(&dir.join("data")),
        Some(&dir.join("config")),
        Some(&dir.join("run")),
    );
    let mut options = crate::daemon::DaemonOptions::new(paths);
    options.escalator = adapter;
    Daemon::new(options)
}

#[tokio::test]
async fn lazy_turn_uses_repository_off_at_escalation_dispatch() {
    use std::fs;
    use tkm_proto::EventKind;
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join(".git")).unwrap();
    fs::write(
        dir.path().join(".tokenimum.toml"),
        "[integrations]\nsuperorchestrator = 'off'\n",
    )
    .unwrap();
    let adapter = fake_integrations(dir.path());
    let daemon = integration_daemon(dir.path(), adapter.clone());
    daemon.handle_tool_done(&session_request(dir.path(), EventKind::ToolDone, "old"));
    let job = escalation_job(dir.path(), adapter.as_ref());
    assert!(job.delegated_run.is_some());
    daemon
        .handle(session_request(dir.path(), EventKind::SessionStart, "old"))
        .await;
    assert_eq!(
        lock(&daemon.state).sessions[&(Host::Claude, "s".into())]
            .settings
            .superorchestrator_mode,
        tkm_integrations::detect::Mode::Off
    );
    daemon.run_escalation(job).await;
    assert!(dir.path().join("fallback").exists());
    assert!(!dir.path().join("mcp-env").exists());
}

#[tokio::test]
async fn resumed_session_start_refreshes_mcp_token() {
    use tkm_proto::EventKind;
    let dir = tempfile::tempdir().unwrap();
    let adapter = fake_integrations(dir.path());
    let daemon = integration_daemon(dir.path(), adapter.clone());
    daemon
        .handle(session_request(dir.path(), EventKind::SessionStart, "old"))
        .await;
    daemon
        .handle(session_request(
            dir.path(),
            EventKind::SessionStart,
            "renewed",
        ))
        .await;
    daemon
        .run_escalation(escalation_job(dir.path(), adapter.as_ref()))
        .await;
    assert_eq!(
        std::fs::read_to_string(dir.path().join("mcp-env")).unwrap(),
        "renewed"
    );
}

#[tokio::test]
async fn pending_signal_tracks_queued_and_cleared_context() {
    let dir = tempfile::tempdir().unwrap();
    let paths = tkm_config::paths::Paths::from_dirs(dir.path(), None, None, None);
    let daemon = Daemon::new(crate::daemon::DaemonOptions::new(paths));
    let request = session_request(dir.path(), tkm_proto::EventKind::SessionStart, "unused");
    daemon.handle(request.clone()).await;
    let tag = (Host::Claude, "s".to_owned());
    {
        let mut state = crate::daemon::lock(&daemon.state);
        state.sessions.get_mut(&tag).unwrap().pending_inject = Some("queued".into());
    }
    assert_eq!(
        daemon
            .stamp_pending(&request, crate::daemon::noop_reply(Host::Claude))
            .pending,
        Some(true)
    );
    {
        let mut state = crate::daemon::lock(&daemon.state);
        state.sessions.get_mut(&tag).unwrap().pending_inject = None;
    }
    assert_eq!(
        daemon
            .stamp_pending(&request, crate::daemon::noop_reply(Host::Claude))
            .pending,
        Some(false)
    );
    let tool = session_request(dir.path(), tkm_proto::EventKind::ToolDone, "unused");
    daemon.handle_tool_done(&tool);
    {
        let mut state = crate::daemon::lock(&daemon.state);
        state
            .sessions
            .get_mut(&tag)
            .unwrap()
            .turn
            .as_mut()
            .unwrap()
            .pending_redirect = Some("steer".into());
    }
    assert_eq!(
        daemon
            .stamp_pending(&tool, crate::daemon::noop_reply(Host::Claude))
            .pending,
        Some(true)
    );
    {
        let mut state = crate::daemon::lock(&daemon.state);
        state
            .sessions
            .get_mut(&tag)
            .unwrap()
            .turn
            .as_mut()
            .unwrap()
            .pending_redirect = None;
    }
    assert_eq!(
        daemon
            .stamp_pending(&tool, crate::daemon::noop_reply(Host::Claude))
            .pending,
        Some(false)
    );
}
