//! Command failures and their process exit codes.

use std::path::PathBuf;
use std::process::ExitCode;

/// A failure reported by the CLI without panicking.
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    /// This command has no implementation in the current build.
    #[error("tkm {0}: not available in this build")]
    NotAvailable(&'static str),
    /// Command arguments or values were invalid.
    #[error("tkm: {0}")]
    Usage(String),
    /// The process environment cannot identify application directories.
    #[error("tkm: cannot resolve the data directory (no home directory)")]
    PathsUnavailable,
    /// The database does not exist yet.
    #[error("tkm: database {} does not exist; run tkm init", .0.display())]
    MissingDatabase(PathBuf),
    /// Opening an existing or writable database failed.
    #[error("tkm: cannot open database {}: {source}", .path.display())]
    Database {
        /// Database file involved in the operation.
        path: PathBuf,
        /// Underlying store failure.
        #[source]
        source: tkm_store::StoreError,
    },
    /// Configuration could not be loaded or validated.
    #[error("tkm: {0}")]
    Config(#[from] tkm_config::config::ConfigError),
    /// A filesystem or output operation failed.
    #[error("tkm: {0}")]
    Io(#[from] std::io::Error),
}

impl CliError {
    /// Return exit 2 for usage/unavailable commands, or exit 1 for runtime failures.
    #[must_use]
    pub fn exit_code(&self) -> ExitCode {
        match self {
            Self::NotAvailable(_) | Self::Usage(_) => ExitCode::from(2),
            Self::PathsUnavailable
            | Self::MissingDatabase(_)
            | Self::Database { .. }
            | Self::Config(_)
            | Self::Io(_) => ExitCode::FAILURE,
        }
    }
}
