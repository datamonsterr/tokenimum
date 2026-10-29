//! `tkm run` wrapper library for fold-only mode (SPEC 6.12, ADR 0026).
//!
//! The CLI command is epic E7. This module builds the child's environment,
//! spawns it with inherited stdio, and reports start and end to an observer,
//! which triggers the immediate fold. It reads no clock itself: the caller
//! supplies a [`Clock`].

use crate::AdapterError;
use std::ffi::OsString;
use std::process::Command;
use tkm_core::model::Timestamp;
use tkm_proto::Host;

/// Time source supplied by the caller.
pub trait Clock {
    /// Current time.
    fn now(&self) -> Timestamp;
}

/// A resolved harness launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunPlan {
    /// Program to execute.
    pub program: OsString,
    /// Arguments passed verbatim.
    pub args: Vec<OsString>,
    /// Variables added to the inherited environment; they override equal names.
    pub env: Vec<(String, String)>,
    /// Harness being wrapped.
    pub host: Host,
    /// Session id exported as `TKM_SESSION_ID`.
    pub session_id: String,
}

/// Result of a finished run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunOutcome {
    /// Exit code, or `None` when a signal ended the process.
    pub exit_code: Option<i32>,
    /// Unix signal number if the child was terminated by a signal.
    pub signal: Option<i32>,
    /// Time reported just after a successful spawn.
    pub started_at: Timestamp,
    /// Time reported just after the process exited.
    pub ended_at: Timestamp,
}

/// Receives run lifecycle events; the end event is the immediate-fold trigger.
pub trait RunObserver {
    /// Called once, after the process spawned.
    fn started(&mut self, plan: &RunPlan, at: Timestamp);
    /// Called once, after the process exited. Never called when the spawn failed.
    fn ended(&mut self, plan: &RunPlan, at: Timestamp, exit_code: Option<i32>);
}

/// Builds the plan, adding `TKM_SESSION_ID` and `TKM_HOST` (`Host::as_str`).
#[must_use]
pub fn plan<A: Into<OsString>>(
    host: Host,
    program: impl Into<OsString>,
    args: impl IntoIterator<Item = A>,
    session_id: &str,
) -> RunPlan {
    RunPlan {
        program: program.into(),
        args: args.into_iter().map(Into::into).collect(),
        env: vec![
            ("TKM_SESSION_ID".to_owned(), session_id.to_owned()),
            ("TKM_HOST".to_owned(), host.as_str().to_owned()),
        ],
        host,
        session_id: session_id.to_owned(),
    }
}

/// Runs the plan with inherited stdio and waits for it to exit.
///
/// Signals: Ctrl-C reaches the child and this process alike (same foreground
/// process group). The caller (`tkm run`, epic E7) must ignore SIGINT in the
/// parent while the child runs, so the parent survives to see the exit and fire
/// `ended`. This library does not touch signal handlers (it forbids `unsafe`).
///
/// # Errors
/// [`AdapterError::Io`] when the spawn fails (the observer sees nothing) or the
/// wait fails (the observer saw `started` but gets no `ended`).
pub fn execute(
    plan: &RunPlan,
    clock: &impl Clock,
    observer: &mut impl RunObserver,
) -> Result<RunOutcome, AdapterError> {
    let mut child = Command::new(&plan.program)
        .args(&plan.args)
        .envs(plan.env.iter().map(|(name, value)| (name, value)))
        .spawn()?;
    let started_at = clock.now();
    observer.started(plan, started_at);
    let status = child.wait()?;
    let exit_code = status.code();
    #[cfg(unix)]
    let signal = {
        use std::os::unix::process::ExitStatusExt;
        status.signal()
    };
    #[cfg(not(unix))]
    let signal = None;
    let ended_at = clock.now();
    observer.ended(plan, ended_at, exit_code);
    Ok(RunOutcome {
        exit_code,
        signal,
        started_at,
        ended_at,
    })
}
