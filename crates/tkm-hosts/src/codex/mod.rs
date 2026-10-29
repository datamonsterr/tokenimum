//! Codex adapter: hook mapping, replies and rollout reading.
//!
//! The installer waits for the shared writer; until then `install` and
//! `uninstall` report `Unsupported`.

use std::path::{Component, Path, PathBuf};

use serde_json::Value;
use tkm_core::model::{HookEvent, Session};
use tkm_proto::{Host, ReplyAction, Request};

use crate::{AdapterError, HostAdapter, InstallContext, InstallReport, PruneTarget, SessionSource};

mod digest_shim;
mod map;
pub mod reader;
mod reply;

/// Adapter for `OpenAI` Codex hooks and rollout logs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CodexAdapter;

impl CodexAdapter {
    /// Creates the Codex adapter.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl HostAdapter for CodexAdapter {
    fn host(&self) -> Host {
        Host::Codex
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

    fn install(&self, _context: &InstallContext) -> Result<InstallReport, AdapterError> {
        Err(AdapterError::Unsupported {
            operation: "codex install pending writer".into(),
        })
    }

    fn uninstall(&self, context: &InstallContext) -> Result<InstallReport, AdapterError> {
        self.install(context)
    }

    fn prune_target(&self) -> PruneTarget {
        // No built-in tool deny key exists (docs/verified/claude-codex.md).
        PruneTarget::ReportOnly
    }
}

/// Makes `path` relative to `cwd`, or `None` when it points outside it.
///
/// A path under `cwd` is stripped first, even when both are non-absolute
/// (redacted fixtures). Otherwise absolute paths and `..` escapes are refused.
pub(crate) fn repo_relative(path: &str, cwd: Option<&Path>) -> Option<PathBuf> {
    let path = Path::new(path.trim());
    let inner = match cwd.and_then(|root| path.strip_prefix(root).ok()) {
        Some(rest) => rest,
        None if path.is_absolute() => return None,
        None => path,
    };
    let mut clean = PathBuf::new();
    for component in inner.components() {
        match component {
            Component::Normal(part) => clean.push(part),
            Component::CurDir => {}
            _ => return None,
        }
    }
    (!clean.as_os_str().is_empty()).then_some(clean)
}

/// Readable text of a tool result: strings as-is, MCP `content[].text`, the
/// `output` of a Codex shell chunk, else compact JSON.
pub(crate) fn output_text(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::String(text) => text.clone(),
        Value::Object(map) => {
            if let Some(text) = map.get("output").and_then(Value::as_str) {
                return text.to_owned();
            }
            if let Some(items) = map.get("content").and_then(Value::as_array) {
                let texts: Vec<&str> = items
                    .iter()
                    .filter_map(|item| item.get("text").and_then(Value::as_str))
                    .collect();
                if !texts.is_empty() {
                    return texts.join("\n");
                }
            }
            value.to_string()
        }
        other => other.to_string(),
    }
}
