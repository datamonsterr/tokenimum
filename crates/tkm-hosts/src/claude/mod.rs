//! Claude Code hook mapping, advisory replies, transcript reading and installation.

mod install;
mod map;
mod reader;
mod reply;
mod time;

use crate::{AdapterError, HostAdapter, InstallContext, InstallReport, PruneTarget, SessionSource};
use tkm_core::model::{HookEvent, Session};
use tkm_proto::{Host, ReplyAction, Request};

/// Stateless Claude Code adapter; all filesystem roots are supplied by callers.
#[derive(Debug, Default, Clone, Copy)]
pub struct ClaudeAdapter;

impl ClaudeAdapter {
    /// Creates a Claude Code adapter without reading configuration or the clock.
    #[must_use]
    pub const fn new() -> Self {
        Self
    }
}

impl HostAdapter for ClaudeAdapter {
    fn host(&self) -> Host {
        Host::Claude
    }
    fn map_event(&self, request: &Request) -> Result<Option<HookEvent>, AdapterError> {
        map::map_event(request)
    }
    fn render_reply(&self, request: &Request, action: &ReplyAction) -> String {
        reply::render(request, action)
    }
    fn read_session(&self, source: &SessionSource) -> Result<Session, AdapterError> {
        reader::read(source)
    }
    fn install(&self, context: &InstallContext) -> Result<InstallReport, AdapterError> {
        install::install(context)
    }
    fn uninstall(&self, context: &InstallContext) -> Result<InstallReport, AdapterError> {
        install::uninstall(context)
    }
    fn prune_target(&self) -> PruneTarget {
        PruneTarget::SettingsDenyList {
            path: ".claude/settings.local.json".into(),
            key: "permissions.deny".to_owned(),
        }
    }
}
