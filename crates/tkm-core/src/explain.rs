//! Plain-language explanations of actions.
use serde::{Deserialize, Serialize};
/// Action kind from SPEC 11.5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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
macro_rules! action_params {
    ($(($name:ident, $variant:ident, $description:literal)),+ $(,)?) => {
        $(#[doc = $description]
        #[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
        pub struct $name(
            /// Shared evidence, action detail, model, cost and undo fields.
            pub Params,
        );)+
        /// Action-specific parameters stored alongside a template identifier.
        #[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
        #[serde(tag = "kind", content = "values", rename_all = "snake_case")]
        pub enum ActionParams {
            $(#[doc = $description]
            $variant($name),)+
        }
        impl ActionParams {
            fn fields_and_kind(&self) -> (&Params, ActionKind) {
                match self { $(Self::$variant(value) => (&value.0, ActionKind::$variant),)+ }
            }
        }
    };
}
action_params!(
    (
        FactInjectedParams,
        FactInjected,
        "Parameters for fact injection."
    ),
    (
        FactPromotedParams,
        FactPromoted,
        "Parameters for fact promotion."
    ),
    (
        FactRetiredParams,
        FactRetired,
        "Parameters for fact retirement."
    ),
    (RedirectParams, Redirect, "Parameters for a redirect."),
    (EscalationParams, Escalation, "Parameters for escalation."),
    (
        DenyListProposalParams,
        DenyListProposal,
        "Parameters for a deny-list proposal."
    ),
    (
        DenyListWriteParams,
        DenyListWrite,
        "Parameters for a deny-list write."
    ),
    (RestoreParams, Restore, "Parameters for restoration."),
    (
        BudgetReachedParams,
        BudgetReached,
        "Parameters for a reached budget."
    ),
    (
        AdapterDisabledParams,
        AdapterDisabled,
        "Parameters for adapter disablement."
    ),
);
/// Stored template identity and parameters.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Explanation {
    /// Stable action template ID.
    pub template_id: ActionKind,
    /// Typed template parameters.
    pub params: ActionParams,
}
// Limit each supplied field to one line and 512 Unicode characters. Treat all
// controls and whitespace as separators, so parameters cannot forge sections.
fn clean(value: &str) -> String {
    value
        .chars()
        .take(512)
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}
fn wrap(prefix: &str, text: &str) -> String {
    let mut result = prefix.to_owned();
    let mut width = prefix.chars().count();
    for word in text.split_whitespace() {
        let len = word.chars().count();
        if width > 0 && width + 1 + len > 79 {
            result.push('\n');
            width = 0;
        } else if width > 0 && !result.ends_with(' ') {
            result.push(' ');
            width += 1;
        }
        result.push_str(word);
        width += len;
    }
    result
}
/// Render the five SPEC 11.5 sections from a code-owned action template.
/// Parameter fields are collapsed to one line and capped at 512 characters.
/// A mismatched template ID and parameter kind renders an empty string.
#[must_use]
pub fn render(explanation: &Explanation) -> String {
    let (p, kind) = explanation.params.fields_and_kind();
    if explanation.template_id != kind {
        return String::new();
    }
    let action = match explanation.template_id {
        ActionKind::FactInjected => "tokenimum added a short note to the agent's context",
        ActionKind::FactPromoted => "tokenimum promoted a repository fact",
        ActionKind::FactRetired => "tokenimum retired a repository fact",
        ActionKind::Redirect => "tokenimum redirected the agent",
        ActionKind::Escalation => "tokenimum escalated the turn",
        ActionKind::DenyListProposal => "tokenimum proposed a deny-list change",
        ActionKind::DenyListWrite => "tokenimum wrote a deny-list change",
        ActionKind::Restore => "tokenimum restored a tool",
        ActionKind::BudgetReached => "tokenimum reached a budget",
        ActionKind::AdapterDisabled => "tokenimum disabled an adapter",
    };
    let detail = clean(&p.detail);
    let detail = if explanation.template_id == ActionKind::FactInjected {
        format!("  \"{detail}\"")
    } else {
        format!("  {detail}")
    };
    format!(
        "{}\n\nWhat happened: {action}:\n{detail}\n\n{}\n\nModel checks: {}\nCost of those checks: {}.\n\nUndo: {}",
        clean(&p.event),
        wrap("Why: ", &clean(&p.why)),
        clean(&p.model_checks),
        clean(&p.cost),
        clean(&p.undo)
    )
}
