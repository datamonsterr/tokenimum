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
