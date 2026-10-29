#![forbid(unsafe_code)]
#![deny(missing_docs)]
//! SQLite storage for tokenimum.
//!
//! Each thread should use its own [`Store`]. WAL permits readers to run alongside
//! one writer. Callers should fail open when [`StoreError::is_locked`] is true.
//!
//! ```
//! # fn main() -> Result<(), tkm_store::StoreError> {
//! use tkm_core::model::{RepoKey, Timestamp};
//! use tkm_store::Repo;
//! let store = tkm_store::Store::open_in_memory()?;
//! let repo = Repo {
//!     repo_key: RepoKey("example/project".into()),
//!     display_name: "project".into(),
//!     first_seen: Timestamp(0),
//!     last_seen: Timestamp(0),
//!     delegated_runs: 0,
//! };
//! store.upsert_repo(&repo)?;
//! assert_eq!(store.get_repo(&repo.repo_key)?, Some(repo));
//! # Ok(())
//! # }
//! ```

mod decisions;
mod mechanisms;
mod migrations;
mod queries;
mod sessions;

pub use decisions::{CalibrationRecord, DecisionRecord, InstallRecord, NewDecision};
pub use mechanisms::{DenylistEntry, ENVELOPE_RING, FactRecord, FactState, ToolEvidence};
pub use queries::Repo;
pub use sessions::{
    EventRecord, LedgerRecord, NewEvent, NewSession, SessionKey, SessionRecord, TurnRecord,
};

use std::path::Path;
use std::time::Duration;

use rusqlite::{Connection, ErrorCode, OpenFlags, TransactionBehavior};

const SCHEMA_VERSION: i64 = 1;
const DEFAULT_BUSY_TIMEOUT: Duration = Duration::from_secs(2);

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
    /// A token count exceeds the range of a SQLite integer.
    #[error("token count exceeds SQLite integer range")]
    TokenOverflow,
    /// SQLite failed.
    #[error(transparent)]
    Sqlite(rusqlite::Error),
    /// JSON encoding or decoding failed.
    #[error(transparent)]
    Json(#[from] serde_json::Error),
    /// Creating the database directory or validating options failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

impl StoreError {
    /// Whether the failure represents SQLite lock contention.
    #[must_use]
    pub fn is_locked(&self) -> bool {
        matches!(self, Self::Locked)
    }
}

impl From<rusqlite::Error> for StoreError {
    fn from(error: rusqlite::Error) -> Self {
        if matches!(
            error.sqlite_error_code(),
            Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
        ) {
            Self::Locked
        } else {
            Self::Sqlite(error)
        }
    }
}

fn migration_error(error: rusqlite::Error) -> StoreError {
    if matches!(
        error.sqlite_error_code(),
        Some(ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked)
    ) {
        StoreError::Locked
    } else {
        StoreError::Migration(error)
    }
}

/// A single SQLite connection to tokenimum's store.
pub struct Store {
    conn: Connection,
}

impl Store {
    /// Open a database, creating missing parent directories.
    ///
    /// # Errors
    /// Returns an error if the database cannot be opened or migrated.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, StoreError> {
        Self::open_with_timeout(path, DEFAULT_BUSY_TIMEOUT)
    }

    /// Open a database using a caller-selected busy timeout.
    ///
    /// # Errors
    /// Returns an error if the database cannot be opened or migrated, or if
    /// the timeout exceeds SQLite's signed 32-bit millisecond limit.
    pub fn open_with_timeout(
        path: impl AsRef<Path>,
        timeout: Duration,
    ) -> Result<Self, StoreError> {
        if let Some(parent) = path
            .as_ref()
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)?;
        }
        Self::initialize(Connection::open(path)?, timeout)
    }

    /// Open a private in-memory store.
    ///
    /// # Errors
    /// Returns an error if SQLite cannot initialize the database.
    pub fn open_in_memory() -> Result<Self, StoreError> {
        Self::initialize(Connection::open_in_memory()?, DEFAULT_BUSY_TIMEOUT)
    }

    fn initialize(mut conn: Connection, timeout: Duration) -> Result<Self, StoreError> {
        if timeout.as_millis() > 2_147_483_647_u128 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "busy timeout is too large",
            )
            .into());
        }
        conn.busy_timeout(timeout)?;
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", true)?;
        // The common path does not contend with writers. Recheck under an
        // immediate transaction only if a migration appears necessary.
        loop {
            let observed: i64 = conn
                .pragma_query_value(None, "user_version", |row| row.get(0))
                .map_err(migration_error)?;
            if observed > SCHEMA_VERSION {
                return Err(StoreError::NewerSchema {
                    found: observed,
                    supported: SCHEMA_VERSION,
                });
            }
            if observed == SCHEMA_VERSION {
                break;
            }
            let tx = conn
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(migration_error)?;
            let found: i64 = tx
                .pragma_query_value(None, "user_version", |row| row.get(0))
                .map_err(migration_error)?;
            if found > SCHEMA_VERSION {
                return Err(StoreError::NewerSchema {
                    found,
                    supported: SCHEMA_VERSION,
                });
            }
            if found == SCHEMA_VERSION {
                tx.commit().map_err(migration_error)?;
                break;
            }
            let index = usize::try_from(found).map_err(|_| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "negative schema version")
            })?;
            let sql = migrations::MIGRATIONS.get(index).ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::InvalidData, "missing migration")
            })?;
            tx.execute_batch(sql).map_err(migration_error)?;
            tx.pragma_update(None, "user_version", found + 1)
                .map_err(migration_error)?;
            tx.commit().map_err(migration_error)?;
        }
        Ok(Self { conn })
    }
}

/// Open an existing foreign SQLite database without permitting writes or creation.
///
/// # Errors
/// Returns an error if SQLite cannot open the file read-only.
pub fn open_read_only(path: impl AsRef<Path>) -> Result<Connection, StoreError> {
    Ok(Connection::open_with_flags(
        path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?)
}
