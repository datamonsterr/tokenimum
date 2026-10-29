//! Codewhale (`DeepSeek` harness) hook mapping, replies, session reading, and install.

mod install;
mod map;
mod project_install;
mod reader;
mod reply;

use tkm_core::model::{HookEvent, Session};
use tkm_proto::{Host, PROTOCOL_VERSION, ReplyAction, Request};

use crate::probe::ExpectedEvent;
use crate::{AdapterError, HostAdapter, InstallContext, InstallReport, PruneTarget, SessionSource};

/// Expected live events for the Codewhale harness.
pub const EXPECTED_EVENTS: &[ExpectedEvent] = &[
    ExpectedEvent {
        native: "session_start",
        kind: tkm_proto::EventKind::SessionStart,
        required: true,
        note: "env only, cannot inject; `codewhale exec` fires only tool_call_before (with --hooks)",
    },
    ExpectedEvent {
        native: "message_submit",
        kind: tkm_proto::EventKind::Prompt,
        required: true,
        note: "`codewhale exec` fires only tool_call_before (with --hooks)",
    },
    ExpectedEvent {
        native: "tool_call_before",
        kind: tkm_proto::EventKind::ToolBefore,
        required: true,
        note: "only injection surface; `codewhale exec` fires only tool_call_before (with --hooks)",
    },
    ExpectedEvent {
        native: "tool_call_after",
        kind: tkm_proto::EventKind::ToolDone,
        required: true,
        note: "`codewhale exec` fires only tool_call_before (with --hooks)",
    },
    ExpectedEvent {
        native: "turn_end",
        kind: tkm_proto::EventKind::TurnEnd,
        required: true,
        note: "`codewhale exec` fires only tool_call_before (with --hooks)",
    },
    ExpectedEvent {
        native: "session_end",
        kind: tkm_proto::EventKind::SessionEnd,
        required: true,
        note: "`codewhale exec` fires only tool_call_before (with --hooks)",
    },
];

/// Environment variables forwarded from Codewhale to `tkm-hook`.
pub const ENV_MARKERS: &[&str] = &[
    "CODEWHALE_HOME",
    "DEEPSEEK_SESSION_ID",
    "CODEWHALE_SESSION_ID",
    "DEEPSEEK_WORKSPACE",
    "DEEPSEEK_MODEL",
    "DEEPSEEK_MODE",
    "DEEPSEEK_TOOL_NAME",
    "DEEPSEEK_TOOL_ARGS",
    "DEEPSEEK_TOOL_CALL_ID",
    "CODEWHALE_TOOL_CALL_ID",
    "DEEPSEEK_TOOL_RESULT",
    "DEEPSEEK_TOOL_SUCCESS",
    "DEEPSEEK_TOOL_EXIT_CODE",
    "DEEPSEEK_TOOL_STATUS",
    "DEEPSEEK_TOTAL_TOKENS",
];

/// Stateless Codewhale adapter; all filesystem roots are supplied by callers.
#[derive(Debug, Default, Clone, Copy)]
pub struct CodewhaleAdapter;

impl CodewhaleAdapter {
    /// Creates a Codewhale adapter without reading configuration or the clock.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl HostAdapter for CodewhaleAdapter {
    fn host(&self) -> Host {
        Host::Deepseek
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
        if context.repo_root.is_some() {
            project_install::install(context)
        } else {
            install::install(context)
        }
    }

    fn uninstall(&self, context: &InstallContext) -> Result<InstallReport, AdapterError> {
        if context.repo_root.is_some() {
            project_install::uninstall(context)
        } else {
            install::uninstall(context)
        }
    }

    fn prune_target(&self) -> PruneTarget {
        PruneTarget::ReportOnly
    }
}

pub(crate) fn native_event(request: &Request) -> Result<&str, AdapterError> {
    if request.version != PROTOCOL_VERSION {
        return Err(AdapterError::InvalidPayload {
            message: "unsupported protocol version".to_owned(),
        });
    }
    if request.host != Host::Deepseek {
        return Err(AdapterError::InvalidPayload {
            message: "host does not match Deepseek".to_owned(),
        });
    }
    if !request.payload.is_object() && !request.payload.is_null() {
        return Err(AdapterError::InvalidPayload {
            message: "payload must be an object or null".to_owned(),
        });
    }
    let payload_event = if let serde_json::Value::Object(map) = &request.payload {
        if let Some(event_val) = map.get("event") {
            let Some(event_str) = event_val.as_str() else {
                return Err(AdapterError::InvalidPayload {
                    message: "payload event must be a string".to_owned(),
                });
            };
            Some(event_str)
        } else {
            None
        }
    } else {
        None
    };

    match (payload_event, request.host_event.as_deref()) {
        (Some(p_ev), Some(h_ev)) => {
            if p_ev == h_ev {
                Ok(p_ev)
            } else {
                Err(AdapterError::InvalidPayload {
                    message: "payload event conflicts with host_event".to_owned(),
                })
            }
        }
        (Some(p_ev), None) => Ok(p_ev),
        (None, Some(h_ev)) => Ok(h_ev),
        (None, None) => Err(AdapterError::InvalidPayload {
            message: "native event is missing".to_owned(),
        }),
    }
}
