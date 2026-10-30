//! Cursor adapter: hook mapping, replies, transcript reading, and installation.

mod install;
pub(crate) use install::DENY_PATH;
mod map;
mod reader;
mod reply;

use std::path::PathBuf;

use tkm_core::model::{HookEvent, Session};
use tkm_proto::{EventKind, Host, PROTOCOL_VERSION, ReplyAction, Request};

use crate::writer::invalid;
use crate::{AdapterError, HostAdapter, InstallContext, InstallReport, PruneTarget, SessionSource};

/// Environment markers forwarded for Cursor sessions.
pub const ENV_MARKERS: &[&str] = &[
    "CURSOR_PROJECT_DIR",
    "CURSOR_VERSION",
    "CURSOR_TRANSCRIPT_PATH",
];

/// Expected events and their requirement statuses for `tkm doctor --probe`.
pub const EXPECTED_EVENTS: &[crate::probe::ExpectedEvent] = &[
    crate::probe::ExpectedEvent {
        native: "sessionStart",
        kind: EventKind::SessionStart,
        required: true,
        note: "",
    },
    crate::probe::ExpectedEvent {
        native: "beforeSubmitPrompt",
        kind: EventKind::Prompt,
        required: false,
        note: "not fired by `agent -p` (forum)",
    },
    crate::probe::ExpectedEvent {
        native: "postToolUse",
        kind: EventKind::ToolDone,
        required: true,
        note: "",
    },
    crate::probe::ExpectedEvent {
        native: "postToolUseFailure",
        kind: EventKind::ToolDone,
        required: false,
        note: "only on failures",
    },
    crate::probe::ExpectedEvent {
        native: "stop",
        kind: EventKind::TurnEnd,
        required: false,
        note: "not fired by `agent -p` (forum)",
    },
    crate::probe::ExpectedEvent {
        native: "sessionEnd",
        kind: EventKind::SessionEnd,
        required: true,
        note: "",
    },
];

/// Returns `true` if `payload` is an object containing a string `cursor_version`.
///
/// Used by the daemon to ignore `--host claude` requests that Cursor runs
/// from Claude Code settings (ADR 0020).
#[must_use]
pub fn is_cursor_payload(payload: &serde_json::Value) -> bool {
    payload
        .as_object()
        .and_then(|obj| obj.get("cursor_version"))
        .and_then(serde_json::Value::as_str)
        .is_some()
}

/// Stateless Cursor adapter implementing [`HostAdapter`].
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CursorAdapter;

impl CursorAdapter {
    /// Creates a new Cursor adapter.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl HostAdapter for CursorAdapter {
    fn host(&self) -> Host {
        Host::Cursor
    }

    fn map_event(&self, request: &Request) -> Result<Option<HookEvent>, AdapterError> {
        map::map_event(request)
    }

    fn render_reply(&self, request: &Request, action: &ReplyAction) -> String {
        reply::render_reply(request, action)
    }

    fn read_session(&self, source: &SessionSource) -> Result<Session, AdapterError> {
        reader::read_session(source)
    }

    fn install(&self, context: &InstallContext) -> Result<InstallReport, AdapterError> {
        install::install(context)
    }

    fn uninstall(&self, context: &InstallContext) -> Result<InstallReport, AdapterError> {
        install::uninstall(context)
    }

    fn prune_target(&self) -> PruneTarget {
        PruneTarget::SettingsDenyList {
            path: PathBuf::from(".cursor/cli.json"),
            key: "permissions.deny".to_owned(),
        }
    }
}

pub(super) fn native_event(request: &Request) -> Result<&str, AdapterError> {
    if request.version != PROTOCOL_VERSION {
        return Err(invalid("unsupported protocol version"));
    }
    if request.host != Host::Cursor {
        return Err(invalid("host does not match Cursor"));
    }
    if !request.payload.is_object() {
        return Err(invalid("payload must be an object"));
    }
    if let Some(event) = request.payload.get("hook_event_name") {
        event
            .as_str()
            .ok_or_else(|| invalid("hook_event_name must be a string"))
            .and_then(|event| {
                if request
                    .host_event
                    .as_deref()
                    .is_some_and(|host| host != event)
                {
                    Err(invalid("hook_event_name conflicts with host_event"))
                } else {
                    Ok(event)
                }
            })
    } else {
        request
            .host_event
            .as_deref()
            .ok_or_else(|| invalid("native event is missing"))
    }
}
