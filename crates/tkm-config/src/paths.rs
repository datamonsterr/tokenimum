//! Data, configuration and runtime locations.
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
/// Default error-log rotation threshold, in bytes.
pub const ERRORS_LOG_MAX_BYTES: u64 = 5 * 1024 * 1024;
/// File name of the daemon's exclusive lock, kept in the data directory.
pub const DAEMON_LOCK_NAME: &str = "tkmd.lock";
/// Parent of the per-user runtime directories that `systemd-logind` and `elogind` create.
pub const USER_RUNTIME_ROOT: &str = "/run/user";

/// Picks the runtime directory: an absolute `xdg` (`$XDG_RUNTIME_DIR`), else `<user_root>/<uid>`
/// when it is a private directory owned by `uid`.
///
/// Hook and shell environments often lack `$XDG_RUNTIME_DIR` while the service manager sets it,
/// so the fallback keeps every caller on the socket the daemon actually listens on.
#[must_use]
pub fn resolve_runtime_dir(
    xdg: Option<&Path>,
    user_root: &Path,
    uid: Option<u32>,
) -> Option<PathBuf> {
    if let Some(dir) = xdg.filter(|dir| dir.is_absolute()) {
        return Some(dir.to_path_buf());
    }
    let dir = user_root.join(uid?.to_string());
    owned_private_dir(&dir, uid?).then_some(dir)
}

#[cfg(unix)]
fn owned_private_dir(dir: &Path, uid: u32) -> bool {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    fs::symlink_metadata(dir).is_ok_and(|meta| {
        meta.is_dir() && meta.uid() == uid && meta.permissions().mode().trailing_zeros() >= 6 // no group or other bits
    })
}

#[cfg(not(unix))]
fn owned_private_dir(_dir: &Path, _uid: u32) -> bool {
    false
}

/// The effective user id, read from the owner of `/proc/self` (Linux); `None` elsewhere.
#[must_use]
pub fn current_uid() -> Option<u32> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        fs::metadata("/proc/self").ok().map(|meta| meta.uid())
    }
    #[cfg(not(unix))]
    {
        None
    }
}
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
    /// Exclusive daemon lock: one `tkmd` per data directory, whatever the runtime directory.
    pub daemon_lock: PathBuf,
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
            daemon_lock: data_dir.join(DAEMON_LOCK_NAME),
            errors_log: data_dir.join("errors.log"),
            global_config: config_dir.join("config.toml"),
            data_dir,
            config_dir,
            socket,
        }
    }
    /// Resolve paths using the platform directories and process environment.
    ///
    /// Without `$XDG_RUNTIME_DIR` the socket goes to `/run/user/<uid>` when that is the caller's
    /// private login directory, else under the data directory; see [`resolve_runtime_dir`].
    #[must_use]
    pub fn from_env() -> Option<Self> {
        let base = directories::BaseDirs::new()?;
        let runtime = resolve_runtime_dir(
            base.runtime_dir(),
            Path::new(USER_RUNTIME_ROOT),
            current_uid(),
        );
        Some(Self::from_dirs(
            base.home_dir(),
            Some(base.data_dir()),
            Some(base.config_dir()),
            runtime.as_deref(),
        ))
    }

    /// Takes the exclusive daemon lock; `None` when another process holds it.
    ///
    /// The returned file holds the lock until dropped.
    ///
    /// # Errors
    /// Returns an error when the data directory or lock file cannot be created or locked.
    pub fn lock_daemon(&self) -> io::Result<Option<File>> {
        if let Some(parent) = self.daemon_lock.parent() {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(&self.daemon_lock)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(file)),
            Err(fs::TryLockError::WouldBlock) => Ok(None),
            Err(fs::TryLockError::Error(error)) => Err(error),
        }
    }

    /// Whether a daemon holds the lock for this data directory. Never creates the lock file.
    ///
    /// # Errors
    /// Returns an error when an existing lock file cannot be opened or probed.
    pub fn daemon_running(&self) -> io::Result<bool> {
        lock_held(&self.daemon_lock)
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
/// Whether another process holds the advisory lock on `path`. Never creates the file.
///
/// # Errors
/// Returns an error when an existing lock file cannot be opened or probed.
pub fn lock_held(path: &Path) -> io::Result<bool> {
    let file = match OpenOptions::new().write(true).open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error),
    };
    match file.try_lock() {
        // Dropping the file releases the probe's own lock.
        Ok(()) => Ok(false),
        Err(fs::TryLockError::WouldBlock) => Ok(true),
        Err(fs::TryLockError::Error(error)) => Err(error),
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
        Self::new(paths.errors_log.clone(), ERRORS_LOG_MAX_BYTES)
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
                // Unix rename replaces the previous backup atomically.
                #[cfg(not(unix))]
                if let Err(error) = fs::remove_file(&rotated)
                    && error.kind() != io::ErrorKind::NotFound
                {
                    return Err(error);
                }
                if let Err(error) = fs::rename(&self.path, rotated)
                    && error.kind() != io::ErrorKind::NotFound
                {
                    return Err(error);
                }
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
