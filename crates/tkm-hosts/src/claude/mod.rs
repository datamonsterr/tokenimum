//! Claude Code hook mapping, advisory replies, transcript reading and installation.

mod install;
pub(crate) use install::DENY_PATH;
mod map;
mod reader;
mod reply;
pub(crate) mod time;

use crate::writer::invalid;
use crate::{AdapterError, HostAdapter, InstallContext, InstallReport, PruneTarget, SessionSource};
use tkm_core::model::{HookEvent, Session};
use tkm_proto::{Host, PROTOCOL_VERSION, ReplyAction, Request};

/// Stateless Claude Code adapter; all filesystem roots are supplied by callers.
#[derive(Debug, Default, Clone, Copy)]
pub struct ClaudeAdapter;

impl ClaudeAdapter {
    /// Creates a Claude Code adapter without reading configuration or the clock.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl HostAdapter for ClaudeAdapter {
    fn host(&self) -> Host {
        Host::Claude
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
        PruneTarget::SettingsDenyList {
            path: ".claude/settings.local.json".into(),
            key: "permissions.deny".to_owned(),
        }
    }
}

fn native_event(request: &Request) -> Result<&str, AdapterError> {
    if request.version != PROTOCOL_VERSION {
        return Err(invalid("unsupported protocol version"));
    }
    if request.host != Host::Claude {
        return Err(invalid("host does not match Claude"));
    }
    if !request.payload.is_object() {
        return Err(invalid("payload must be an object"));
    }
    if request
        .payload
        .get("session_id")
        .and_then(serde_json::Value::as_str)
        .is_none()
    {
        return Err(invalid("required payload string is missing or invalid"));
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

/// Parse a Claude transcript RFC3339 timestamp, including offsets and fractions.
///
/// Benchmark readers use this for agent records that the normalized reader skips.
#[must_use]
pub fn parse_timestamp(text: &str) -> Option<tkm_core::model::Timestamp> {
    time::parse(text)
}
