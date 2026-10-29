use crate::digest;
use serde_json::Value;
use tkm_proto::{ReplyAction, Request};

pub(super) fn render(request: &Request, action: &ReplyAction) -> String {
    // Use the same validation as mapping, so malformed requests always fail open.
    if !matches!(super::map::map_event(request), Ok(Some(_))) {
        return String::new();
    }
    let Ok(event) = super::map::native_event(request) else {
        return String::new();
    };
    if event == "SessionEnd" {
        return String::new();
    }
    let (inject, notice) = match action {
        ReplyAction::Inject { text } => (Some(text.as_str()), None),
        ReplyAction::Notice { text } => (None, Some(text.as_str())),
        ReplyAction::InjectAndNotice { inject, notice } => {
            (Some(inject.as_str()), Some(notice.as_str()))
        }
        _ => (None, None),
    };
    let mut parts = Vec::new();
    if event != "Stop"
        && let Some(text) = inject
    {
        let text = Value::String(digest::truncate(text, 10_000));
        let event = Value::String(event.to_owned());
        parts.push(format!(
            "\"hookSpecificOutput\":{{\"hookEventName\":{event},\"additionalContext\":{text}}}"
        ));
    }
    if let Some(text) = notice {
        let text = Value::String(digest::truncate(text, 10_000));
        parts.push(format!("\"systemMessage\":{text}"));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("{{{}}}", parts.join(","))
    }
}
