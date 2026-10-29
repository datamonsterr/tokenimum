use crate::shim;
use tkm_proto::{ReplyAction, Request};

pub(super) fn render(request: &Request, action: &ReplyAction) -> String {
    // Use the same validation as mapping, so malformed requests always fail open.
    if !matches!(super::map::map_event(request), Ok(Some(_))) {
        return String::new();
    }
    let Ok(native) = super::native(request) else {
        return String::new();
    };
    // Only `chat.message` can push a part; a deleted session has no one to notify.
    shim::reply_json(
        action,
        native.hook == "chat.message",
        native.event_type != Some("session.deleted"),
    )
}
