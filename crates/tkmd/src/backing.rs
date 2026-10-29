//! Storage access behind a trait, so tests can count every database open.

use std::path::PathBuf;
use std::time::Duration;

use tkm_store::{Store, StoreError};

/// Opens the tokenimum database. Every disk access of the daemon goes through here, which makes
/// "the `tool_done` path does no storage access" testable.
pub trait Backing: Send + Sync {
    /// Opens a connection (running migrations if needed).
    fn open(&self) -> Result<Store, StoreError>;
}

/// SQLite file backing with a short busy timeout, because callers run on the request path or on
/// workers that must not stall behind another process.
#[derive(Debug, Clone)]
pub struct SqliteBacking {
    path: PathBuf,
    busy: Duration,
}

impl SqliteBacking {
    /// Default busy timeout: shorter than the 300 ms `session_start` read timeout.
    pub const DEFAULT_BUSY: Duration = Duration::from_millis(100);

    /// Backing for the database at `path`.
    pub fn new(path: PathBuf) -> Self {
        Self::with_busy_timeout(path, Self::DEFAULT_BUSY)
    }

    /// Backing with an explicit busy timeout.
    pub fn with_busy_timeout(path: PathBuf, busy: Duration) -> Self {
        Self { path, busy }
    }
}

impl Backing for SqliteBacking {
    fn open(&self) -> Result<Store, StoreError> {
        Store::open_with_timeout(&self.path, self.busy)
    }
}
