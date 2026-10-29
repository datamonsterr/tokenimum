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
    // Tool-result injections are redirects delivered through Pi steering.
    let reply = shim::reply_json(
        action,
        matches!(hook, "before_agent_start" | "tool_result"),
        hook != "session_shutdown",
    );
    if hook == "tool_result" {
        reply.replacen("\"context\":", "\"steer\":", 1)
    } else {
        reply
    }
}
