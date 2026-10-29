//! Kimi Code hook mapping, replies, session reading, and configuration install.

mod install;
mod map;
pub mod md5;
mod reader;
mod reply;

use std::path::{Path, PathBuf};

use tkm_core::model::{HookEvent, Session};
use tkm_proto::{EventKind, Host, PROTOCOL_VERSION, ReplyAction, Request};

use crate::probe::ExpectedEvent;
use crate::{
    AdapterError, HostAdapter, InstallContext, InstallReport, PruneTarget, SessionSource, writer,
};

pub use md5::md5_hex;

/// Expected live events for the Kimi Code harness.
pub const EXPECTED_EVENTS: &[ExpectedEvent] = &[
    ExpectedEvent {
        native: "SessionStart",
        kind: EventKind::SessionStart,
        required: true,
        note: "stdout injection ignored by kimi-cli 1.52.0",
    },
    ExpectedEvent {
        native: "UserPromptSubmit",
        kind: EventKind::Prompt,
        required: true,
        note: "stdout injection ignored by kimi-cli 1.52.0",
    },
    ExpectedEvent {
        native: "PostToolUse",
        kind: EventKind::ToolDone,
        required: true,
        note: "",
    },
    ExpectedEvent {
        native: "PostToolUseFailure",
        kind: EventKind::ToolDone,
        required: false,
        note: "only on tool exceptions",
    },
    ExpectedEvent {
        native: "Stop",
        kind: EventKind::TurnEnd,
        required: true,
        note: "",
    },
    ExpectedEvent {
        native: "SessionEnd",
        kind: EventKind::SessionEnd,
        required: true,
        note: "",
    },
];

/// Environment variables forwarded from Kimi Code to `tkm-hook`.
pub const ENV_MARKERS: &[&str] = &["KIMI_SHARE_DIR"];

/// Computes the session directory path for a Kimi Code session.
///
/// Hash input is the unresolved UTF-8 bytes of `work_dir`.
#[must_use]
pub fn session_dir(share_dir: &Path, work_dir: &str, session_id: &str) -> PathBuf {
    share_dir
        .join("sessions")
        .join(md5_hex(work_dir.as_bytes()))
        .join(session_id)
}

/// Stateless Kimi Code adapter; all filesystem roots are supplied by callers.
#[derive(Debug, Default, Clone, Copy)]
pub struct KimiAdapter;

impl KimiAdapter {
    /// Creates a Kimi Code adapter without reading configuration or the clock.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl HostAdapter for KimiAdapter {
    fn host(&self) -> Host {
        Host::Kimi
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
        PruneTarget::ReportOnly
    }
}

pub(crate) fn native_event(request: &Request) -> Result<&str, AdapterError> {
    if request.version != PROTOCOL_VERSION {
        return Err(writer::invalid("unsupported protocol version"));
    }
    if request.host != Host::Kimi {
        return Err(writer::invalid("host does not match Kimi"));
    }
    if !request.payload.is_object() {
        return Err(writer::invalid("payload must be an object"));
    }
    if request
        .payload
        .get("session_id")
        .and_then(serde_json::Value::as_str)
        .is_none()
    {
        return Err(writer::invalid(
            "required payload string is missing or invalid",
        ));
    }
    if let Some(event) = request.payload.get("hook_event_name") {
        event
            .as_str()
            .ok_or_else(|| writer::invalid("hook_event_name must be a string"))
            .and_then(|event| {
                if request
                    .host_event
                    .as_deref()
                    .is_some_and(|host| host != event)
                {
                    Err(writer::invalid("hook_event_name conflicts with host_event"))
                } else {
                    Ok(event)
                }
            })
    } else {
        request
            .host_event
            .as_deref()
            .ok_or_else(|| writer::invalid("native event is missing"))
    }
}
