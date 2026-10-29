//! Pure tool pruning and skill usage evidence.
use crate::model::{Host, Session, Timestamp};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
/// Pruning mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PruneMode {
    /// Disable pruning.
    Off,
    /// Report evidence.
    Report,
    /// Propose changes.
    #[default]
    Propose,
    /// Apply changes after the quiet window.
    Apply,
}
/// Evidence and scheduling configuration (SPEC 4.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PruneConfig {
    /// Prompt threshold.
    pub evidence_prompts: u64,
    /// Session minimum.
    pub min_sessions: u64,
    /// Alternate session threshold.
    pub sessions_any: u64,
    /// Idle minutes.
    pub quiet_minutes: u64,
    /// Requested mode.
    pub mode: PruneMode,
    /// User-protected names or prefix patterns.
    pub protect: Vec<String>,
    /// Whether the delegation adapter is enabled.
    pub superorchestrator_adapter_enabled: bool,
}
impl Default for PruneConfig {
    fn default() -> Self {
        Self {
            evidence_prompts: 10,
            min_sessions: 2,
            sessions_any: 5,
            quiet_minutes: 60,
            mode: PruneMode::Propose,
            protect: Vec::new(),
            superorchestrator_adapter_enabled: false,
        }
    }
}
/// Counts since the last use.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolEvidence {
    /// Unused loaded prompts.
    pub prompts: u64,
    /// Unused loaded sessions.
    pub sessions: u64,
}
/// Evidence for a single repository and host.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PruneEvidence {
    /// Tools and their unused streaks.
    pub tools: BTreeMap<String, ToolEvidence>,
}
/// Fold a session into evidence.
pub fn fold(_state: &mut PruneEvidence, _session: &Session) {}
/// Whether a tool is protected.
#[must_use]
pub fn is_protected(_name: &str, _host: Host, _config: &PruneConfig) -> bool {
    false
}
/// Whether an unused streak is ready.
#[must_use]
pub fn eligible(_evidence: &ToolEvidence, _config: &PruneConfig) -> bool {
    false
}
/// Whether the idle window or 24-hour cap permits a write.
#[must_use]
pub fn write_allowed(
    _last_activity: Timestamp,
    _ready_since: Timestamp,
    _now: Timestamp,
    _config: &PruneConfig,
) -> bool {
    false
}
/// Effective mode after host and delegation restrictions.
#[must_use]
pub fn effective_mode(
    _host: Host,
    _config: &PruneConfig,
    _last_delegated_run: Option<Timestamp>,
    _now: Timestamp,
) -> PruneMode {
    PruneMode::Off
}
/// A pure decision; it never writes settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PruneDecision {
    /// Effective mode.
    pub mode_effective: PruneMode,
    /// Eligible unprotected tools.
    pub candidates: Vec<String>,
    /// Eligible protected tools.
    pub protected_skipped: Vec<String>,
}
/// Decide from evidence.
#[must_use]
pub fn decide(
    _state: &PruneEvidence,
    _host: Host,
    _config: &PruneConfig,
    _last_delegated_run: Option<Timestamp>,
    _now: Timestamp,
) -> PruneDecision {
    PruneDecision {
        mode_effective: PruneMode::Off,
        candidates: Vec::new(),
        protected_skipped: Vec::new(),
    }
}
