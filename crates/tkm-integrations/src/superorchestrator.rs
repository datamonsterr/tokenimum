//! Superorchestrator account, run, escalation, and spend integration.
use crate::{
    EscalationEvent, Escalator,
    detect::{AdapterHealth, Clock},
    process,
};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashMap},
    ffi::{OsStr, OsString},
    fmt,
    fs::{self, OpenOptions},
    io::{self, BufRead, BufReader, Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{Arc, Mutex, mpsc},
    thread,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tkm_proto::Host;

/// An account home suitable for host installation.
#[derive(Debug, Clone, Deserialize)]
pub struct AccountHome {
    /// Account name.
    pub name: String,
    /// Harness identifier.
    pub harness: String,
    /// Configuration home.
    pub home: PathBuf,
    /// Mapped supported host.
    #[serde(skip)]
    pub host: Option<Host>,
    /// Whether account is enabled.
    pub enabled: bool,
}
/// Reads supported fields from account TOML files, ignoring malformed files.
#[must_use]
pub fn account_homes(config_dir: &Path) -> Vec<AccountHome> {
    let Ok(entries) = fs::read_dir(config_dir.join("accounts")) else {
        return Vec::new();
    };
    let mut paths: Vec<_> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "toml"))
        .collect();
    paths.sort();
    paths
        .into_iter()
        .filter_map(|p| {
            let text = fs::read_to_string(p).ok()?;
            let mut account: AccountHome = toml::from_str(&text).ok()?;
            account.host = match account.harness.as_str() {
                "claude" => Some(Host::Claude),
                "codex" => Some(Host::Codex),
                "agy" => Some(Host::Agy),
                "opencode" => Some(Host::Opencode),
                "cursor" => Some(Host::Cursor),
                "kimi" => Some(Host::Kimi),
                "codewhale" | "deepseek" => Some(Host::Deepseek),
                "pi" => Some(Host::Pi),
                _ => None,
            };
            Some(account)
        })
        .collect()
}
/// Installer outcome for one account.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallOutcome {
    /// New entries installed.
    Installed,
    /// Entries already existed.
    AlreadyPresent,
}
/// Account installation counts.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct InstallReport {
    /// Newly installed accounts.
    pub installed: usize,
    /// Previously installed accounts.
    pub already_present: usize,
    /// Unsupported or disabled accounts skipped.
    pub skipped_unsupported: usize,
    /// Failed installations.
    pub failed: usize,
}
/// Installs each enabled, supported account using the supplied installer.
#[must_use]
pub fn install_account_homes<E>(
    homes: &[AccountHome],
    mut installer: impl FnMut(Host, &Path) -> Result<InstallOutcome, E>,
) -> InstallReport {
    let mut report = InstallReport::default();
    for home in homes {
        if let Some(host) = home.host.filter(|_| home.enabled) {
            match installer(host, &home.home) {
                Ok(InstallOutcome::Installed) => report.installed += 1,
                Ok(InstallOutcome::AlreadyPresent) => report.already_present += 1,
                Err(_) => report.failed += 1,
            }
        } else {
            report.skipped_unsupported += 1;
        }
    }
    report
}
/// A secret that never displays its contents.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);
impl Secret {
    /// Wraps a secret string.
    #[must_use]
    pub fn new(value: String) -> Self {
        Self(value)
    }
}
impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}
impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}
/// A delegated run and its optional authentication context.
#[derive(Debug, Clone)]
pub struct DelegatedRun {
    /// Run identifier.
    pub run_id: String,
    /// Run authentication token.
    pub token: Option<Secret>,
    /// Orchestrator socket.
    pub socket: Option<String>,
    /// Orchestrator configuration directory.
    pub config_dir: Option<String>,
}
#[derive(Deserialize)]
struct RunRow {
    id: String,
    state: String,
    cwd: PathBuf,
    worktree: Option<PathBuf>,
}
type RunCache = (Option<Duration>, Vec<(PathBuf, DelegatedRun)>);
/// Cached run list for cwd fallback detection.
pub struct RunDetector {
    clock: Arc<dyn Clock>,
    cache: Mutex<RunCache>,
}
impl RunDetector {
    /// Creates an empty cache.
    #[must_use]
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            clock,
            cache: Mutex::new((None, Vec::new())),
        }
    }
    /// Extracts primary run identity from a hook environment.
    #[must_use]
    pub fn from_env(env: &BTreeMap<String, String>) -> Option<DelegatedRun> {
        let run_id = env.get("SUPERORCHESTRATOR_RUN_ID")?.clone();
        if run_id.is_empty() {
            return None;
        }
        Some(DelegatedRun {
            run_id,
            token: env
                .get("SUPERORCHESTRATOR_RUN_TOKEN")
                .filter(|s| !s.is_empty())
                .cloned()
                .map(Secret),
            socket: env.get("SUPERORCHESTRATOR_SOCKET").cloned(),
            config_dir: env.get("SUPERORCHESTRATOR_CONFIG_DIR").cloned(),
        })
    }
    /// Refreshes the run cache no more than once per thirty seconds.
    ///
    /// # Errors
    /// Returns an error if the command or its JSON fails.
    pub fn refresh_runs(&self, path: &OsStr) -> io::Result<()> {
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| io::Error::other("cache poisoned"))?;
        let now = self.clock.now();
        if cache
            .0
            .is_some_and(|last| now.saturating_sub(last) < Duration::from_secs(30))
        {
            return Ok(());
        }
        cache.0 = Some(now);
        drop(cache);
        let bin = process::binary(path, "superorchestrator")
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "binary missing"))?;
        let mut cmd = Command::new(bin);
        cmd.args(["runs", "list", "--all", "--json"])
            .env("PATH", path);
        let bytes = process::output(cmd, Duration::from_secs(5))?;
        let rows: Vec<RunRow> = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        let mut cache = self
            .cache
            .lock()
            .map_err(|_| io::Error::other("cache poisoned"))?;
        cache.1 = rows
            .into_iter()
            .filter(|r| matches!(r.state.as_str(), "running" | "queued" | "waiting"))
            .map(|r| {
                (
                    r.worktree.unwrap_or(r.cwd),
                    DelegatedRun {
                        run_id: r.id,
                        token: None,
                        socket: None,
                        config_dir: None,
                    },
                )
            })
            .collect();
        Ok(())
    }
    /// Finds the deepest cached ancestor of a session directory.
    #[must_use]
    pub fn match_cwd(&self, cwd: &Path) -> Option<DelegatedRun> {
        let cache = self.cache.lock().ok()?;
        cache
            .1
            .iter()
            .filter(|(p, _)| cwd.starts_with(p))
            .max_by_key(|(p, _)| p.components().count())
            .map(|(_, run)| run.clone())
    }
}
fn write_message(input: &mut impl Write, value: &serde_json::Value) -> io::Result<()> {
    writeln!(input, "{value}")?;
    input.flush()
}
fn response(reader: &mut impl BufRead, id: u64) -> io::Result<()> {
    loop {
        let mut bytes = Vec::new();
        if (&mut *reader)
            .take(256 * 1024 + 1)
            .read_until(b'\n', &mut bytes)?
            == 0
        {
            return Err(io::Error::new(io::ErrorKind::UnexpectedEof, "MCP closed"));
        }
        if bytes.len() > 256 * 1024 {
            return Err(io::Error::other("MCP frame too large"));
        }
        let value: serde_json::Value = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        if value.get("id").and_then(serde_json::Value::as_u64) == Some(id) {
            return if value.get("error").is_some()
                || !value
                    .get("result")
                    .is_some_and(serde_json::Value::is_object)
                || (id == 1
                    && value
                        .pointer("/result/protocolVersion")
                        .and_then(serde_json::Value::as_str)
                        .is_none())
                || (id == 2
                    && value
                        .pointer("/result/content")
                        .and_then(serde_json::Value::as_array)
                        .is_none_or(Vec::is_empty)
                    && !value
                        .pointer("/result/structuredContent")
                        .is_some_and(serde_json::Value::is_object))
                || value
                    .pointer("/result/isError")
                    .and_then(serde_json::Value::as_bool)
                    == Some(true)
            {
                Err(io::Error::other("MCP request failed"))
            } else {
                Ok(())
            };
        }
    }
}

/// Sends delegated loop escalation events through the MCP server.
pub struct SuperorchestratorEscalator {
    path: OsString,
    home: PathBuf,
    health: Arc<AdapterHealth>,
    sessions: Mutex<HashMap<(Host, String), DelegatedRun>>,
}
impl SuperorchestratorEscalator {
    /// Creates an escalation adapter.
    #[must_use]
    pub fn new(path: OsString, home: PathBuf, health: Arc<AdapterHealth>) -> Self {
        Self {
            path,
            home,
            health,
            sessions: Mutex::new(HashMap::new()),
        }
    }
    fn send(&self, run: &DelegatedRun, event: &EscalationEvent) -> io::Result<()> {
        let bin = process::binary(&self.path, "superorchestrator")
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "binary missing"))?;
        let mut cmd = Command::new(bin);
        cmd.arg("mcp")
            .env_clear()
            .env("PATH", &self.path)
            .env("HOME", &self.home)
            .env("SUPERORCHESTRATOR_RUN_ID", &run.run_id)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        if let Some(token) = &run.token {
            cmd.env("SUPERORCHESTRATOR_RUN_TOKEN", &token.0);
        }
        if let Some(socket) = &run.socket {
            cmd.env("SUPERORCHESTRATOR_SOCKET", socket);
        }
        if let Some(config) = &run.config_dir {
            cmd.env("SUPERORCHESTRATOR_CONFIG_DIR", config);
        }
        process::prepare_group(&mut cmd);
        let mut child = cmd.spawn()?;
        let Some(mut input) = child.stdin.take() else {
            process::kill_group(&mut child);
            let _ = child.wait();
            return Err(io::Error::other("missing stdin"));
        };
        let Some(output) = child.stdout.take() else {
            process::kill_group(&mut child);
            let _ = child.wait();
            return Err(io::Error::other("missing stdout"));
        };
        let note = serde_json::json!({"kind":"tokenimum.loop","run_id":run.run_id,"repo_key":event.repo,"turn_calls":event.turn_calls,"envelope":event.envelope,"stuck":event.stuck,"progress":event.progress,"redirect_given":event.redirect_given,"suggest":event.suggest,"suggest_probs":event.suggest_probs}).to_string();
        let status = serde_json::to_value(event.suggest).map_err(io::Error::other)?;
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let result = (|| -> io::Result<()> {
                let mut reader = BufReader::new(output);
                write_message(
                    &mut input,
                    &serde_json::json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"tokenimum","version":"0.0.0"}}}),
                )?;
                response(&mut reader, 1)?;
                write_message(
                    &mut input,
                    &serde_json::json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
                )?;
                write_message(
                    &mut input,
                    &serde_json::json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"report_progress","arguments":{"step":"tokenimum.loop","status":status,"note":note}}}),
                )?;
                response(&mut reader, 2)
            })();
            let _ = tx.send(result);
        });
        let deadline = Instant::now() + Duration::from_secs(5);
        let result = rx
            .recv_timeout(deadline.saturating_duration_since(Instant::now()))
            .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "MCP timeout"));
        process::kill_group(&mut child);
        let _ = child.wait();
        result?
    }
}
impl Escalator for SuperorchestratorEscalator {
    fn forget_session(&self, host: Host, session_id: &str) {
        if let Ok(mut sessions) = self.sessions.lock() {
            sessions.remove(&(host, session_id.to_owned()));
        }
    }
    fn delegated_run_for(&self, host: Host, session_id: &str) -> Option<DelegatedRun> {
        self.sessions
            .lock()
            .ok()?
            .get(&(host, session_id.to_owned()))
            .cloned()
    }
    fn observe_session(
        &self,
        host: Host,
        session_id: &str,
        _: &Path,
        env: &BTreeMap<String, String>,
    ) {
        if let Ok(mut sessions) = self.sessions.lock() {
            let key = (host, session_id.to_owned());
            if let Some(run) = RunDetector::from_env(env) {
                sessions.insert(key, run);
            } else {
                sessions.remove(&key);
            }
        }
    }
    fn run_id(&self, host: Host, session_id: &str) -> Option<String> {
        self.sessions
            .lock()
            .ok()?
            .get(&(host, session_id.to_owned()))
            .map(|run| run.run_id.clone())
    }
    fn handles(&self, host: Host, session_id: &str) -> bool {
        !self.health.in_backoff()
            && self
                .sessions
                .lock()
                .ok()
                .and_then(|s| s.get(&(host, session_id.to_owned())).cloned())
                .is_some_and(|r| r.token.is_some())
    }
    fn escalate(&self, event: &EscalationEvent) {
        let _ = self.try_escalate(event);
    }
    fn try_escalate(&self, event: &EscalationEvent) -> bool {
        let run = self.delegated_run_for(event.host, &event.session_id);
        self.try_escalate_with_run(event, run.as_ref())
    }
    fn try_escalate_with_run(&self, event: &EscalationEvent, run: Option<&DelegatedRun>) -> bool {
        if self.health.in_backoff() {
            return false;
        }
        let Some(run) = run.filter(|r| r.token.is_some()) else {
            return false;
        };
        if self.send(run, event).is_err() {
            self.health
                .report_failure("superorchestrator escalation failed");
            tracing::warn!("superorchestrator escalation failed");
            return false;
        }
        true
    }
}
/// Decision spend sink.
pub trait SpendSink: Send + Sync {
    /// Records one decision's cost.
    fn record(&self, purpose: &str, model: &str, cost_usd: f64, run_id: Option<&str>);
    /// Completes buffered writes, if the sink buffers records.
    fn flush(&self) {}
}
/// Spend sink that discards records.
#[derive(Default)]
pub struct NoSpend;
impl SpendSink for NoSpend {
    fn record(&self, _: &str, _: &str, _: f64, _: Option<&str>) {}
}
/// Appends decision spend as JSON lines.
pub struct SpendReporter {
    path: PathBuf,
    lock: Mutex<()>,
}
impl SpendReporter {
    /// Creates a reporter for a tokenimum-owned file.
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self {
            path,
            lock: Mutex::new(()),
        }
    }
    fn write(
        &self,
        purpose: &str,
        model: &str,
        cost_usd: f64,
        run_id: Option<&str>,
    ) -> io::Result<()> {
        let _guard = self
            .lock
            .lock()
            .map_err(|_| io::Error::other("spend lock poisoned"))?;
        if let Some(parent) = self.path.parent().filter(|p| !p.as_os_str().is_empty()) {
            fs::create_dir_all(parent)?;
        }
        let mut file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        writeln!(
            file,
            "{}",
            serde_json::json!({"account":"tokenimum-decisions","ts":ts,"purpose":purpose,"model":model,"cost_usd":cost_usd,"run_id":run_id})
        )
    }
}
impl SpendSink for SpendReporter {
    fn record(&self, purpose: &str, model: &str, cost_usd: f64, run_id: Option<&str>) {
        if self.write(purpose, model, cost_usd, run_id).is_err() {
            tracing::warn!("failed to record integration spend");
        }
    }
}
