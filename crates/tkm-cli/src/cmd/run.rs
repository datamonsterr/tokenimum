//! Fold-only harness wrapper command entry point.

#[cfg(unix)]
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use tkm_core::model::Timestamp;
use tkm_hosts::fold_only::run::{self, Clock, RunObserver, RunPlan};
#[cfg(unix)]
use tkm_proto::EventKind;
use tkm_proto::Host;

use crate::{cli::RunArgs, ctx::Ctx, error::CliError};

struct SystemClock;
impl Clock for SystemClock {
    fn now(&self) -> Timestamp {
        let millis = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        Timestamp(i64::try_from(millis).unwrap_or(i64::MAX))
    }
}

struct Observer<'a, 'b> {
    ctx: &'a mut Ctx<'b>,
    failed: bool,
}

impl RunObserver for Observer<'_, '_> {
    fn started(&mut self, plan: &RunPlan, _at: Timestamp) {
        self.send(plan, false);
    }
    fn ended(&mut self, plan: &RunPlan, _at: Timestamp, _exit_code: Option<i32>) {
        self.send(plan, true);
        if self.failed {
            let _ = writeln!(
                self.ctx.stderr,
                "tkm run: daemon not running or unavailable; session events could not be delivered"
            );
        }
    }
}

impl Observer<'_, '_> {
    // Agy has no session-end hook; the daemon's idle watcher handles its fold.
    fn send(&mut self, plan: &RunPlan, end: bool) {
        if end && plan.host == Host::Agy {
            return;
        }
        #[cfg(not(unix))]
        {
            self.failed = true;
        }
        #[cfg(unix)]
        {
            let event = if end {
                EventKind::SessionEnd
            } else {
                EventKind::SessionStart
            };
            let native = match (plan.host, end) {
                (Host::Claude | Host::Codex | Host::Kimi | Host::Agy, false) => "SessionStart",
                (Host::Claude | Host::Codex | Host::Kimi, true) => "SessionEnd",
                (Host::Cursor, false) => "sessionStart",
                (Host::Cursor, true) => "sessionEnd",
                (Host::Deepseek | Host::Pi, false) => "session_start",
                (Host::Deepseek, true) => "session_end",
                (Host::Pi, true) => "session_shutdown",
                (Host::Opencode, false) => "session.created",
                (Host::Opencode, true) => "session.deleted",
                _ => return,
            };
            let payload = match plan.host {
                Host::Agy => serde_json::json!({"conversationId": plan.session_id}),
                Host::Deepseek => {
                    serde_json::json!({"event": native, "session_id": plan.session_id})
                }
                Host::Pi => serde_json::json!({"hook": native, "session_id": plan.session_id}),
                Host::Opencode => {
                    serde_json::json!({"hook": "event", "event": {"type": native, "properties": {"info": {"id": plan.session_id}}}})
                }
                _ => serde_json::json!({"hook_event_name": native, "session_id": plan.session_id}),
            };
            let mut request = tkm_hook::build_request(
                plan.host,
                event,
                payload,
                BTreeMap::new(),
                Some(self.ctx.cwd.to_string_lossy().into_owned()),
            );
            request.host_event = Some(native.to_owned());
            if !matches!(
                tkm_hook::connect_and_exchange(&self.ctx.paths.socket, &request),
                tkm_hook::ExchangeOutcome::Reply(_)
            ) {
                self.failed = true;
            }
        }
    }
}

fn program(host: Host, path: &std::ffi::OsStr) -> Option<PathBuf> {
    let names: &[&str] = match host {
        Host::Claude => &["claude"],
        Host::Codex => &["codex"],
        Host::Agy => &["agy"],
        Host::Cursor => &["agent", "cursor-agent"],
        Host::Kimi => &["kimi"],
        Host::Deepseek => &["codewhale", "deepseek"],
        Host::Opencode => &["opencode"],
        Host::Pi => &["pi"],
        _ => return None,
    };
    std::env::split_paths(path).find_map(|dir| {
        names
            .iter()
            .map(|name| dir.join(name))
            .find(|candidate| executable(candidate))
    })
}

fn executable(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        metadata.is_file()
    }
}

/// Execute `tkm run`.
///
/// # Errors
/// Returns a usage error for an unknown harness, or an I/O error if signal setup fails.
pub fn run(ctx: &mut Ctx<'_>, args: &RunArgs) -> Result<ExitCode, CliError> {
    let host: Host = match args.harness.as_str() {
        "codewhale" => "deepseek",
        "cursor-agent" | "agent" => "cursor",
        name => name,
    }
    .parse()
    .map_err(|_| CliError::Usage(format!("unknown harness: {}", args.harness)))?;
    let Some(program) = program(host, &std::env::var_os("PATH").unwrap_or_default()) else {
        writeln!(
            ctx.stderr,
            "tkm run: harness binary not found: {}",
            host.as_str()
        )?;
        return Ok(ExitCode::from(127));
    };
    #[cfg(unix)]
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_io()
        .build()?;
    #[cfg(unix)]
    let _signals = {
        use tokio::signal::unix::{SignalKind, signal};
        let _guard = runtime.enter();
        (
            signal(SignalKind::interrupt())?,
            signal(SignalKind::terminate())?,
            signal(SignalKind::hangup())?,
        )
    };
    // On non-Unix platforms signal handling is currently a no-op.
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let id = format!("{}-{now}", std::process::id());
    let plan = run::plan(host, program, args.args.clone(), &id);
    let mut observer = Observer { ctx, failed: false };
    match run::execute(&plan, &SystemClock, &mut observer) {
        Ok(outcome) => {
            let code = outcome
                .exit_code
                .or_else(|| outcome.signal.map(|signal| 128 + signal))
                .unwrap_or(1);
            Ok(ExitCode::from(u8::try_from(code).unwrap_or(1)))
        }
        Err(error) => {
            writeln!(
                observer.ctx.stderr,
                "tkm run: could not run harness: {error}"
            )?;
            Ok(ExitCode::from(127))
        }
    }
}
