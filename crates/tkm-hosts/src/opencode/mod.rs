//! opencode shim payload mapping, advisory replies, `opencode.db` reading and
//! installation of the generated plugin (SPEC 6.11, ADR 0025).

mod install;
pub(crate) use install::DENY_PATH;
mod map;
mod reader;
mod reply;

use crate::writer::invalid;
use crate::{AdapterError, HostAdapter, InstallContext, InstallReport, PruneTarget, SessionSource};
use serde_json::Value;
use tkm_core::model::{HookEvent, Session};
use tkm_proto::{Host, PROTOCOL_VERSION, ReplyAction, Request};

/// Stateless opencode adapter; all filesystem roots are supplied by callers.
#[derive(Debug, Default, Clone, Copy)]
pub struct OpencodeAdapter;

impl OpencodeAdapter {
    /// Creates an opencode adapter without reading configuration or the clock.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl HostAdapter for OpencodeAdapter {
    fn host(&self) -> Host {
        Host::Opencode
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
            path: "opencode.json".into(),
            key: "permission".to_owned(),
        }
    }
}

/// The plugin hook, and for `event` hooks the bus event type, of one request.
struct Native<'a> {
    hook: &'a str,
    event_type: Option<&'a str>,
}

fn native(request: &Request) -> Result<Native<'_>, AdapterError> {
    if request.version != PROTOCOL_VERSION {
        return Err(invalid("unsupported protocol version"));
    }
    if request.host != Host::Opencode {
        return Err(invalid("host does not match opencode"));
    }
    let payload = &request.payload;
    if !payload.is_object() {
        return Err(invalid("payload must be an object"));
    }
    let hook = payload
        .get("hook")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("payload hook is missing or invalid"))?;
    let event_type = if hook == "event" {
        Some(
            payload
                .pointer("/event/type")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid("event type is missing or invalid"))?,
        )
    } else {
        None
    };
    if request
        .host_event
        .as_deref()
        .is_some_and(|host| host != hook && Some(host) != event_type)
    {
        return Err(invalid("payload hook conflicts with host_event"));
    }
    Ok(Native { hook, event_type })
}
