//! Antigravity CLI (agy) hook mapping, advisory replies, transcript reading, and installation.

mod install;
mod map;
mod reader;
mod reply;

use std::path::{Component, Path, PathBuf};
use tkm_core::model::{HookEvent, Session};
use tkm_proto::{Host, PROTOCOL_VERSION, ReplyAction, Request};

use crate::{AdapterError, HostAdapter, InstallContext, InstallReport, PruneTarget, SessionSource};

/// Stateless Antigravity CLI adapter; all filesystem roots are supplied by callers.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct AgyAdapter;

impl AgyAdapter {
    /// Creates an Antigravity CLI adapter without reading configuration or the clock.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl HostAdapter for AgyAdapter {
    fn host(&self) -> Host {
        Host::Agy
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

/// Hook events expected from agy.
pub const EXPECTED_EVENTS: &[crate::probe::ExpectedEvent] = &[
    crate::probe::ExpectedEvent {
        native: "SessionStart",
        kind: tkm_proto::EventKind::SessionStart,
        required: true,
        note: "",
    },
    crate::probe::ExpectedEvent {
        native: "PreInvocation",
        kind: tkm_proto::EventKind::Prompt,
        required: true,
        note: "",
    },
    crate::probe::ExpectedEvent {
        native: "PostToolUse",
        kind: tkm_proto::EventKind::ToolDone,
        required: false,
        note: "not observed under agy -p 1.4.9; calls come from the transcript",
    },
    crate::probe::ExpectedEvent {
        native: "Stop",
        kind: tkm_proto::EventKind::TurnEnd,
        required: true,
        note: "",
    },
];

/// Environment markers forwarded for agy.
pub const ENV_MARKERS: &[&str] = &["ANTIGRAVITY_CONVERSATION_ID"];

pub(crate) fn repo_relative(path: &str, cwd: Option<&Path>) -> Option<PathBuf> {
    let path = Path::new(path.trim());
    let inner = match cwd.and_then(|root| path.strip_prefix(root).ok()) {
        Some(rest) => rest,
        None if path.is_absolute() => return None,
        None => path,
    };
    let mut clean = PathBuf::new();
    for component in inner.components() {
        match component {
            Component::Normal(part) => clean.push(part),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!clean.as_os_str().is_empty()).then_some(clean)
}

fn native_event(request: &Request) -> Result<&str, AdapterError> {
    if request.version != PROTOCOL_VERSION {
        return Err(crate::writer::invalid("unsupported protocol version"));
    }
    if request.host != Host::Agy {
        return Err(crate::writer::invalid("host does not match Agy"));
    }
    if !request.payload.is_object() {
        return Err(crate::writer::invalid("payload must be an object"));
    }
    request
        .host_event
        .as_deref()
        .ok_or_else(|| crate::writer::invalid("native event is missing"))
}
