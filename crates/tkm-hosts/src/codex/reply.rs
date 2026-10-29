//! Host-shaped stdout for Codex hooks (ADR 0011, "Replies"); no I/O.

use tkm_proto::{ReplyAction, Request};

const MAX_REPLY_CHARS: usize = 10_000;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Native {
    SessionStart,
    Prompt,
    ToolDone,
    Stop,
    SessionEnd,
    Other,
}

impl Native {
    fn from_name(name: &str) -> Self {
        match name {
            "SessionStart" => Self::SessionStart,
            "UserPromptSubmit" => Self::Prompt,
            "PostToolUse" => Self::ToolDone,
            "Stop" => Self::Stop,
            "SessionEnd" => Self::SessionEnd,
            _ => Self::Other,
        }
    }

    /// Native name used in `hookEventName`; only injectable events have one.
    const fn injectable_name(self) -> Option<&'static str> {
        match self {
            Self::SessionStart => Some("SessionStart"),
            Self::Prompt => Some("UserPromptSubmit"),
            Self::ToolDone => Some("PostToolUse"),
            Self::Stop | Self::SessionEnd | Self::Other => None,
        }
    }

    const fn allows_notice(self) -> bool {
        !matches!(self, Self::SessionEnd | Self::Other)
    }
}

fn json_string(text: &str) -> String {
    serde_json::to_string(text).unwrap_or_else(|_| String::from("\"\""))
}

/// Non-empty text capped at 10,000 characters, cut on a `char` boundary.
fn capped(text: &str) -> Option<String> {
    let text: String = text.chars().take(MAX_REPLY_CHARS).collect();
    (!text.is_empty()).then_some(text)
}

pub(crate) fn render_reply(request: &Request, action: &ReplyAction) -> String {
    let (inject, notice) = match action {
        ReplyAction::Inject { text } => (capped(text), None),
        ReplyAction::Notice { text } => (None, capped(text)),
        ReplyAction::InjectAndNotice { inject, notice } => (capped(inject), capped(notice)),
        _ => (None, None),
    };
    let Ok(Some(native)) = super::native_event(request) else {
        return String::new();
    };
    let native = Native::from_name(native);
    let inject = inject.zip(native.injectable_name());
    let notice = notice.filter(|_| native.allows_notice());

    let mut parts = Vec::new();
    if let Some((text, name)) = inject {
        parts.push(format!(
            "\"hookSpecificOutput\":{{\"hookEventName\":{},\"additionalContext\":{}}}",
            json_string(name),
            json_string(&text)
        ));
    }
    if let Some(text) = notice {
        parts.push(format!("\"systemMessage\":{}", json_string(&text)));
    }
    if parts.is_empty() {
        String::new()
    } else {
        format!("{{{}}}", parts.join(","))
    }
}
