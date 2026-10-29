#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! Coding harness adapters.
//!
//! Adapters normalize host inputs and render host-shaped stdout. All methods
//! are synchronous: the daemon runs log readers and installers on blocking
//! workers, while payload mapping and reply rendering stay in memory.

pub mod agy;
pub mod claude;
pub mod codewhale;
pub mod codex;
pub mod cursor;
pub mod digest;
pub mod kimi;
pub mod probe;
mod toml_util;
pub mod writer;

use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use thiserror::Error;
use tkm_core::model::{HookEvent, RepoKey, Session};
use tkm_proto::{Host, ReplyAction, Request};

/// Environment variables `tkm-hook` copies into `Request::env` for `host` (ADR 0020).
#[must_use]
pub fn env_markers(host: Host) -> &'static [&'static str] {
    match host {
        Host::Agy => agy::ENV_MARKERS,
        Host::Cursor => cursor::ENV_MARKERS,
        Host::Kimi => kimi::ENV_MARKERS,
        Host::Deepseek => codewhale::ENV_MARKERS,
        _ => &[],
    }
}

/// Resolved log source and identity supplied to a host session reader.
///
/// `path` can name a JSONL file, database, or session directory; the adapter
/// determines its interpretation. Foreign databases must be opened read-only.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct SessionSource {
    /// Resolved harness data/configuration home.
    pub agent_home: PathBuf,
    /// Host-specific session file, database, or directory.
    pub path: PathBuf,
    /// Host-native session identifier to select within a shared database.
    pub session_id: String,
    /// Repository identity already computed by the caller.
    pub repo_key: RepoKey,
    /// Session working directory.
    pub cwd: PathBuf,
}

/// Scope for host installation or removal; detailed writer policy belongs to E5.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct InstallContext {
    /// Resolved host configuration home; never inferred from process globals.
    pub agent_home: PathBuf,
    /// Project root for a project-scoped install; `None` selects user scope.
    pub repo_root: Option<PathBuf>,
    /// Compute changes and instructions without writing files when true.
    pub dry_run: bool,
}

/// Files changed, or proposed for a dry run, and any manual setup instructions.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct InstallReport {
    /// Paths written/removed, or that would change when `dry_run` is true.
    pub changed_files: Vec<PathBuf>,
    /// Person-facing steps, including host trust approval that cannot be automated.
    pub instructions: Vec<String>,
    /// Whether this report describes proposed changes without filesystem writes.
    pub dry_run: bool,
}

/// A verified pruning surface, or the safe report-only fallback (SPEC 7.3).
///
/// Paths are relative to the project root; dotted keys describe host syntax,
/// interpreted by that host's writer. This metadata grants no write permission:
/// committed files, escaping symlinks, and foreign entries remain protected.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
#[non_exhaustive]
pub enum PruneTarget {
    /// Append only tokenimum-owned entries to a settings deny list.
    SettingsDenyList {
        /// Project-relative settings file path.
        path: PathBuf,
        /// Dotted settings key, such as `permissions.deny`.
        key: String,
    },
    /// Update a host-specific config key using its verified pruning convention.
    ConfigFileKey {
        /// Project-relative config file path.
        path: PathBuf,
        /// Dotted key or key prefix, such as `permission` or `defaultTools`.
        key: String,
    },
    /// Report evidence only; no verified writable deny surface is available.
    #[default]
    ReportOnly,
}

/// Recoverable host-adapter failure; callers log it and use a no-op fallback.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum AdapterError {
    /// Input JSON is valid but lacks metadata required by this host.
    #[error("invalid host payload: {message}")]
    InvalidPayload {
        /// Explanation of the invalid or absent metadata, without secret values.
        message: String,
    },
    /// A settings target escapes its permitted root or is tracked by git.
    #[error("unsafe adapter target {path}: {reason}", path = .path.display())]
    UnsafeTarget {
        /// Refused settings or auxiliary path.
        path: PathBuf,
        /// Safe explanation of the refusal.
        reason: String,
    },
    /// A host surface is unavailable or not yet verified.
    #[error("unsupported adapter operation: {operation}")]
    Unsupported {
        /// Operation name or safe diagnostic for the unavailable surface.
        operation: String,
    },
    /// A log or settings file could not be read or written.
    #[error("adapter I/O failed: {0}")]
    Io(
        /// Original I/O error, retaining its kind and source.
        #[from]
        io::Error,
    ),
    /// A host log, settings file, or payload contains malformed JSON.
    #[error("malformed host JSON: {0}")]
    Malformed(
        /// Original JSON error for diagnostic chaining.
        #[from]
        serde_json::Error,
    ),
}

/// Object-safe host boundary shared by the daemon, replay, and installers.
///
/// Implementations must be usable as `Box<dyn HostAdapter + Send + Sync>`.
/// Mapping and rendering perform no I/O. Readers and installers may block;
/// the daemon calls them through its blocking worker facility, such as
/// `tokio::task::spawn_blocking`, without adding a runtime dependency here.
pub trait HostAdapter: Send + Sync {
    /// Returns the harness handled by this adapter.
    #[must_use]
    fn host(&self) -> Host;

    /// Normalizes one request, returning `None` for an intentionally ignored event.
    ///
    /// # Errors
    /// Returns [`AdapterError`] for unsupported protocol versions, host mismatch,
    /// or invalid required payload metadata. Implementations perform no I/O.
    fn map_event(&self, request: &Request) -> Result<Option<HookEvent>, AdapterError>;

    /// Renders exact stdout for this event and semantic action, without I/O.
    ///
    /// Implementations enforce host caps and event rules, including no Codex
    /// injection on `turn_end`/`session_end`. Unsupported actions fall back to
    /// the host's no-op reply; notices are advisory and never block a turn.
    /// The request retains the native event name needed for host-shaped replies.
    /// The hook prints this result verbatim and never interprets the action.
    #[must_use]
    fn render_reply(&self, request: &Request, action: &ReplyAction) -> String;

    /// Reads a host log into a normalized session, counting skipped records.
    ///
    /// Foreign databases are read-only. Sessions with more than 20% skipped
    /// records are marked partial and may contribute to the ledger only.
    ///
    /// # Errors
    /// Returns [`AdapterError`] when reading or decoding the source fails.
    fn read_session(&self, source: &SessionSource) -> Result<Session, AdapterError>;

    /// Installs tokenimum-owned host entries or reports proposed changes.
    ///
    /// Writers preserve foreign entries, respect `dry_run`, back up before
    /// the first write, and reject unsafe or committed targets per SPEC 13.4.
    ///
    /// # Errors
    /// Returns [`AdapterError`] for unavailable surfaces, invalid targets,
    /// or settings read/write failures.
    fn install(&self, context: &InstallContext) -> Result<InstallReport, AdapterError>;

    /// Removes only tokenimum-owned host entries or reports proposed changes.
    ///
    /// # Errors
    /// Returns [`AdapterError`] for invalid targets or settings read/write
    /// failures. The same preservation and dry-run rules as installation apply.
    fn uninstall(&self, context: &InstallContext) -> Result<InstallReport, AdapterError>;

    /// Returns verified pruning metadata, or report-only for unverified surfaces.
    #[must_use]
    fn prune_target(&self) -> PruneTarget;
}

/// Reference/test implementation that performs no host mapping or filesystem I/O.
///
/// Every action renders the selected host's exact no-op stdout. Session reads
/// preserve source identity but contain no turns, catalog entries, or usage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoopAdapter {
    host: Host,
}

impl NoopAdapter {
    /// Creates the reference adapter for a particular host.
    #[must_use]
    pub const fn new(host: Host) -> Self {
        Self { host }
    }
}

impl HostAdapter for NoopAdapter {
    fn host(&self) -> Host {
        self.host
    }

    fn map_event(&self, _request: &Request) -> Result<Option<HookEvent>, AdapterError> {
        Ok(None)
    }

    fn render_reply(&self, _request: &Request, _action: &ReplyAction) -> String {
        self.host.noop_reply().into()
    }

    fn read_session(&self, source: &SessionSource) -> Result<Session, AdapterError> {
        Ok(Session {
            agent_home: source.agent_home.clone(),
            session_id: source.session_id.clone(),
            repo_key: source.repo_key.clone(),
            cwd: source.cwd.clone(),
            ..Session::new(self.host)
        })
    }

    fn install(&self, context: &InstallContext) -> Result<InstallReport, AdapterError> {
        Ok(InstallReport {
            dry_run: context.dry_run,
            ..InstallReport::default()
        })
    }

    fn uninstall(&self, context: &InstallContext) -> Result<InstallReport, AdapterError> {
        self.install(context)
    }

    fn prune_target(&self) -> PruneTarget {
        PruneTarget::ReportOnly
    }
}
