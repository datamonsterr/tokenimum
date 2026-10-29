use crate::shim;
use tkm_proto::{ReplyAction, Request};

pub(super) fn render(request: &Request, action: &ReplyAction) -> String {
    // Use the same validation as mapping, so malformed requests always fail open.
    if !matches!(super::map::map_event(request), Ok(Some(_))) {
        return String::new();
    }
    let Ok(hook) = super::native(request) else {
        return String::new();
    };
    // Only `before_agent_start` can return a message; shutdown has no reader.
    shim::reply_json(
        action,
        hook == "before_agent_start",
        hook != "session_shutdown",
    )
}
