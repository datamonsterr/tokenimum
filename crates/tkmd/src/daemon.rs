//! The daemon core: router, hot path, session lifecycle, flusher and worker orchestration.
//!
//! Locks: `state` is taken before `outbox`; neither is held across an `.await` or any I/O.
//! `state` is only held for in-memory work, including the flusher's activity snapshot;
//! a handler never waits behind disk or network. See ADR 0034.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc as std_mpsc;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use tkm_config::paths::{ErrorLog, Paths};
use tkm_core::inject;
use tkm_core::loopguard::{EscalationMode, LoopAction, TurnGuard, stop_key};
use tkm_core::model::{HookEvent, RepoKey, StopReason, Timestamp, Turn};
use tkm_hosts::{AdapterError, HostAdapter};
use tkm_integrations::{Escalator, NoEscalator, NoSkills, SkillSource};
use tkm_proto::monitor::{MonitorMessage, RedirectNotice, SessionUpdate};
use tkm_proto::{
    EventKind, Host, MAX_REPLY_FRAME, PROTOCOL_VERSION, Reply, ReplyAction, Request, encode_frame,
};
use tkm_store::{Repo, SessionKey, StoreError};
use tokio::sync::{Notify, broadcast, mpsc};
use tokio::task::JoinHandle;

use crate::backing::{Backing, SqliteBacking};
use crate::clock::{Clock, SystemClock};
use crate::decider::{Decider, NullDecider, StoreCalibration, StoreLedger};
use crate::live::{EscalationJob, JudgeJob};
use crate::persist::{EventInsert, FlushBatch, HOOK_ACTIVITY, SessionUpsert, TurnUpsert};
use crate::registry::AdapterRegistry;
use crate::settings::{FileSettings, LoopMode, Settings, SettingsSource};
use crate::state::{MAX_TURN_CALLS, SessionState, StateMap, Tag, TurnState};

/// Failure of one handler; the router turns it into the no-op reply and one log line.
#[derive(Debug, thiserror::Error)]
pub enum HandlerError {
    /// Host adapter rejected the payload or could not read a log.
    #[error("adapter: {0}")]
    Adapter(#[from] AdapterError),
    /// Database failure.
    #[error("storage: {0}")]
    Store(#[from] StoreError),
    /// Anything else, with a short description.
    #[error("{0}")]
    Other(String),
}

pub(crate) type HResult<T> = Result<T, HandlerError>;

/// Side effects queued by handlers for the background worker (never run on the hot path).
pub(crate) enum Effect {
    Judge(Box<JudgeJob>),
    Escalate(EscalationJob),
    Resolve(SessionSeed, u64),
}

/// What is known about a session before repository resolution.
#[derive(Clone)]
pub(crate) struct SessionSeed {
    pub host: Host,
    pub session_id: String,
    pub cwd: PathBuf,
    pub agent_home: PathBuf,
    pub log_path: Option<PathBuf>,
    pub delegated: bool,
    pub env: std::collections::BTreeMap<String, String>,
}

/// Constructor arguments. [`DaemonOptions::new`] gives production defaults; tests replace the
/// pieces they instrument.
pub struct DaemonOptions {
    /// Resolved directories.
    pub paths: Paths,
    /// Database access.
    pub backing: Arc<dyn Backing>,
    /// Decision source.
    pub decider: Arc<dyn Decider>,
    /// Time source.
    pub clock: Arc<dyn Clock>,
    /// Settings source, consulted when a session starts.
    pub settings: Arc<dyn SettingsSource>,
    /// Host adapters.
    pub adapters: AdapterRegistry,
    /// otherskill candidates (epic E9).
    pub skills: Arc<dyn SkillSource>,
    /// superorchestrator escalation (epic E9).
    pub escalator: Arc<dyn Escalator>,
    /// Spend ledger shared with the decision client.
    pub ledger: Arc<StoreLedger>,
    /// Flusher period (SPEC: 5 s).
    pub flush_interval: Duration,
    /// Idle time after which a session is folded (SPEC: 30 min).
    pub idle_after: Duration,
    /// Wake-up period of the idle watcher (SPEC: 60 s).
    pub scan_interval: Duration,
    /// Idle time after which a live session of the same repository is folded at the next
    /// `session_start`.
    pub stale_after: Duration,
    /// Monitor broadcast buffer; a subscriber that falls this far behind is dropped.
    pub monitor_capacity: usize,
    /// Process environment captured at start (redaction and `GIT_CEILING_DIRECTORIES`).
    pub env: Vec<(String, String)>,
}

impl DaemonOptions {
    /// Production defaults over `paths`, except that decisions are off until a decider is set.
    pub fn new(paths: Paths) -> Self {
        Self {
            backing: Arc::new(SqliteBacking::new(paths.db.clone())),
            decider: Arc::new(NullDecider),
            clock: Arc::new(SystemClock),
            settings: Arc::new(FileSettings::new(paths.clone(), Vec::new())),
            adapters: AdapterRegistry::standard(),
            skills: Arc::new(NoSkills),
            escalator: Arc::new(NoEscalator),
            ledger: Arc::new(StoreLedger::default()),
            flush_interval: Duration::from_secs(5),
            idle_after: Duration::from_mins(30),
            scan_interval: Duration::from_secs(60),
            stale_after: Duration::from_mins(5),
            monitor_capacity: 256,
            env: Vec::new(),
            paths,
        }
    }
}

/// The daemon.
pub struct Daemon {
    pub(crate) paths: Paths,
    errors_tx: std_mpsc::SyncSender<ErrorMessage>,
    dropped_errors: AtomicUsize,
    pub(crate) adapters: AdapterRegistry,
    pub(crate) backing: Arc<dyn Backing>,
    pub(crate) decider: Arc<dyn Decider>,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) started_at_ms: i64,
    pub(crate) settings_source: Arc<dyn SettingsSource>,
    pub(crate) default_settings: Arc<Settings>,
    pub(crate) skills: Arc<dyn SkillSource>,
    pub(crate) escalator: Arc<dyn Escalator>,
    pub(crate) ledger: Arc<StoreLedger>,
    pub(crate) calibration: Arc<StoreCalibration>,
    pub(crate) state: Mutex<StateMap>,
    pub(crate) outbox: Mutex<FlushBatch>,
    pub(crate) monitor_tx: broadcast::Sender<MonitorMessage>,
    pub(crate) effects_tx: mpsc::Sender<Effect>,
    effects_rx: Mutex<Option<mpsc::Receiver<Effect>>>,
    pub(crate) flush_notify: Notify,
    pub(crate) flush_lock: tokio::sync::Mutex<()>,
    pub(crate) activity: Notify,
    pub(crate) inflight: AtomicUsize,
    pub(crate) folds: AtomicUsize,
    pub(crate) inject_cache: Mutex<inject::Cache>,
    pub(crate) repo_cache: Mutex<std::collections::HashMap<PathBuf, (RepoKey, String, PathBuf)>>,
    pub(crate) log_watch: Mutex<crate::fold::LogWatch>,
    pub(crate) flush_interval: Duration,
    pub(crate) idle_after: Duration,
    pub(crate) scan_interval: Duration,
    pub(crate) stale_after: Duration,
    pub(crate) env: Vec<(String, String)>,
    pub(crate) model_probe_revision: Mutex<Option<i64>>,
    pub(crate) maintenance_lock: tokio::sync::Mutex<()>,
    pub(crate) idle_events:
        Mutex<std::collections::HashMap<Host, std::collections::BTreeSet<String>>>,
    pub(crate) host_fallback: Mutex<std::collections::HashMap<Host, bool>>,
    tasks: Mutex<Vec<JoinHandle<()>>>,
}

enum ErrorMessage {
    Line(String),
    Flush(std_mpsc::SyncSender<()>),
}

pub(crate) fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

pub(crate) fn noop_reply(host: Host) -> Reply {
    Reply {
        version: PROTOCOL_VERSION,
        action: ReplyAction::Nothing,
        stdout: host.noop_reply().to_owned(),
    }
}

/// Replaces a reply that does not fit [`MAX_REPLY_FRAME`] with the host's no-op reply.
pub fn cap_reply(reply: Reply, host: Host) -> Reply {
    let text_len = match &reply.action {
        ReplyAction::Inject { text } | ReplyAction::Notice { text } => text.len(),
        ReplyAction::InjectAndNotice { inject, notice } => inject.len() + notice.len(),
        _ => 0,
    };
    // JSON escaping grows text at most 6x: small replies always fit.
    if reply.stdout.len() <= 4_096 && text_len <= 4_096 {
        return reply;
    }
    match encode_frame(&reply, MAX_REPLY_FRAME) {
        Ok(_) => reply,
        Err(_) => noop_reply(host),
    }
}

fn join_text(first: Option<String>, second: Option<String>) -> Option<String> {
    match (first, second) {
        (Some(a), Some(b)) => Some(format!("{a}\n{b}")),
        (a, b) => a.or(b),
    }
}

fn timestamp(ms: u64) -> Timestamp {
    Timestamp(i64::try_from(ms).unwrap_or(i64::MAX))
}

impl Daemon {
    /// Builds a daemon. Call [`Daemon::start_workers`] to run the flusher, the effect worker and
    /// the idle watcher.
    pub fn new(options: DaemonOptions) -> Arc<Self> {
        let (monitor_tx, _) = broadcast::channel(options.monitor_capacity.max(1));
        let (effects_tx, effects_rx) = mpsc::channel(1_024);
        let (errors_tx, errors_rx) = std_mpsc::sync_channel(1_024);
        let errors = ErrorLog::for_paths(&options.paths);
        std::thread::spawn(move || {
            while let Ok(message) = errors_rx.recv() {
                match message {
                    ErrorMessage::Line(line) => {
                        if let Err(error) = errors.append(&line) {
                            tracing::warn!("cannot write errors.log: {error}");
                        }
                    }
                    ErrorMessage::Flush(done) => {
                        let _ = done.send(());
                    }
                }
            }
        });
        let default_settings = options.settings.settings_for(None);
        Arc::new(Self {
            model_probe_revision: Mutex::new(None),
            maintenance_lock: tokio::sync::Mutex::new(()),
            idle_events: Mutex::new(std::collections::HashMap::new()),
            host_fallback: Mutex::new(std::collections::HashMap::new()),
            errors_tx,
            dropped_errors: AtomicUsize::new(0),
            paths: options.paths,
            adapters: options.adapters,
            backing: options.backing,
            decider: options.decider,
            started_at_ms: i64::try_from(options.clock.now_ms()).unwrap_or(i64::MAX),
            clock: options.clock,
            settings_source: options.settings,
            default_settings,
            skills: options.skills,
            escalator: options.escalator,
            ledger: options.ledger,
            calibration: Arc::new(StoreCalibration::default()),
            state: Mutex::new(StateMap::default()),
            outbox: Mutex::new(FlushBatch::default()),
            monitor_tx,
            effects_tx,
            effects_rx: Mutex::new(Some(effects_rx)),
            flush_notify: Notify::new(),
            flush_lock: tokio::sync::Mutex::new(()),
            activity: Notify::new(),
            inflight: AtomicUsize::new(0),
            folds: AtomicUsize::new(0),
            inject_cache: Mutex::new(inject::Cache::default()),
            repo_cache: Mutex::new(std::collections::HashMap::new()),
            log_watch: Mutex::new(crate::fold::LogWatch::default()),
            flush_interval: options.flush_interval,
            idle_after: options.idle_after,
            scan_interval: options.scan_interval,
            stale_after: options.stale_after,
            env: options.env,
            tasks: Mutex::new(Vec::new()),
        })
    }

    /// Resolved directories.
    pub fn paths(&self) -> &Paths {
        &self.paths
    }

    /// Host adapters, for registering test or future adapters.
    pub fn adapters(&self) -> &AdapterRegistry {
        &self.adapters
    }

    /// Calibration thresholds per model version.
    pub fn calibration(&self) -> &Arc<StoreCalibration> {
        &self.calibration
    }

    /// Decision spend ledger.
    pub fn ledger(&self) -> &Arc<StoreLedger> {
        &self.ledger
    }

    /// Subscribes to the monitor push stream.
    pub fn subscribe(&self) -> broadcast::Receiver<MonitorMessage> {
        self.monitor_tx.subscribe()
    }

    /// Number of monitor subscribers.
    pub fn subscriber_count(&self) -> usize {
        self.monitor_tx.receiver_count()
    }

    pub(crate) fn publish(&self, message: impl FnOnce() -> MonitorMessage) {
        if self.monitor_tx.receiver_count() > 0 {
            let _ = self.monitor_tx.send(message());
        }
    }

    /// Queues one line for the error-log writer without waiting for disk I/O.
    pub fn log_error(&self, line: &str) {
        let flat: String = line
            .chars()
            .map(|c| if c == '\n' { ' ' } else { c })
            .collect();
        if self.errors_tx.try_send(ErrorMessage::Line(flat)).is_err() {
            self.dropped_errors.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Number of error lines dropped because the writer was full or unavailable.
    pub fn dropped_error_lines(&self) -> usize {
        self.dropped_errors.load(Ordering::Relaxed)
    }

    /// Waits for the writer to process all previously queued error lines.
    pub async fn flush_error_log(&self) {
        let tx = self.errors_tx.clone();
        let _ = tokio::task::spawn_blocking(move || {
            let (done, received) = std_mpsc::sync_channel(0);
            if tx.send(ErrorMessage::Flush(done)).is_ok() {
                let _ = received.recv();
            }
        })
        .await;
    }

    /// Spawns the flusher, the effect worker and the idle watcher on the current runtime.
    pub fn start_workers(self: &Arc<Self>) {
        let Some(rx) = lock(&self.effects_rx).take() else {
            return;
        };
        let mut tasks = lock(&self.tasks);
        tasks.push(tokio::spawn(self.clone().effects_loop(rx)));
        tasks.push(tokio::spawn(self.clone().flush_loop()));
        tasks.push(tokio::spawn(self.clone().idle_loop()));
        tasks.push(crate::fold::spawn_log_watcher(self.clone()));
        tasks.push(tokio::spawn(self.clone().maintenance_loop()));
    }

    /// Refreshes integration-dependent settings on all live sessions after detection.
    pub fn refresh_integration_settings(&self) {
        let enabled = self.escalator.superorchestrator_enabled();
        for session in lock(&self.state).sessions.values_mut() {
            if session.settings.prune.superorchestrator_adapter_enabled != enabled {
                Arc::make_mut(&mut session.settings)
                    .prune
                    .superorchestrator_adapter_enabled = enabled;
            }
        }
    }

    /// Final flush and worker shutdown.
    pub async fn shutdown(self: &Arc<Self>) {
        self.settle().await;
        self.flush_now().await;
        let ledger = self.ledger.clone();
        let _ = tokio::task::spawn_blocking(move || ledger.flush()).await;
        for task in lock(&self.tasks).drain(..) {
            task.abort();
        }
        self.flush_error_log().await;
    }

    /// Waits until no decision job or fold is running (bounded at 10 s). For tests and shutdown.
    pub async fn settle(&self) {
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        while (self.inflight.load(Ordering::SeqCst) > 0 || self.folds.load(Ordering::SeqCst) > 0)
            && tokio::time::Instant::now() < deadline
        {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    // ---------------------------------------------------------------- router

    /// Handles one request. Never fails: every error becomes the host's no-op reply plus one
    /// `errors.log` line.
    pub async fn handle(self: &Arc<Self>, request: Request) -> Reply {
        let host = request.host;
        if is_cursor_duplicate(&request) {
            return noop_reply(host);
        }
        if request.version != PROTOCOL_VERSION {
            self.log_error(&format!(
                "{} {}: unsupported protocol version {}",
                host.as_str(),
                request.event.as_str(),
                request.version
            ));
            return noop_reply(host);
        }
        let reply = match self.dispatch(&request).await {
            Ok(reply) => reply,
            Err(error) => {
                self.log_error(&format!(
                    "{} {}: {error}",
                    host.as_str(),
                    request.event.as_str()
                ));
                noop_reply(host)
            }
        };
        cap_reply(reply, host)
    }

    async fn dispatch(self: &Arc<Self>, request: &Request) -> HResult<Reply> {
        let adapter = self.adapters.get(request.host);
        let Some(event) = adapter.map_event(request)? else {
            return Ok(noop_reply(request.host));
        };
        if event.kind == EventKind::ToolDone {
            if !self.host_allows_live(event.host, &event.session_id) {
                return Ok(noop_reply(request.host));
            }
            return self.tool_done(request, adapter.as_ref(), event);
        }
        let tag = (event.host, event.session_id.clone());
        if event.kind == EventKind::SessionStart {
            lock(&self.state).disabled_sessions.remove(&tag);
        } else if lock(&self.state).disabled_sessions.contains(&tag) {
            return Ok(noop_reply(request.host));
        }
        let kind = event.kind;
        let recorded = self.observe_hook(&tag, kind, request.host_event.as_deref());
        if matches!(kind, EventKind::TurnEnd | EventKind::SessionEnd)
            && self.event_uses_idle(request.host, request.host_event.as_deref(), kind)
        {
            return Ok(noop_reply(request.host));
        }
        let result = match event.kind {
            EventKind::ToolBefore => Ok(self.deliver_pending(request, adapter.as_ref(), &event)),
            EventKind::SessionStart => self.session_start(request, adapter.as_ref(), event).await,
            EventKind::Prompt => self.prompt(request, adapter.as_ref(), event).await,
            EventKind::TurnEnd => Ok(self.turn_end(request, &event)),
            EventKind::SessionEnd => self.session_end(request, event).await,
            _ => Ok(noop_reply(request.host)),
        };
        if !recorded {
            self.observe_hook(&tag, kind, request.host_event.as_deref());
        }
        result
    }

    fn host_allows_live(&self, host: Host, session_id: &str) -> bool {
        let state = lock(&self.state);
        let tag = (host, session_id.to_owned());
        if state.disabled_sessions.contains(&tag) {
            return false;
        }
        let settings = state
            .sessions
            .get(&tag)
            .map_or(self.default_settings.as_ref(), |session| {
                session.settings.as_ref()
            });
        settings.host(host).enabled
            && settings.effective_host_level(host) != crate::settings::HostLevel::FoldOnly
    }

    /// Synchronous `tool_done` entry point: the hot path. Mutates memory and pushes to queues
    /// only; no network, no disk, no `.await`.
    pub fn handle_tool_done(&self, request: &Request) -> Reply {
        let host = request.host;
        if is_cursor_duplicate(request) {
            return noop_reply(host);
        }
        let adapter = self.adapters.get(host);
        let result = match adapter.map_event(request) {
            Ok(Some(event)) if event.kind == EventKind::ToolDone => {
                if !self.host_allows_live(event.host, &event.session_id) {
                    return noop_reply(host);
                }
                self.tool_done(request, adapter.as_ref(), event)
            }
            Ok(_) => Ok(noop_reply(host)),
            Err(error) => Err(error.into()),
        };
        match result {
            Ok(reply) => cap_reply(reply, host),
            Err(error) => {
                self.log_error(&format!("{} tool_done: {error}", host.as_str()));
                noop_reply(host)
            }
        }
    }

    /// Renders `action`; an inject the host cannot show comes back as `None` so the caller keeps
    /// the text pending.
    pub(crate) fn render(
        request: &Request,
        adapter: &dyn HostAdapter,
        action: ReplyAction,
    ) -> (Reply, bool) {
        let host = request.host;
        if matches!(action, ReplyAction::Nothing) {
            return (noop_reply(host), true);
        }
        let stdout = adapter.render_reply(request, &action);
        let delivered = stdout != host.noop_reply();
        let action = if delivered {
            action
        } else {
            ReplyAction::Nothing
        };
        (
            Reply {
                version: PROTOCOL_VERSION,
                action,
                stdout,
            },
            delivered,
        )
    }

    // ---------------------------------------------------------------- seeds

    fn agent_home(&self, host: Host, request: &Request) -> PathBuf {
        let var = |name: &str| {
            request.env.get(name).cloned().or_else(|| {
                self.env
                    .iter()
                    .rev()
                    .find(|(k, _)| k == name)
                    .map(|(_, v)| v.clone())
            })
        };
        let home = var("HOME").map(PathBuf::from).unwrap_or_default();
        match host {
            Host::Claude => {
                var("CLAUDE_CONFIG_DIR").map_or_else(|| home.join(".claude"), PathBuf::from)
            }
            Host::Codex => var("CODEX_HOME").map_or_else(|| home.join(".codex"), PathBuf::from),
            Host::Agy => home.join(".gemini"),
            Host::Cursor => home.join(".cursor"),
            Host::Kimi => var("KIMI_SHARE_DIR").map_or_else(|| home.join(".kimi"), PathBuf::from),
            Host::Deepseek => {
                var("CODEWHALE_HOME").map_or_else(|| home.join(".codewhale"), PathBuf::from)
            }
            Host::Opencode => home.join(".config/opencode"),
            Host::Pi => {
                var("PI_CODING_AGENT_DIR").map_or_else(|| home.join(".pi/agent"), PathBuf::from)
            }
            _ => home.join(".tokenimum-unknown"),
        }
    }

    fn expand_home(&self, path: &str) -> PathBuf {
        if let Some(rest) = path.strip_prefix("~/")
            && let Some((_, home)) = self.env.iter().rev().find(|(k, _)| k == "HOME")
        {
            return Path::new(home).join(rest);
        }
        PathBuf::from(path)
    }

    pub(crate) fn seed(&self, request: &Request, event: &HookEvent) -> SessionSeed {
        let cwd = event
            .cwd
            .clone()
            .or_else(|| request.cwd.as_ref().map(PathBuf::from))
            .unwrap_or_default();
        let log_path = ["transcript_path", "rollout_path", "session_path"]
            .iter()
            .find_map(|field| {
                request
                    .payload
                    .get(field)
                    .and_then(serde_json::Value::as_str)
            })
            .map(|text| self.expand_home(text));
        SessionSeed {
            host: event.host,
            session_id: event.session_id.clone(),
            cwd: cwd.clone(),
            agent_home: self.agent_home(event.host, request),
            log_path,
            delegated: request.env.contains_key("TKM_DELEGATED")
                || self.escalator.delegated_run(&cwd, &request.env).is_some(),
            env: request.env.clone(),
        }
    }

    fn lazy_session(
        &self,
        seed: &SessionSeed,
        repo: Option<RepoKey>,
        now: u64,
        generation: u64,
    ) -> SessionState {
        self.escalator
            .observe_session(seed.host, &seed.session_id, &seed.cwd, &seed.env);
        SessionState {
            generation,
            key: SessionKey {
                host: seed.host,
                agent_home: seed.agent_home.to_string_lossy().into_owned(),
                session_id: seed.session_id.clone(),
            },
            host: seed.host,
            session_id: seed.session_id.clone(),
            repo: repo.unwrap_or_default(),
            repo_label: String::new(),
            root: None,
            cwd: seed.cwd.clone(),
            log_path: seed.log_path.clone(),
            agent_home: seed.agent_home.clone(),
            settings: self.default_settings.clone(),
            started: timestamp(now),
            last_activity_ms: now,
            next_turn_idx: 0,
            turn: None,
            pending_inject: None,
            injected: std::collections::BTreeSet::new(),
            delegated: seed.delegated,
            resolved: false,
            shadow: false,
            envelope: None,
            loop_state: "watching",
            loop_evidence: None,
            observed_hooks: std::collections::BTreeSet::new(),
        }
    }

    fn escalation_mode(&self, session: &SessionState) -> EscalationMode {
        if session.settings.superorchestrator_mode != tkm_integrations::detect::Mode::Off
            && self.escalator.handles(session.host, &session.session_id)
        {
            EscalationMode::DelegatedWithAdapter
        } else if session.delegated {
            EscalationMode::DelegatedNoAdapter
        } else {
            EscalationMode::Interactive {
                stop_key: stop_key(session.host).to_owned(),
            }
        }
    }

    pub(crate) fn begin_turn(&self, session: &mut SessionState, seq: u64, prompt: String) {
        session.loop_evidence = None;
        let guard = (!self.host_is_fold_only(session.host)
            && session.settings.loop_mode != LoopMode::Off)
            .then(|| TurnGuard::new(self.escalation_mode(session)));
        session.turn = Some(TurnState {
            seq,
            idx: session.next_turn_idx,
            prompt,
            guard,
            calls: Vec::new(),
            total_calls: 0,
            redirects: 0,
            redirect_kind: None,
            judge_scores: None,
            escalated: false,
            judging: false,
            decision_evidence: Vec::new(),
            pending_redirect: None,
        });
        session.loop_state = if session.settings.loop_mode == LoopMode::Off {
            "off"
        } else if session.settings.loop_mode == LoopMode::Shadow {
            "shadow"
        } else {
            "watching"
        };
    }

    /// Records the end of `session.turn` (if any) in the outbox and the envelope.
    pub(crate) fn finish_turn(
        &self,
        guard: &mut StateMap,
        tag: &Tag,
        stopped_by: StopReason,
        outbox: &mut FlushBatch,
    ) {
        let StateMap {
            sessions,
            envelopes,
            ..
        } = guard;
        let Some(session) = sessions.get_mut(tag) else {
            return;
        };
        let Some(turn) = session.turn.take() else {
            return;
        };
        let escalated = turn.escalated;
        let backstop = turn
            .guard
            .as_ref()
            .is_some_and(|guard| guard.backstop_triggered);
        let model_turn = Turn {
            prompt: turn.prompt,
            calls: turn.calls,
            stopped_by: if escalated {
                StopReason::Escalated
            } else {
                stopped_by
            },
            ..Turn::default()
        };
        let mut good = false;
        if session.resolved {
            let entry = envelopes
                .entry((session.repo.clone(), session.host))
                .or_default();
            good = entry.record(&model_turn, escalated, &session.settings.loop_cfg);
            if good {
                session.envelope = entry.value;
                outbox.envelope.push_back(crate::persist::EnvelopeSample {
                    repo: session.repo.clone(),
                    host: session.host,
                    calls: turn.total_calls,
                });
            }
        }
        let good_outcome = tkm_core::loopguard::is_good_turn(&model_turn, escalated);
        for (decision_id, call_index) in &turn.decision_evidence {
            let label = if backstop || stopped_by == StopReason::Interrupted {
                Some("stuck")
            } else if good_outcome && turn.total_calls > *call_index {
                Some("progress")
            } else {
                None
            };
            if let Some(label) = label {
                outbox.outcomes.push_back(crate::persist::DecisionOutcome {
                    decision_id: decision_id.clone(),
                    label,
                });
            }
        }
        outbox.push_turn(TurnUpsert {
            key: session.key.clone(),
            turn_idx: turn.idx,
            calls: i64::from(turn.total_calls),
            ended_well: good,
            escalated,
        });
        session.next_turn_idx = turn.idx + 1;
        session.loop_state = "watching";
    }

    // ------------------------------------------------------------- tool_done

    #[allow(clippy::unnecessary_wraps)]
    fn tool_done(
        &self,
        request: &Request,
        adapter: &dyn HostAdapter,
        mut event: HookEvent,
    ) -> HResult<Reply> {
        let host = request.host;
        let Some(call) = event.call.take() else {
            self.observe_hook(
                &(event.host, event.session_id),
                event.kind,
                request.host_event.as_deref(),
            );
            return Ok(noop_reply(host));
        };
        let now = self.clock.now_ms();
        let tag: Tag = (event.host, event.session_id.clone());
        let mut inject_text: Option<String> = None;
        let mut redirect_text: Option<String>;
        let turn_seq_now;
        let mut notice_text: Option<String> = None;
        {
            let mut guard = lock(&self.state);
            let state = &mut *guard;
            if !state.sessions.contains_key(&tag) {
                let seed = self.seed(request, &event);
                state.session_generation = state.session_generation.wrapping_add(1);
                let generation = state.session_generation;
                let session = self.lazy_session(&seed, event.repo_key.clone(), now, generation);
                state.sessions.insert(tag.clone(), session);
                self.enqueue(Effect::Resolve(seed, generation));
                self.activity.notify_one();
            }
            let StateMap {
                sessions, turn_seq, ..
            } = state;
            let Some(session) = sessions.get_mut(&tag) else {
                return Ok(noop_reply(host));
            };
            session.last_activity_ms = now;
            self.record_hook_seen(
                session,
                event.kind,
                request.host_event.as_deref(),
                Some(now),
            );
            if self.host_is_fold_only(host)
                || self.event_uses_idle(host, request.host_event.as_deref(), event.kind)
            {
                return Ok(noop_reply(host));
            }
            if session.turn.is_none() {
                *turn_seq += 1;
                self.begin_turn(session, *turn_seq, String::new());
            }
            let settings = session.settings.clone();
            let envelope = session.envelope;
            // The turn is taken out while it is processed, so `session` stays freely borrowable.
            let Some(mut turn) = session.turn.take() else {
                return Ok(noop_reply(host));
            };
            redirect_text = turn.pending_redirect.take();
            turn.total_calls = turn.total_calls.saturating_add(1);
            let actions = match turn.guard.as_mut() {
                Some(turn_guard) => turn_guard.on_tool_done(&call, &settings.loop_cfg, envelope),
                None => Vec::new(),
            };
            if turn.calls.len() < MAX_TURN_CALLS {
                turn.calls.push(call);
            }
            turn_seq_now = turn.seq;
            for action in actions {
                match action {
                    LoopAction::RequestJudge { request: judge } => {
                        let queued = if settings.decide.enabled {
                            let job = JudgeJob {
                                tag: tag.clone(),
                                repo: session.repo.clone(),
                                key: session.key.clone(),
                                seq: turn_seq_now,
                                turn_idx: turn.idx,
                                request: judge,
                                prompt: turn.prompt.clone(),
                                calls: turn.calls.clone(),
                                repo_root: session
                                    .root
                                    .clone()
                                    .unwrap_or_else(|| session.cwd.clone()),
                                settings: settings.clone(),
                            };
                            self.enqueue(Effect::Judge(Box::new(job)))
                        } else {
                            false
                        };
                        if let Some(turn_guard) = turn.guard.as_mut() {
                            if queued {
                                turn.judging = true;
                                session.loop_state = "judging";
                            } else {
                                turn_guard.on_judge_unavailable(judge);
                            }
                        }
                    }
                    LoopAction::Redirect {
                        redirect_kind,
                        text,
                    } => {
                        let shadow = settings.loop_mode == LoopMode::Shadow;
                        let kind = redirect_kind.as_str();
                        if shadow {
                            session.shadow = true;
                            self.record_loop_event(session, "loop.shadow", kind, &text, true);
                        } else {
                            turn.redirects += 1;
                            turn.redirect_kind = Some(kind.to_owned());
                            session.loop_state = "redirected";
                            self.record_loop_event(session, "loop.redirect", kind, &text, false);
                            redirect_text = join_text(redirect_text, Some(text));
                        }
                    }
                    LoopAction::Escalate { mode } => {
                        turn.escalated = true;
                        session.loop_state = "escalated";
                        let shadow = settings.loop_mode == LoopMode::Shadow;
                        let text = match &mode {
                            EscalationMode::Interactive { stop_key } => format!(
                                "tokenimum: this turn looks stuck again after a redirect. Press {stop_key} to stop it and give new direction."
                            ),
                            _ => "tokenimum: delegated run is stuck after a redirect.".to_owned(),
                        };
                        session.shadow |= shadow;
                        self.record_loop_event(
                            session,
                            "loop.escalation",
                            "escalation",
                            &text,
                            shadow,
                        );
                        if !shadow {
                            match mode {
                                EscalationMode::Interactive { .. } => notice_text = Some(text),
                                EscalationMode::DelegatedWithAdapter
                                | EscalationMode::DelegatedNoAdapter => {
                                    self.enqueue(Effect::Escalate(EscalationJob {
                                        host: session.host,
                                        session_id: session.session_id.clone(),
                                        repo: session.repo.clone(),
                                        turn_calls: turn.total_calls,
                                        envelope: session.envelope.map_or(0, |v| {
                                            format!("{v:.0}").parse().unwrap_or_default()
                                        }),
                                        redirect_given: turn.redirect_kind.clone(),
                                        scores: turn.judge_scores,
                                        prompt: turn.prompt.clone(),
                                        calls: turn.calls.clone(),
                                        repo_root: session
                                            .root
                                            .clone()
                                            .unwrap_or_else(|| session.cwd.clone()),
                                        decide_enabled: settings.decide.enabled,
                                        with_adapter: matches!(
                                            mode,
                                            EscalationMode::DelegatedWithAdapter
                                        ),
                                        delegated_run: self
                                            .escalator
                                            .delegated_run_for(session.host, &session.session_id),
                                        command: settings.escalate_command.clone(),
                                        secrets: request
                                            .env
                                            .iter()
                                            .filter(|(name, _)| {
                                                let name = name.to_ascii_uppercase();
                                                ["KEY", "TOKEN", "SECRET"]
                                                    .iter()
                                                    .any(|part| name.contains(part))
                                            })
                                            .map(|(name, value)| (name.clone(), value.clone()))
                                            .collect(),
                                    }));
                                }
                            }
                        }
                    }
                }
            }
            session.turn = Some(turn);
            inject_text = join_text(session.pending_inject.take(), inject_text);
            if self.monitor_tx.receiver_count() > 0 {
                let update = Self::session_update(session, "active");
                let _ = self.monitor_tx.send(MonitorMessage::Session(update));
            }
        }
        let combined = join_text(inject_text.clone(), redirect_text.clone());
        let action = match (combined, notice_text) {
            (Some(inject), Some(notice)) => ReplyAction::InjectAndNotice { inject, notice },
            (Some(text), None) => ReplyAction::Inject { text },
            (None, Some(text)) => ReplyAction::Notice { text },
            (None, None) => ReplyAction::Nothing,
        };
        let (reply, delivered) = Self::render(request, adapter, action);
        // The host cannot show it on this event: keep it for the next injectable one.
        if !delivered && let Some(session) = lock(&self.state).sessions.get_mut(&tag) {
            session.pending_inject = join_text(inject_text, session.pending_inject.take());
            if let Some(turn) = session.turn.as_mut()
                && turn.seq == turn_seq_now
            {
                turn.pending_redirect = join_text(redirect_text, turn.pending_redirect.take());
            }
        }
        Ok(reply)
    }

    /// Delivers a pending block on an event that can inject, without other processing.
    fn deliver_pending(
        &self,
        request: &Request,
        adapter: &dyn HostAdapter,
        event: &HookEvent,
    ) -> Reply {
        if self.host_is_fold_only(event.host) {
            return noop_reply(request.host);
        }
        let tag: Tag = (event.host, event.session_id.clone());
        let (pending, redirect, seq) = {
            let mut state = lock(&self.state);
            let Some(session) = state.sessions.get_mut(&tag) else {
                return noop_reply(request.host);
            };
            let seq = session.turn.as_ref().map(|turn| turn.seq);
            let redirect = session
                .turn
                .as_mut()
                .and_then(|turn| turn.pending_redirect.take());
            (session.pending_inject.take(), redirect, seq)
        };
        let Some(text) = join_text(pending.clone(), redirect.clone()) else {
            return noop_reply(request.host);
        };
        let (reply, delivered) =
            Self::render(request, adapter, ReplyAction::Inject { text: text.clone() });
        if delivered && request.host == Host::Cursor && event.kind == EventKind::SessionStart {
            if let Some(session) = lock(&self.state).sessions.get_mut(&tag) {
                session.pending_inject = join_text(Some(text), session.pending_inject.take());
            }
        } else if !delivered && let Some(session) = lock(&self.state).sessions.get_mut(&tag) {
            session.pending_inject = join_text(pending, session.pending_inject.take());
            if let Some(turn) = session.turn.as_mut()
                && Some(turn.seq) == seq
            {
                turn.pending_redirect = join_text(redirect, turn.pending_redirect.take());
            }
        }
        reply
    }

    /// Queues an effect; returns false (and drops it) when the queue is full or stopped.
    pub(crate) fn enqueue(&self, effect: Effect) -> bool {
        self.inflight.fetch_add(1, Ordering::SeqCst);
        if self.effects_tx.try_send(effect).is_ok() {
            true
        } else {
            self.inflight.fetch_sub(1, Ordering::SeqCst);
            false
        }
    }

    pub(crate) fn session_update(session: &SessionState, phase: &str) -> SessionUpdate {
        SessionUpdate {
            host: session.host,
            session_id: session.session_id.clone(),
            turn_id: session
                .turn
                .as_ref()
                .and_then(|turn| u64::try_from(turn.idx).ok()),
            repo: (!session.repo.0.is_empty()).then(|| session.repo.0.clone()),
            phase: phase.to_owned(),
            turn_calls: session.turn.as_ref().map_or(0, |turn| turn.total_calls),
            envelope: session.envelope,
            loop_state: session.loop_state.to_owned(),
        }
    }

    fn observe_hook(&self, tag: &Tag, kind: EventKind, native: Option<&str>) -> bool {
        let mut state = lock(&self.state);
        let Some(session) = state.sessions.get_mut(tag) else {
            return false;
        };
        self.record_hook_seen(session, kind, native, None);
        true
    }

    fn record_hook_seen(
        &self,
        session: &mut SessionState,
        kind: EventKind,
        native: Option<&str>,
        now: Option<u64>,
    ) {
        let name = native.unwrap_or(kind.as_str());
        // Repeated hooks borrow the name: no allocation, timestamp or outbox lock.
        if session.observed_hooks.contains(name) {
            return;
        }
        session.observed_hooks.insert(name.to_owned());
        lock(&self.outbox).events.push_back(EventInsert {
            key: session.key.clone(),
            kind: "hook.seen".into(),
            inputs: serde_json::json!({"event":kind.as_str(),"native":native}),
            template: "observation".into(),
            params: serde_json::json!({}),
            created_at: timestamp(now.unwrap_or_else(|| self.clock.now_ms())),
            evidence_ids: Vec::new(),
            decision_id: None,
        });
    }

    /// Queues a `loop.*` event row and publishes the redirect notice.
    pub(crate) fn record_loop_event(
        &self,
        session: &SessionState,
        kind: &str,
        detail: &str,
        text: &str,
        shadow: bool,
    ) {
        let template = if kind == "loop.escalation" {
            "escalation"
        } else {
            "redirect"
        };
        lock(&self.outbox).events.push_back(EventInsert {
            key: session.key.clone(),
            kind: kind.to_owned(),
            inputs: serde_json::json!({"redirect": detail, "shadow": shadow, "triggering_calls": session.turn.as_ref().map(|turn| turn.calls.iter().map(|call| call.tool.clone()).collect::<Vec<_>>()).unwrap_or_default()}),
            template: template.to_owned(),
            created_at: timestamp(self.clock.now_ms()),
            evidence_ids: Vec::new(),
            decision_id: session.loop_evidence.clone(),
            params: serde_json::json!({
                "event": kind,
                "detail": text,
                "why": "the loop guard saw repeated failures in this turn",
                "model_checks": "",
                "cost": "",
                "undo": "",
            }),
        });
        self.publish(|| {
            MonitorMessage::Redirect(RedirectNotice {
                host: session.host,
                session_id: session.session_id.clone(),
                triggering_calls: session
                    .turn
                    .as_ref()
                    .map(|turn| turn.calls.iter().map(|call| call.tool.clone()).collect())
                    .unwrap_or_default(),
                kind: if kind == "loop.escalation" {
                    "escalation"
                } else {
                    "redirect"
                }
                .to_owned(),
                text: text.to_owned(),
                shadow,
            })
        });
    }

    // ------------------------------------------------------- session lifecycle

    /// Resolves repository identity and settings, and loads the snapshot, for a session that is
    /// new or was created lazily by `tool_done`.
    pub(crate) async fn resolve_session(
        self: &Arc<Self>,
        seed: SessionSeed,
        generation: u64,
        want_stale: bool,
    ) {
        {
            let guard = lock(&self.state);
            if !guard
                .sessions
                .get(&(seed.host, seed.session_id.clone()))
                .is_some_and(|s| s.generation == generation && !s.resolved)
            {
                return;
            }
        }
        let me = self.clone();
        let blocking_seed = seed.clone();
        let resolved =
            tokio::task::spawn_blocking(move || me.resolve_blocking(&blocking_seed)).await;
        let resolved = match resolved {
            Ok(resolved) => resolved,
            Err(error) => {
                self.log_error(&format!("resolve session: {error}"));
                return;
            }
        };
        if let Some(error) = &resolved.error {
            self.log_error(&format!(
                "{} session {}: {error}",
                seed.host.as_str(),
                seed.session_id
            ));
        }
        for line in &resolved.settings.config_errors {
            self.log_error(&format!("config: {line}"));
        }
        let now = self.clock.now_ms();
        let tag: Tag = (seed.host, seed.session_id.clone());
        if !resolved.settings.host(seed.host).enabled
            || resolved.settings.effective_host_level(seed.host)
                == crate::settings::HostLevel::FoldOnly
        {
            let mut state = lock(&self.state);
            state.sessions.remove(&tag);
            state.disabled_sessions.insert(tag);
            return;
        }
        let mut stale_tags: Vec<Tag> = Vec::new();
        {
            let mut guard = lock(&self.state);
            let StateMap {
                sessions,
                envelopes,
                turn_bases,
                ..
            } = &mut *guard;
            let Some(session) = sessions
                .get_mut(&tag)
                .filter(|s| s.generation == generation && !s.resolved)
            else {
                return;
            };
            session.repo.clone_from(&resolved.repo);
            session.repo_label.clone_from(&resolved.label);
            session.root = Some(resolved.root.clone());
            let mut settings = (*resolved.settings).clone();
            settings.prune.superorchestrator_adapter_enabled =
                self.escalator.superorchestrator_enabled();
            if self.host_is_fold_only(seed.host) {
                settings.inject_enabled = false;
                settings.loop_mode = LoopMode::Off;
            }
            session.settings = Arc::new(settings);
            session.resolved = true;
            let remembered = turn_bases.get(&tag).copied().unwrap_or(0);
            session.next_turn_idx = session
                .next_turn_idx
                .max(resolved.turn_base)
                .max(remembered);
            if let Some(turn) = session.turn.as_mut()
                && turn.idx < session.next_turn_idx
                && turn.total_calls == 0
            {
                turn.idx = session.next_turn_idx;
            }
            let entry = envelopes
                .entry((resolved.repo.clone(), seed.host))
                .or_insert_with(|| {
                    crate::state::EnvelopeEntry::from_samples(
                        &resolved.envelope,
                        &resolved.settings.loop_cfg,
                    )
                });
            session.envelope = entry.value;
            let mut outbox = lock(&self.outbox);
            outbox.push_repo(Repo {
                repo_key: resolved.repo.clone(),
                display_name: resolved.label.clone(),
                first_seen: timestamp(now),
                last_seen: timestamp(now),
                delegated_runs: 0,
            });
            outbox.push_session(SessionUpsert {
                key: session.key.clone(),
                repo_key: session.repo.clone(),
                cwd: session.cwd.clone(),
                started: session.started,
                ended: None,
                fold_status: "open".into(),
                shadow: false,
                integration_level: if self.host_is_fold_only(seed.host) {
                    "fold-only"
                } else {
                    "hooks"
                }
                .into(),
            });
            drop(outbox);
            if want_stale {
                stale_tags = sessions
                    .iter()
                    .filter(|(other, s)| {
                        **other != tag
                            && s.repo == resolved.repo
                            && now.saturating_sub(s.last_activity_ms)
                                >= u64::try_from(self.stale_after.as_millis()).unwrap_or(u64::MAX)
                    })
                    .map(|(other, _)| other.clone())
                    .collect();
            }
        }
        self.calibration.refresh(&resolved.calibration);
        self.watch_log(&seed);
        for stale in stale_tags {
            self.fold_live_session(&stale, false);
        }
        if want_stale {
            self.fold_stored_sessions(&resolved, &seed, now);
        }
    }

    pub(crate) async fn ensure_session(self: &Arc<Self>, seed: SessionSeed, want_stale: bool) {
        let tag = (seed.host, seed.session_id.clone());
        let generation = {
            let mut state = lock(&self.state);
            if !state.sessions.contains_key(&tag) {
                state.session_generation = state.session_generation.wrapping_add(1);
                let session =
                    self.lazy_session(&seed, None, self.clock.now_ms(), state.session_generation);
                state.sessions.insert(tag.clone(), session);
            }
            state.sessions.get(&tag).map(|session| session.generation)
        };
        if let Some(generation) = generation {
            self.resolve_session(seed, generation, want_stale).await;
        }
    }

    pub(crate) fn refresh_session_credentials(&self, seed: &SessionSeed) {
        if seed.env.contains_key("SUPERORCHESTRATOR_RUN_ID")
            && lock(&self.state)
                .sessions
                .contains_key(&(seed.host, seed.session_id.clone()))
        {
            self.escalator
                .observe_session(seed.host, &seed.session_id, &seed.cwd, &seed.env);
        }
    }

    async fn session_start(
        self: &Arc<Self>,
        request: &Request,
        adapter: &dyn HostAdapter,
        event: HookEvent,
    ) -> HResult<Reply> {
        let seed = self.seed(request, &event);
        self.refresh_session_credentials(&seed);
        self.ensure_session(seed, true).await;
        let tag: Tag = (event.host, event.session_id.clone());
        {
            let mut guard = lock(&self.state);
            if let Some(session) = guard.sessions.get_mut(&tag) {
                session.last_activity_ms = self.clock.now_ms();
                if self.monitor_tx.receiver_count() > 0 {
                    let update = Self::session_update(session, "started");
                    let _ = self.monitor_tx.send(MonitorMessage::Session(update));
                }
            }
        }
        self.activity.notify_one();
        Ok(self.deliver_pending(request, adapter, &event))
    }

    async fn session_end(self: &Arc<Self>, request: &Request, event: HookEvent) -> HResult<Reply> {
        let seed = self.seed(request, &event);
        self.ensure_session(seed, false).await;
        let tag: Tag = (event.host, event.session_id.clone());
        self.fold_live_session(&tag, true);
        Ok(noop_reply(request.host))
    }

    fn turn_end(&self, request: &Request, event: &HookEvent) -> Reply {
        let tag: Tag = (event.host, event.session_id.clone());
        {
            let mut guard = lock(&self.state);
            let mut outbox = lock(&self.outbox);
            if let Some(session) = guard.sessions.get_mut(&tag) {
                session.last_activity_ms = self.clock.now_ms();
            }
            self.finish_turn(
                &mut guard,
                &tag,
                event.stopped_by.unwrap_or(StopReason::OwnStop),
                &mut outbox,
            );
            if self.monitor_tx.receiver_count() > 0
                && let Some(session) = guard.sessions.get(&tag)
            {
                let update = Self::session_update(session, "active");
                let _ = self.monitor_tx.send(MonitorMessage::Session(update));
            }
        }
        self.flush_notify.notify_one();
        noop_reply(request.host)
    }

    // ---------------------------------------------------------------- flusher

    /// Writes everything queued, on a blocking task. A failure keeps the unwritten rest queued
    /// and logs one line.
    pub async fn flush_now(self: &Arc<Self>) {
        let _serial = self.flush_lock.lock().await;
        let mut batch = std::mem::take(&mut *lock(&self.outbox));
        {
            // Reuse the timestamp already advanced by tool_done. Snapshot and allocation
            // belong to this periodic flush, leaving the repeated-hook path unchanged.
            let state = lock(&self.state);
            for session in state
                .sessions
                .values()
                .filter(|s| !s.observed_hooks.is_empty())
            {
                batch.events.push_back(EventInsert {
                    key: session.key.clone(),
                    kind: HOOK_ACTIVITY.into(),
                    inputs: serde_json::Value::Null,
                    template: String::new(),
                    params: serde_json::Value::Null,
                    created_at: timestamp(session.last_activity_ms),
                    evidence_ids: Vec::new(),
                    decision_id: None,
                });
            }
        }
        for record in self.calibration.take_pending() {
            batch.calibration.push_back(record);
        }
        if batch.is_empty() {
            return;
        }
        let me = self.clone();
        let joined = tokio::task::spawn_blocking(move || {
            let result = me
                .backing
                .open()
                .and_then(|store| crate::persist::apply(&store, &mut batch));
            (batch, result)
        })
        .await;
        match joined {
            Ok((rest, result)) => {
                if let Err(error) = result {
                    self.log_error(&format!("flush: {error}"));
                }
                let dropped = lock(&self.outbox).requeue(rest);
                if dropped > 0 {
                    self.log_error(&format!("flush queue overflow: dropped {dropped} rows"));
                }
            }
            Err(error) => self.log_error(&format!("flush task: {error}")),
        }
    }

    async fn flush_loop(self: Arc<Self>) {
        let mut ticker = tokio::time::interval(self.flush_interval);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            tokio::select! {
                _ = ticker.tick() => {}
                () = self.flush_notify.notified() => {}
            }
            self.flush_now().await;
        }
    }

    async fn effects_loop(self: Arc<Self>, mut rx: mpsc::Receiver<Effect>) {
        while let Some(effect) = rx.recv().await {
            let me = self.clone();
            tokio::spawn(async move {
                match effect {
                    Effect::Judge(job) => me.run_judge(*job).await,
                    Effect::Escalate(job) => me.run_escalation(job).await,
                    Effect::Resolve(seed, generation) => {
                        me.resolve_session(seed, generation, false).await;
                    }
                }
                me.inflight.fetch_sub(1, Ordering::SeqCst);
            });
        }
    }

    /// Spend ledger snapshot for monitor spend messages.
    pub(crate) fn spend_update(&self, settings: &Settings) -> tkm_proto::monitor::SpendUpdate {
        tkm_proto::monitor::SpendUpdate {
            today_usd: self.ledger.spent_today(self.clock.now_ms()),
            daily_cap_usd: settings.decide.daily_cap_usd,
            session_cap_usd: settings.decide.session_cap_usd,
        }
    }
}

fn is_cursor_duplicate(request: &Request) -> bool {
    request.host == Host::Claude && tkm_hosts::cursor::is_cursor_payload(&request.payload)
}
