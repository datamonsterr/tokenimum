//! Expected hook events per host, consumed by `tkm doctor --probe` (ADR 0020).

use tkm_proto::{EventKind, Host};

/// One hook event `tkm doctor --probe` expects from a host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExpectedEvent {
    /// Native host event name, e.g. `UserPromptSubmit`.
    pub native: &'static str,
    /// Normalized event kind.
    pub kind: EventKind,
    /// False when documented but unobserved/unreliable; absence then only switches the event to idle-fold.
    pub required: bool,
    /// Short reason or caveat shown by doctor; empty when none.
    pub note: &'static str,
}

const fn required(native: &'static str, kind: EventKind) -> ExpectedEvent {
    ExpectedEvent {
        native,
        kind,
        required: true,
        note: "",
    }
}

const CLAUDE: &[ExpectedEvent] = &[
    required("SessionStart", EventKind::SessionStart),
    required("UserPromptSubmit", EventKind::Prompt),
    required("PostToolUse", EventKind::ToolDone),
    required("PostToolUseFailure", EventKind::ToolDone),
    required("Stop", EventKind::TurnEnd),
    required("SessionEnd", EventKind::SessionEnd),
];

const CODEX: &[ExpectedEvent] = &[
    required("SessionStart", EventKind::SessionStart),
    required("UserPromptSubmit", EventKind::Prompt),
    required("PostToolUse", EventKind::ToolDone),
    required("Stop", EventKind::TurnEnd),
    required("SessionEnd", EventKind::SessionEnd),
];

/// Expected events for `host`, in install order.
#[must_use]
pub fn expected_events(host: Host) -> &'static [ExpectedEvent] {
    match host {
        Host::Claude => CLAUDE,
        Host::Codex => CODEX,
        Host::Agy => crate::agy::EXPECTED_EVENTS,
        Host::Cursor => crate::cursor::EXPECTED_EVENTS,
        Host::Kimi => crate::kimi::EXPECTED_EVENTS,
        Host::Deepseek => crate::codewhale::EXPECTED_EVENTS,
        Host::Opencode => OPENCODE,
        Host::Pi => PI,
        _ => &[],
    }
}

const OPENCODE: &[ExpectedEvent] = &[
    required("session.created", EventKind::SessionStart),
    required("chat.message", EventKind::Prompt),
    required("tool.execute.after", EventKind::ToolDone),
    required("session.idle", EventKind::TurnEnd),
    required("session.deleted", EventKind::SessionEnd),
];
const PI: &[ExpectedEvent] = &[
    required("session_start", EventKind::SessionStart),
    required("before_agent_start", EventKind::Prompt),
    required("tool_result", EventKind::ToolDone),
    required("agent_end", EventKind::TurnEnd),
    required("session_shutdown", EventKind::SessionEnd),
];

/// Documented one-shot arguments, excluding the probe prompt.
/// Codewhale exec omits lifecycle hooks; Codex's fresh HOME has no persisted hook trust.
/// Both therefore use recorded live events rather than a misleading headless result.
#[must_use]
pub fn headless_args(host: Host) -> Option<&'static [&'static str]> {
    match host {
        Host::Claude | Host::Agy | Host::Cursor => Some(&["-p"]),
        Host::Kimi => Some(&["--print", "--prompt"]),
        Host::Opencode => Some(&["run"]),
        Host::Pi => Some(&["--print"]),
        _ => None,
    }
}
