//! Plain-language explanations of actions.
use serde::{Deserialize, Serialize};
/// Action kind from SPEC 11.5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionKind {
    /// Fact injected.
    FactInjected,
    /// Fact promoted.
    FactPromoted,
    /// Fact retired.
    FactRetired,
    /// Redirect.
    Redirect,
    /// Escalation.
    Escalation,
    /// Deny-list proposal.
    DenyListProposal,
    /// Deny-list write.
    DenyListWrite,
    /// Restore.
    Restore,
    /// Budget reached.
    BudgetReached,
    /// Adapter disabled.
    AdapterDisabled,
}
/// Parameters for an explanation.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Params {
    /// Event heading.
    pub event: String,
    /// Action detail.
    pub detail: String,
    /// Evidence and reason.
    pub why: String,
    /// Model checks.
    pub model_checks: String,
    /// Cost.
    pub cost: String,
    /// Undo command or explanation.
    pub undo: String,
}
/// Stored template identity and parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Explanation {
    /// Stable action template ID.
    pub template_id: ActionKind,
    /// Typed template parameters.
    pub params: Params,
}
/// Render an explanation with sanitized parameters.
#[must_use]
pub fn render(_explanation: &Explanation) -> String {
    String::new()
}
