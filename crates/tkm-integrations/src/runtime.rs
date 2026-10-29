//! Switchable integration adapters.
use crate::{
    EscalationEvent, Escalator, SkillCandidate, SkillSource,
    detect::{AdapterHealth, AdapterState, AdapterStatus, Clock, Mode, Probe},
    otherskill::OtherskillSource,
    superorchestrator::{RunDetector, SuperorchestratorEscalator},
};
use std::{
    collections::BTreeMap,
    ffi::OsString,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tkm_core::model::RepoKey;
use tkm_proto::Host;

/// Detection, health, and adapters shared by the daemon.
pub struct Integrations {
    path: OsString,
    home: PathBuf,
    env: BTreeMap<String, String>,
    modes: (Mode, Mode),
    health: (Arc<AdapterHealth>, Arc<AdapterHealth>),
    statuses: Mutex<Vec<AdapterStatus>>,
    runs: RunDetector,
    skills: OtherskillSource,
    escalator: SuperorchestratorEscalator,
}
impl Integrations {
    /// Creates disabled adapters until the first detection pass.
    #[must_use]
    pub fn new(
        super_mode: Mode,
        skills_mode: Mode,
        path: OsString,
        home: PathBuf,
        env: BTreeMap<String, String>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        let super_health = Arc::new(AdapterHealth::new(clock.clone()));
        let skills_health = Arc::new(AdapterHealth::new(clock.clone()));
        Self {
            runs: RunDetector::new(clock),
            skills: OtherskillSource::new(path.clone(), skills_health.clone()),
            escalator: SuperorchestratorEscalator::new(
                path.clone(),
                home.clone(),
                super_health.clone(),
            ),
            path,
            home,
            env,
            modes: (super_mode, skills_mode),
            health: (super_health, skills_health),
            statuses: Mutex::new(Vec::new()),
        }
    }
    /// Runs both probes and refreshes the run cache when enabled.
    pub fn detect_now(&self, cwd: &Path) {
        let probe = Probe::new(
            self.path.clone(),
            self.home.clone(),
            cwd.to_path_buf(),
            self.env.clone(),
        );
        let statuses = vec![
            probe.detect("superorchestrator", self.modes.0),
            probe.detect("otherskill", self.modes.1),
        ];
        if let Ok(mut current) = self.statuses.lock() {
            *current = statuses;
        }
        if self.superorchestrator_enabled() {
            self.refresh_runs();
        }
    }
    /// Runs a detection pass when ten minutes have elapsed.
    pub fn redetect_if_due(&self, cwd: &Path) -> bool {
        if self.health.0.should_redetect() {
            self.detect_now(cwd);
            true
        } else {
            false
        }
    }
    /// Returns the current detection and health states.
    #[must_use]
    pub fn status(&self) -> Vec<AdapterStatus> {
        self.statuses.lock().map_or_else(
            |_| Vec::new(),
            |statuses| {
                statuses
                    .iter()
                    .cloned()
                    .enumerate()
                    .map(|(i, s)| {
                        if i == 0 {
                            self.health.0.status(s)
                        } else {
                            self.health.1.status(s)
                        }
                    })
                    .collect()
            },
        )
    }
    /// Whether the orchestrator adapter is available.
    #[must_use]
    pub fn superorchestrator_enabled(&self) -> bool {
        self.status()
            .first()
            .is_some_and(|s| matches!(s.state, AdapterState::Enabled))
    }
    /// Whether the skill adapter is available.
    #[must_use]
    pub fn otherskill_enabled(&self) -> bool {
        self.status()
            .get(1)
            .is_some_and(|s| matches!(s.state, AdapterState::Enabled))
    }
    /// Finds a delegated run without subprocesses or filesystem access.
    #[must_use]
    pub fn delegated_run(&self, cwd: &Path, env: &BTreeMap<String, String>) -> Option<String> {
        if !self.superorchestrator_enabled() {
            return None;
        }
        RunDetector::from_env(env)
            .or_else(|| self.runs.match_cwd(cwd))
            .map(|r| r.run_id)
    }
    /// Refreshes the cached run list, subject to its rate limit.
    pub fn refresh_runs(&self) {
        if self.superorchestrator_enabled() && self.runs.refresh_runs(&self.path).is_err() {
            self.health
                .0
                .report_failure("superorchestrator run list failed");
        }
    }
}
impl SkillSource for Integrations {
    fn candidates(&self, repo: &RepoKey, host: Host, prompt: &str) -> Vec<SkillCandidate> {
        if self.otherskill_enabled() {
            self.skills.candidates(repo, host, prompt)
        } else {
            Vec::new()
        }
    }
}
impl Escalator for Integrations {
    fn delegated_run(&self, cwd: &Path, env: &BTreeMap<String, String>) -> Option<String> {
        Self::delegated_run(self, cwd, env)
    }
    fn superorchestrator_enabled(&self) -> bool {
        Self::superorchestrator_enabled(self)
    }
    fn observe_session(
        &self,
        host: Host,
        session_id: &str,
        cwd: &Path,
        env: &BTreeMap<String, String>,
    ) {
        if self.superorchestrator_enabled() {
            let mut session_env = env.clone();
            if !session_env.contains_key("SUPERORCHESTRATOR_RUN_ID")
                && let Some(run) = self.runs.match_cwd(cwd)
            {
                session_env.insert("SUPERORCHESTRATOR_RUN_ID".into(), run.run_id);
            }
            self.escalator
                .observe_session(host, session_id, cwd, &session_env);
        }
    }
    fn run_id(&self, host: Host, session_id: &str) -> Option<String> {
        if self.superorchestrator_enabled() {
            self.escalator.run_id(host, session_id)
        } else {
            None
        }
    }
    fn handles(&self, host: Host, session_id: &str) -> bool {
        self.superorchestrator_enabled() && self.escalator.handles(host, session_id)
    }
    fn escalate(&self, event: &EscalationEvent) {
        if self.superorchestrator_enabled() {
            self.escalator.escalate(event);
        }
    }
}
