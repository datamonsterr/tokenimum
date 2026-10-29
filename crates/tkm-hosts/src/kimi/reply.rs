//! Reply rendering for Kimi Code.

use tkm_proto::{ReplyAction, Request};

use crate::digest::truncate;

pub(super) fn render_reply(request: &Request, action: &ReplyAction) -> String {
    // Malformed requests always fail open with empty stdout.
    if !matches!(super::map::map_event(request), Ok(Some(_))) {
        return String::new();
    }
    let Ok(native) = super::native_event(request) else {
        return String::new();
    };

    if native != "SessionStart" && native != "UserPromptSubmit" {
        return String::new();
    }

    let text = match action {
        ReplyAction::Inject { text } => text.as_str(),
        ReplyAction::InjectAndNotice { inject, .. } => inject.as_str(),
        _ => return String::new(),
    };

    // Kimi parses JSON stdout for a permission decision; never let injected text look like one.
    if text.trim_start().starts_with('{') {
        return String::new();
    }

    truncate(text, 10_000)
}
