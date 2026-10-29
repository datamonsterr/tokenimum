//! Codex adapter: hook mapping, replies and rollout reading.
//!
//! The installer registers tokenimum hooks in `hooks.json` through the shared writer.

use std::path::{Component, Path, PathBuf};

use tkm_core::model::{HookEvent, Session};
use tkm_proto::{Host, ReplyAction, Request};

use crate::{AdapterError, HostAdapter, InstallContext, InstallReport, PruneTarget, SessionSource};

mod install;
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

    fn install(&self, context: &InstallContext) -> Result<InstallReport, AdapterError> {
        install::install(context)
    }

    fn uninstall(&self, context: &InstallContext) -> Result<InstallReport, AdapterError> {
        install::uninstall(context)
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
