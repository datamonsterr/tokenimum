//! Pi shim payload mapping, advisory replies, session JSONL reading and
//! installation of the generated extension (SPEC 6.11, ADR 0025).

mod install;
mod map;
mod reader;
mod reply;

use crate::writer::invalid;
use crate::{AdapterError, HostAdapter, InstallContext, InstallReport, PruneTarget, SessionSource};
use serde_json::Value;
use tkm_core::model::{HookEvent, Session};
use tkm_proto::{Host, PROTOCOL_VERSION, ReplyAction, Request};

/// Stateless Pi adapter; all filesystem roots are supplied by callers.
#[derive(Debug, Default, Clone, Copy)]
pub struct PiAdapter;

impl PiAdapter {
    /// Creates a Pi adapter without reading configuration or the clock.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl HostAdapter for PiAdapter {
    fn host(&self) -> Host {
        Host::Pi
    }
    fn map_event(&self, request: &Request) -> Result<Option<HookEvent>, AdapterError> {
        map::map_event(request)
    }
    fn render_reply(&self, request: &Request, action: &ReplyAction) -> String {
        reply::render(request, action)
    }
    fn read_session(&self, source: &SessionSource) -> Result<Session, AdapterError> {
        reader::read(source)
    }
    fn install(&self, context: &InstallContext) -> Result<InstallReport, AdapterError> {
        install::install(context)
    }
    fn uninstall(&self, context: &InstallContext) -> Result<InstallReport, AdapterError> {
        install::uninstall(context)
    }
    fn prune_target(&self) -> PruneTarget {
        PruneTarget::ConfigFileKey {
            path: ".pi/settings.json".into(),
            key: "defaultTools".to_owned(),
        }
    }
}

/// Validates the request and returns the extension event name.
fn native(request: &Request) -> Result<&str, AdapterError> {
    if request.version != PROTOCOL_VERSION {
        return Err(invalid("unsupported protocol version"));
    }
    if request.host != Host::Pi {
        return Err(invalid("host does not match Pi"));
    }
    let payload = &request.payload;
    if !payload.is_object() {
        return Err(invalid("payload must be an object"));
    }
    if payload.get("session_id").and_then(Value::as_str).is_none() {
        return Err(invalid("required payload string is missing or invalid"));
    }
    let hook = payload
        .get("hook")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("payload hook is missing or invalid"))?;
    if request
        .host_event
        .as_deref()
        .is_some_and(|host| host != hook)
    {
        return Err(invalid("payload hook conflicts with host_event"));
    }
    Ok(hook)
}
