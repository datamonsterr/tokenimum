//! Data, configuration and runtime locations.
use std::{
    fs::{self, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
/// Default error-log rotation threshold, in bytes.
pub const ERRORS_LOG_MAX_BYTES: u64 = 5 * 1024 * 1024;
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
        let data_dir = data
            .map_or_else(|| home.join(".local/share"), Path::to_path_buf)
            .join("tokenimum");
        let config_dir = config
            .map_or_else(|| home.join(".config"), Path::to_path_buf)
            .join("tokenimum");
        let socket = runtime
            .map_or_else(|| data_dir.join("run"), |p| p.join("tokenimum"))
            .join("tkmd.sock");
        Self {
            db: data_dir.join("tokenimum.db"),
            errors_log: data_dir.join("errors.log"),
            global_config: config_dir.join("config.toml"),
            data_dir,
            config_dir,
            socket,
        }
    }
    /// Resolve paths using the platform directories and process environment.
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let base = directories::BaseDirs::new()?;
        let data_dir = base.data_dir().join("tokenimum");
        let config_dir = base.config_dir().join("tokenimum");
        let socket = base
            .runtime_dir()
            .map_or_else(|| data_dir.join("run"), |p| p.join("tokenimum"))
            .join("tkmd.sock");
        Some(Self {
            db: data_dir.join("tokenimum.db"),
            errors_log: data_dir.join("errors.log"),
            global_config: config_dir.join("config.toml"),
            data_dir,
            config_dir,
            socket,
        })
    }
    /// Create the private runtime directory.
    ///
    /// # Errors
    /// Returns an error when the directory cannot be created or secured.
    pub fn ensure_runtime_dir(&self) -> io::Result<()> {
        let parent = self
            .socket
            .parent()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "socket has no parent"))?;
        fs::create_dir_all(parent)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(parent, fs::Permissions::from_mode(0o700))?;
        }
        Ok(())
    }
}
/// Set a socket's permissions to owner read/write on Unix.
///
/// # Errors
/// Returns an error if the socket permissions cannot be changed.
pub fn secure_socket(path: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    }
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
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
    /// Create an error log using the standard path and rotation limit.
    #[must_use]
    pub fn for_paths(paths: &Paths) -> Self {
        unimplemented!()
    }
    /// Append one timestamped line, rotating an existing full file first.
    ///
    /// # Errors
    /// Returns an error when the log cannot be rotated or written.
    pub fn append(&self, line: &str) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        match fs::metadata(&self.path) {
            Ok(meta) if meta.len() >= self.limit => {
                let mut rotated = self.path.as_os_str().to_os_string();
                rotated.push(".1");
                let rotated = PathBuf::from(rotated);
                if rotated.exists() {
                    fs::remove_file(&rotated)?;
                }
                fs::rename(&self.path, rotated)?;
            }
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        writeln!(
            OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)?,
            "{timestamp} {line}"
        )
    }
}
