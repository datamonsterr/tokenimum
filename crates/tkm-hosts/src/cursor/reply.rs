//! Cursor reply rendering.

use tkm_proto::{ReplyAction, Request};

use crate::digest;

pub(super) fn render_reply(request: &Request, action: &ReplyAction) -> String {
    if !matches!(super::map::map_event(request), Ok(Some(_))) {
        return "{}".to_owned();
    }
    let Ok(native) = super::native_event(request) else {
        return "{}".to_owned();
    };
    if !matches!(native, "sessionStart" | "postToolUse" | "postToolUseFailure") {
        return "{}".to_owned();
    }
    let inject_text = match action {
        ReplyAction::Inject { text } => Some(text.as_str()),
        ReplyAction::InjectAndNotice { inject, .. } => Some(inject.as_str()),
        _ => None,
    };
    let Some(text) = inject_text else {
        return "{}".to_owned();
    };

    let truncated = digest::truncate(text, 10_000);
    match serde_json::to_string(&truncated) {
        Ok(escaped) => format!("{{\"additional_context\":{escaped}}}"),
        Err(_) => "{}".to_owned(),
    }
}
