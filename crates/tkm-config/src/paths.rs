//! Data, configuration and runtime locations.
use std::{
    io,
    path::{Path, PathBuf},
};

/// Locations used by tokenimum.
#[derive(Debug, Clone)]
pub struct Paths {
    /// Application data directory.
    pub data_dir: PathBuf,
    /// Database file.
    pub db: PathBuf,
    /// Application configuration directory.
    pub config_dir: PathBuf,
    /// Global configuration file.
    pub global_config: PathBuf,
    /// Error log file.
    pub errors_log: PathBuf,
    /// Runtime socket file.
    pub socket: PathBuf,
}
impl Paths {
    /// Resolve paths from explicit XDG directories, falling back to home.
    #[must_use]
    pub fn from_dirs(
        home: &Path,
        data: Option<&Path>,
        config: Option<&Path>,
        runtime: Option<&Path>,
    ) -> Self {
        unimplemented!()
    }
    /// Resolve paths using the platform directories and process environment.
    #[must_use]
    pub fn from_env() -> Option<Self> {
        unimplemented!()
    }
    /// Create the private runtime directory.
    pub fn ensure_runtime_dir(&self) -> io::Result<()> {
        unimplemented!()
    }
}
/// Set a socket's permissions to owner read/write on Unix.
pub fn secure_socket(path: &Path) -> io::Result<()> {
    unimplemented!()
}
/// A size-rotated error log.
pub struct ErrorLog {
    path: PathBuf,
    limit: u64,
}
impl ErrorLog {
    /// Create a logger with the specified rotation limit in bytes.
    #[must_use]
    pub fn new(path: PathBuf, limit: u64) -> Self {
        Self { path, limit }
    }
    /// Append one timestamped line, rotating an existing full file first.
    pub fn append(&self, line: &str) -> io::Result<()> {
        unimplemented!()
    }
}
