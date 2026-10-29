//! Shared helpers for the daemon integration tests.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]

use std::collections::BTreeMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::{Value, json};
use tempfile::TempDir;
use tkm_config::paths::Paths;
use tkm_core::model::{RepoKey, Timestamp};
use tkm_decide::api::Answer;
use tkm_decide::client::{DecideRequest, Decision, Outcome};
use tkm_proto::{EventKind, Host, PROTOCOL_VERSION, Reply, Request};
use tkm_store::{Repo, Store, StoreError};
use tkmd::backing::{Backing, SqliteBacking};
use tkmd::clock::ManualClock;
use tkmd::daemon::{Daemon, DaemonOptions};
use tkmd::decider::{BoxFuture, Decider};
use tkmd::settings::{FixedSettings, Settings};

/// Repository key the Claude fixtures resolve to (their `cwd` is `~/dev/example`).
pub const FIXTURE_REPO: &str = "dir:~/dev/example";
/// Dated model version used by the scripted decider.
pub const MODEL: &str = "test/model-20260101";

pub struct Env {
    pub dir: TempDir,
    pub paths: Paths,
}

impl Env {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(&home).unwrap();
        let paths = Paths::from_dirs(
            &home,
            Some(&dir.path().join("data")),
            Some(&dir.path().join("config")),
            Some(&dir.path().join("run")),
        );
        Self { dir, paths }
    }

    pub fn store(&self) -> Store {
        Store::open(&self.paths.db).unwrap()
    }

    pub fn error_lines(&self) -> Vec<String> {
        std::fs::read_to_string(&self.paths.errors_log)
            .map(|text| text.lines().map(str::to_owned).collect())
            .unwrap_or_default()
    }

    /// Seeds the repository row and `samples` good-turn lengths so the guard has an envelope.
    pub fn seed_envelope(&self, repo: &str, host: Host, samples: &[u32]) {
        let store = self.store();
        seed_repo(&store, repo);
        for calls in samples {
            store
                .push_envelope_sample(&RepoKey(repo.into()), host, *calls)
                .unwrap();
        }
    }
}

pub fn seed_repo(store: &Store, repo: &str) {
    store
        .upsert_repo(&Repo {
            repo_key: RepoKey(repo.into()),
            display_name: "example".into(),
            first_seen: Timestamp(1),
            last_seen: Timestamp(1),
            delegated_runs: 0,
        })
        .unwrap();
}

pub fn fixture(name: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/claude/hooks")
        .join(format!("{name}.json"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

pub fn request(event: EventKind, payload: Value) -> Request {
    let host_event = payload
        .get("hook_event_name")
        .and_then(Value::as_str)
        .map(str::to_owned);
    Request {
        version: PROTOCOL_VERSION,
        host: Host::Claude,
        event,
        host_event,
        payload,
        env: BTreeMap::new(),
        cwd: None,
    }
}

pub fn session_start() -> Request {
    request(EventKind::SessionStart, fixture("SessionStart"))
}

pub fn prompt() -> Request {
    request(EventKind::Prompt, fixture("UserPromptSubmit"))
}

pub fn stop() -> Request {
    request(EventKind::TurnEnd, fixture("Stop"))
}

pub fn session_end() -> Request {
    request(EventKind::SessionEnd, fixture("SessionEnd"))
}

pub fn tool_done() -> Request {
    request(EventKind::ToolDone, fixture("PostToolUse-bash"))
}

/// A failing Bash call; every call carries the same error so the guard sees repeats.
pub fn failing_tool_done() -> Request {
    request(EventKind::ToolDone, fixture("PostToolUseFailure-bash"))
}

pub fn additional_context(reply: &Reply) -> Option<String> {
    let value: Value = serde_json::from_str(&reply.stdout).ok()?;
    value["hookSpecificOutput"]["additionalContext"]
        .as_str()
        .map(str::to_owned)
}

pub fn settings_fixed(settings: Settings) -> Arc<FixedSettings> {
    Arc::new(FixedSettings::new(settings))
}

/// Builds a daemon on a temp dir with a deterministic clock and the given decider.
pub fn daemon_with(env: &Env, settings: Settings, decider: Arc<dyn Decider>) -> Arc<Daemon> {
    let mut options = DaemonOptions::new(env.paths.clone());
    options.decider = decider;
    options.settings = settings_fixed(settings);
    options.clock = Arc::new(ManualClock::new(1_800_000_000_000));
    Daemon::new(options)
}

/// Decider that returns a fixed outcome and counts calls. An optional gate holds each call.
pub struct ScriptedDecider {
    pub outcome: Mutex<Outcome>,
    pub calls: AtomicUsize,
    pub gate: Option<Arc<tokio::sync::Notify>>,
    pub requests: Mutex<Vec<DecideRequest>>,
}

impl ScriptedDecider {
    pub fn new(outcome: Outcome) -> Arc<Self> {
        Arc::new(Self {
            outcome: Mutex::new(outcome),
            calls: AtomicUsize::new(0),
            gate: None,
            requests: Mutex::new(Vec::new()),
        })
    }

    pub fn gated(outcome: Outcome) -> (Arc<Self>, Arc<tokio::sync::Notify>) {
        let gate = Arc::new(tokio::sync::Notify::new());
        let decider = Arc::new(Self {
            outcome: Mutex::new(outcome),
            calls: AtomicUsize::new(0),
            gate: Some(gate.clone()),
            requests: Mutex::new(Vec::new()),
        });
        (decider, gate)
    }

    pub fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl Decider for ScriptedDecider {
    fn decide(&self, request: DecideRequest) -> BoxFuture<'_, Outcome> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            self.requests.lock().unwrap().push(request);
            if let Some(gate) = &self.gate {
                gate.notified().await;
            }
            self.outcome.lock().unwrap().clone()
        })
    }
}

pub fn stuck_decision() -> Outcome {
    let mut answers = BTreeMap::new();
    answers.insert("stuck".to_owned(), Answer::Noul { noul: 0.95 });
    answers.insert("progress".to_owned(), Answer::Noul { noul: 0.1 });
    answers.insert(
        "redirect".to_owned(),
        Answer::Choice {
            choice: "reread_error".into(),
            confidence: Some(0.9),
            probabilities: None,
        },
    );
    Outcome::Decision(Decision {
        model_version: MODEL.into(),
        requested_model: MODEL.into(),
        provider: None,
        answers,
        usage: tkm_decide::api::Usage {
            input_tokens: 100,
            output_tokens: 5,
            cost: Some(0.00001),
        },
        latency_ms: 12,
        cached: false,
    })
}

/// Backing that counts every database open and delegates to a real SQLite file.
pub struct CountingBacking {
    pub inner: SqliteBacking,
    pub opens: AtomicUsize,
}

impl CountingBacking {
    pub fn new(path: PathBuf) -> Arc<Self> {
        Arc::new(Self {
            inner: SqliteBacking::new(path),
            opens: AtomicUsize::new(0),
        })
    }

    pub fn opens(&self) -> usize {
        self.opens.load(Ordering::SeqCst)
    }
}

impl Backing for CountingBacking {
    fn open(&self) -> Result<Store, StoreError> {
        self.opens.fetch_add(1, Ordering::SeqCst);
        self.inner.open()
    }
}

/// Polls `check` every 20 ms for up to `secs` seconds.
pub async fn eventually<F: FnMut() -> bool>(secs: u64, mut check: F) -> bool {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(secs);
    while std::time::Instant::now() < deadline {
        if check() {
            return true;
        }
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    }
    check()
}

pub type Fut<T> = Pin<Box<dyn Future<Output = T> + Send>>;

/// Minimal HTTP/1.1 server answering every POST with `status` and `body` (after `delay`).
pub async fn fake_openrouter(
    status: u16,
    body: String,
    delay: std::time::Duration,
) -> (String, Arc<AtomicUsize>) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!(
        "http://{}/api/alpha/decisions",
        listener.local_addr().unwrap()
    );
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                return;
            };
            let body = body.clone();
            let counter = counter.clone();
            tokio::spawn(async move {
                let mut buffer = vec![0_u8; 65_536];
                let mut seen = Vec::new();
                loop {
                    let Ok(read) = stream.read(&mut buffer).await else {
                        return;
                    };
                    if read == 0 {
                        return;
                    }
                    seen.extend_from_slice(&buffer[..read]);
                    let text = String::from_utf8_lossy(&seen).into_owned();
                    if let Some(split) = text.find("\r\n\r\n") {
                        let length = text[..split]
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length:")
                                    .and_then(|v| v.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        if seen.len() >= split + 4 + length {
                            break;
                        }
                    }
                }
                counter.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(delay).await;
                let reply = format!(
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = stream.write_all(reply.as_bytes()).await;
                let _ = stream.shutdown().await;
            });
        }
    });
    (url, hits)
}

pub fn decision_body() -> String {
    json!({
        "model": MODEL,
        "answers": {
            "stuck": {"type": "noul", "noul": 0.95},
            "progress": {"type": "noul", "noul": 0.1},
            "redirect": {"type": "choice", "choice": "reread_error", "confidence": 0.9}
        },
        "usage": {"input_tokens": 100, "output_tokens": 5, "cost": 0.00001}
    })
    .to_string()
}

/// Seeds an active, injectable boundary fact for the fixture repository.
pub fn seed_fact(env: &Env, host_name: &str) {
    use tkm_store::{FactRecord, FactState};
    let store = env.store();
    seed_repo(&store, FIXTURE_REPO);
    let repo = RepoKey(FIXTURE_REPO.into());
    let id = format!("boundary:{host_name}");
    store
        .insert_candidate_fact(&FactRecord {
            repo_key: repo.clone(),
            fact_id: id.clone(),
            family: "boundary".into(),
            scope: "\"neutral\"".into(),
            text_params: json!({"params": {"hosts": {host_name: 2}}, "contradiction_streak": 0}),
            state: FactState::Candidate,
            score: 3.0,
            confirmations: 3,
            contradictions: 0,
            fire_count: 3,
            fire_sessions: 4,
        })
        .unwrap();
    store
        .transition_fact(&repo, &id, FactState::Active)
        .unwrap();
}

pub fn fact_line(host_name: &str) -> String {
    format!(
        "Outbound connections to {host_name} are refused here; nothing can be fetched from them."
    )
}

/// Claude adapter that cannot inject on `prompt` (like Cursor's `beforeSubmitPrompt`).
pub struct PromptBlind(pub tkm_hosts::claude::ClaudeAdapter);

impl tkm_hosts::HostAdapter for PromptBlind {
    fn host(&self) -> Host {
        Host::Claude
    }
    fn map_event(
        &self,
        request: &Request,
    ) -> Result<Option<tkm_core::model::HookEvent>, tkm_hosts::AdapterError> {
        self.0.map_event(request)
    }
    fn render_reply(&self, request: &Request, action: &tkm_proto::ReplyAction) -> String {
        if request.event == EventKind::Prompt {
            return String::new();
        }
        self.0.render_reply(request, action)
    }
    fn read_session(
        &self,
        source: &tkm_hosts::SessionSource,
    ) -> Result<tkm_core::model::Session, tkm_hosts::AdapterError> {
        self.0.read_session(source)
    }
    fn install(
        &self,
        context: &tkm_hosts::InstallContext,
    ) -> Result<tkm_hosts::InstallReport, tkm_hosts::AdapterError> {
        self.0.install(context)
    }
    fn uninstall(
        &self,
        context: &tkm_hosts::InstallContext,
    ) -> Result<tkm_hosts::InstallReport, tkm_hosts::AdapterError> {
        self.0.uninstall(context)
    }
    fn prune_target(&self) -> tkm_hosts::PruneTarget {
        self.0.prune_target()
    }
}
