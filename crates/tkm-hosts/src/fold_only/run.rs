//! Run wrapper (stub for the red commit).
use crate::AdapterError;
use std::ffi::OsString;
use tkm_core::model::Timestamp;
use tkm_proto::Host;

/// Time source.
pub trait Clock {
    /// Now.
    fn now(&self) -> Timestamp;
}
/// Plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunPlan {
    /// Program.
    pub program: OsString,
    /// Args.
    pub args: Vec<OsString>,
    /// Env.
    pub env: Vec<(String, String)>,
    /// Host.
    pub host: Host,
    /// Session.
    pub session_id: String,
}
/// Outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunOutcome {
    /// Exit.
    pub exit_code: Option<i32>,
    /// Start.
    pub started_at: Timestamp,
    /// End.
    pub ended_at: Timestamp,
}
/// Observer.
pub trait RunObserver {
    /// Started.
    fn started(&mut self, plan: &RunPlan, at: Timestamp);
    /// Ended.
    fn ended(&mut self, plan: &RunPlan, at: Timestamp, exit_code: Option<i32>);
}
/// Stub.
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
        env: Vec::new(),
        host,
        session_id: session_id.to_owned(),
    }
}
/// Stub.
///
/// # Errors
/// Always unsupported.
pub fn execute(
    _plan: &RunPlan,
    _clock: &impl Clock,
    _observer: &mut impl RunObserver,
) -> Result<RunOutcome, AdapterError> {
    Err(AdapterError::Unsupported {
        operation: "stub".into(),
    })
}
