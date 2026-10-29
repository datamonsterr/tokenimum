#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! SQLite storage for tokenimum.

use std::path::Path;
use std::time::Duration;

/// Store operations can fail because of I/O, SQLite, migration, or JSON errors.
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    /// Another connection holds an incompatible lock; callers should fail open.
    #[error("database is locked")]
    Locked,
    /// A migration failed.
    #[error("migration failed: {0}")]
    Migration(#[source] rusqlite::Error),
    /// The database was created by a newer binary.
    #[error("schema version {found} is newer than supported version {supported}")]
    NewerSchema {
        /// Version found in the database.
        found: i64,
        /// Latest version supported by this binary.
        supported: i64,
    },
    /// The requested state transition is not permitted.
    #[error("invalid fact transition")]
    InvalidTransition,
    /// SQLite failed.
    #[error(transparent)]
    Sqlite(rusqlite::Error),
    /// JSON encoding or decoding failed.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// Creating the database directory failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl StoreError {
    /// Whether the failure represents SQLite lock contention.
    #[must_use]
    pub fn is_locked(&self) -> bool {
        unimplemented!()
    }
}

/// A single SQLite connection to tokenimum's store.
pub struct Store;

impl Store {
    /// Open a database, creating missing parent directories.
    ///
    /// # Errors
    /// Returns an error if the database cannot be opened or migrated.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        let _ = path;
        unimplemented!()
    }

    /// Open a database using a caller-selected busy timeout.
    ///
    /// # Errors
    /// Returns an error if the database cannot be opened or migrated.
    pub fn open_with_timeout(
        path: impl AsRef<Path>,
        timeout: Duration,
    ) -> Result<Self, StoreError> {
        let _ = (path, timeout);
        unimplemented!()
    }

    /// Open a private in-memory store.
    ///
    /// # Errors
    /// Returns an error if SQLite cannot initialize the database.
    pub fn open_in_memory() -> Result<Self, StoreError> {
        unimplemented!()
    }
}

/// Open an existing foreign SQLite database without permitting writes or creation.
///
/// # Errors
/// Returns an error if SQLite cannot open the file read-only.
pub fn open_read_only(path: impl AsRef<Path>) -> Result<rusqlite::Connection, StoreError> {
    let _ = path;
    unimplemented!()
}
