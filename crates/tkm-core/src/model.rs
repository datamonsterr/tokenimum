//! Pure normalized session records and live adapter events.
//!
//! These types perform no I/O. Readers supply paths and timestamps; repository
//! identity is computed by E1. Missing metadata remains explicit rather than
//! requiring a host log read on the daemon's live event path.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tkm_proto::EventKind;

/// Shared harness identity, owned by the wire contract to keep the hook small.
pub use tkm_proto::Host;

/// Repository identity shared by worktrees; normalization is implemented in E1.
///
/// The empty default represents identity not yet resolved by the caller.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RepoKey(
    /// Opaque normalized repository identity; callers must not infer its format.
    pub String,
);

/// Signed Unix milliseconds, with no timezone or clock I/O dependency.
///
/// The default is the Unix epoch. Negative values represent pre-epoch times.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default, Serialize, Deserialize,
)]
#[serde(transparent)]
pub struct Timestamp(
    /// Milliseconds since 1970-01-01 00:00:00 UTC.
    pub i64,
);

/// Token counts reported by a harness, split into independent billing classes.
///
/// Readers normalize input to uncached input tokens, excluding cache reads and
/// writes, so adding all classes does not count cached tokens twice.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Usage {
    /// Uncached input tokens.
    pub input: u64,
    /// Generated output tokens.
    pub output: u64,
    /// Tokens read from a prompt cache.
    pub cache_read: u64,
    /// Tokens written to a prompt cache.
    pub cache_write: u64,
}

impl Usage {
    /// Adds each token class independently, saturating at [`u64::MAX`].
    ///
    /// # Examples
    /// ```
    /// use tkm_core::model::Usage;
    /// let usage = Usage { input: u64::MAX, ..Usage::default() };
    /// assert_eq!(usage.add(usage).input, u64::MAX);
    /// ```
    #[must_use]
    pub const fn add(self, other: Self) -> Self {
        Self {
            input: self.input.saturating_add(other.input),
            output: self.output.saturating_add(other.output),
            cache_read: self.cache_read.saturating_add(other.cache_read),
            cache_write: self.cache_write.saturating_add(other.cache_write),
        }
    }
}

impl std::ops::Add for Usage {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self::add(self, rhs)
    }
}

/// Why the harness stopped or yielded a turn.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum StopReason {
    /// The agent ended the turn on its own.
    OwnStop,
    /// The turn escalated to a person or another agent.
    Escalated,
    /// A redirect or new instruction displaced the current turn.
    Redirected,
    /// The person or harness interrupted the turn.
    Interrupted,
    /// The turn ended because of an error.
    Error,
    /// The log or live event does not identify a stop reason.
    #[default]
    Unknown,
}

/// Kind of repository file change observed in a tool call.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum EditKind {
    /// A file was created.
    Create,
    /// An existing file was modified.
    Modify,
    /// A file was deleted.
    Delete,
}

/// How an edit was performed, including shell edits relevant to the loop guard.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum EditVia {
    /// A dedicated edit or write tool changed the file.
    Tool,
    /// A shell command or script changed the file.
    Shell,
}

/// A normalized repository edit; readers must reject paths outside the repo.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileEdit {
    /// Path relative to the repository root, without escaping parent components.
    pub path: PathBuf,
    /// Whether the file was created, modified, or deleted.
    pub kind: EditKind,
    /// Dedicated tool versus shell edit, for loop-guard evidence.
    pub via: EditVia,
}

/// One normalized tool call; unknown status and duration remain absent.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Call {
    /// Host tool name, retaining MCP namespaces when present.
    pub tool: String,
    /// Compact argument description; external decision clients must redact it.
    pub args_digest: String,
    /// Compact output description; external decision clients must redact it.
    pub output_digest: String,
    /// Process exit status where meaningful and reported by the host.
    pub exit_code: Option<i32>,
    /// Whether the host reported a tool failure, independently of exit status.
    pub is_error: bool,
    /// Elapsed milliseconds, or `None` when the host does not report timing.
    pub duration_ms: Option<u64>,
    /// Repository file edits attributed to this call.
    pub edits: Vec<FileEdit>,
}

/// Origin of a catalog entry used by tool and skill pruning evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[non_exhaustive]
pub enum ToolKind {
    /// A tool built into the harness.
    Builtin,
    /// A tool supplied by an MCP server.
    Mcp,
    /// A skill listed or loaded into the session.
    Skill,
    /// An origin not identified by the reader.
    #[default]
    Other,
}

/// Tool or skill exposed to the session, whether or not it was invoked.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct ToolDef {
    /// Host-visible tool or skill name.
    pub name: String,
    /// Tool origin, keeping skill evidence separate from tool pruning.
    pub kind: ToolKind,
    /// MCP server name for MCP tools when known; otherwise `None`.
    pub server: Option<String>,
}

/// One prompt and its tool calls, ending with the observed stop reason.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Turn {
    /// User prompt text, or empty when the reader cannot recover it.
    pub prompt: String,
    /// Calls in execution order.
    pub calls: Vec<Call>,
    /// Observed termination reason; unknown for an incomplete turn.
    pub stopped_by: StopReason,
    /// Model ID when the turn uses one model and its identity is known.
    #[serde(default)]
    pub model: Option<String>,
    /// Token counts by model ID for the turn.
    #[serde(default)]
    pub usage_by_model: BTreeMap<String, Usage>,
    /// Aggregate token counts for the turn.
    pub usage: Usage,
}

/// A normalized session produced by a host log reader (SPEC 6.4).
///
/// Readers count all records and mark `partial` when more than 20% are skipped.
/// Partial sessions contribute to the ledger only, rather than training facts
/// or pruning evidence. Validation and threshold computation belong to readers.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    /// Harness that owns this session.
    pub host: Host,
    /// Resolved harness data/configuration home supplied by the reader.
    pub agent_home: PathBuf,
    /// Host-native session identifier.
    pub session_id: String,
    /// Repository identity, shared across its worktrees.
    pub repo_key: RepoKey,
    /// Session working directory as recorded by the host.
    pub cwd: PathBuf,
    /// Session start time; constructors use the epoch until metadata is filled.
    pub started: Timestamp,
    /// Session end time, absent for a live session or unknown end.
    pub ended: Option<Timestamp>,
    /// Turns in chronological order.
    #[serde(default)]
    pub turns: Vec<Turn>,
    /// Catalog entries exposed to the session, including unused tools and skills.
    #[serde(default)]
    pub tool_catalog: Vec<ToolDef>,
    /// Aggregate session token counts.
    #[serde(default)]
    pub usage: Usage,
    /// Token counts by model ID across the session.
    #[serde(default)]
    pub usage_by_model: BTreeMap<String, Usage>,
    /// Reader found the session too incomplete for fact/pruning evidence.
    #[serde(default)]
    pub partial: bool,
    /// Unrecognized or malformed records skipped by the reader.
    #[serde(default)]
    pub skipped_records: u64,
    /// All records encountered, including skipped records.
    #[serde(default)]
    pub total_records: u64,
}

impl Session {
    /// Creates an empty session for `host`, with unresolved identity and paths.
    ///
    /// Callers populate metadata from the source before using it as evidence.
    /// The constructor performs no I/O and does not infer a start time.
    #[must_use]
    pub fn new(host: Host) -> Self {
        Self {
            host,
            agent_home: PathBuf::new(),
            session_id: String::new(),
            repo_key: RepoKey::default(),
            cwd: PathBuf::new(),
            started: Timestamp::default(),
            ended: None,
            turns: Vec::new(),
            tool_catalog: Vec::new(),
            usage: Usage::default(),
            usage_by_model: BTreeMap::new(),
            partial: false,
            skipped_records: 0,
            total_records: 0,
        }
    }
}

/// Host-independent live input for pure mechanisms and daemon routing.
///
/// Optional fields preserve missing host metadata. `usage` is the increment
/// attributable to this event, so the daemon can add it exactly once; adapters
/// converting cumulative counters must produce the corresponding delta.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HookEvent {
    /// Harness that emitted the event.
    pub host: Host,
    /// Normalized event kind, shared with the request and timeout contract.
    pub kind: EventKind,
    /// Host-native session identifier resolved from payload or environment.
    pub session_id: String,
    /// Host-native turn identifier, if the harness supplies one.
    pub turn_id: Option<String>,
    /// Resolved repository identity, if already known without I/O.
    pub repo_key: Option<RepoKey>,
    /// Working directory, if supplied by the host.
    pub cwd: Option<PathBuf>,
    /// Host event timestamp, if reported; the adapter does not read a clock.
    pub timestamp: Option<Timestamp>,
    /// Prompt text on prompt-bearing events, including redirects.
    pub prompt: Option<String>,
    /// Call information for a before/completed tool event, when available.
    pub call: Option<Call>,
    /// Incremental usage associated with this event, zero if unreported.
    #[serde(default)]
    pub usage: Usage,
    /// Stop reason on end/interruption events, if known.
    pub stopped_by: Option<StopReason>,
    /// Catalog entries reported by this event, empty when absent.
    #[serde(default)]
    pub tool_catalog: Vec<ToolDef>,
}

impl HookEvent {
    /// Creates a live event with only host, kind, and session identity populated.
    #[must_use]
    pub fn new(host: Host, kind: EventKind, session_id: impl Into<String>) -> Self {
        Self {
            host,
            kind,
            session_id: session_id.into(),
            turn_id: None,
            repo_key: None,
            cwd: None,
            timestamp: None,
            prompt: None,
            call: None,
            usage: Usage::default(),
            stopped_by: None,
            tool_catalog: Vec::new(),
        }
    }
}
