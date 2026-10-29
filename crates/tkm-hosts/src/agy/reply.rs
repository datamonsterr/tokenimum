//! Antigravity CLI advisory reply rendering.

use crate::digest;
use tkm_proto::{EventKind, ReplyAction, Request};

pub(super) fn render_reply(request: &Request, action: &ReplyAction) -> String {
    let Ok(Some(event)) = super::map::map_event(request) else {
        return "{}".to_owned();
    };
    let Ok(native) = super::native_event(request) else {
        return "{}".to_owned();
    };
    let inject_text = match action {
        ReplyAction::Inject { text } => Some(text.as_str()),
        ReplyAction::InjectAndNotice { inject, .. } => Some(inject.as_str()),
        _ => None,
    };
    let Some(text) = inject_text else {
        return "{}".to_owned();
    };
    let truncated = digest::truncate(text, 10_000);
    let Ok(escaped) = serde_json::to_string(&truncated) else {
        return "{}".to_owned();
    };
    match (native, event.kind) {
        ("SessionStart", EventKind::SessionStart) => {
            format!(r#"{{"injectSteps":[{{"systemMessage":{{"systemMessage":{escaped}}}}}]}}"#)
        }
        ("PreInvocation", EventKind::Prompt) => {
            format!(r#"{{"injectSteps":[{{"ephemeralMessage":{escaped}}}]}}"#)
        }
        _ => "{}".to_owned(),
    }
}
