use tkm_proto::{ReplyAction, Request};

pub(super) fn render_reply(request: &Request, action: &ReplyAction) -> String {
    let Ok(native) = super::native_event(request) else {
        return String::new();
    };
    if native != "tool_call_before" {
        return String::new();
    }
    let inject_text = match action {
        ReplyAction::Inject { text } => Some(text.as_str()),
        ReplyAction::InjectAndNotice { inject, .. } => Some(inject.as_str()),
        _ => None,
    };
    let Some(text) = inject_text else {
        return String::new();
    };
    let capped = crate::digest::truncate(text, 2_000);
    let Ok(serialized) = serde_json::to_string(&capped) else {
        return String::new();
    };
    format!("{{\"decision\":\"allow\",\"additionalContext\":{serialized}}}")
}
