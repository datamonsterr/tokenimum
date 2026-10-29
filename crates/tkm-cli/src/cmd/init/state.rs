//! Private manifest format shared by init, uninstall, and doctor.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tkm_hosts::{AdapterError, InstallContext};
use tkm_proto::Host;
use tkm_store::InstallRecord;

use crate::{ctx::Ctx, error::CliError};

pub(crate) const HOSTS: [Host; 8] = [
    Host::Claude,
    Host::Codex,
    Host::Agy,
    Host::Cursor,
    Host::Kimi,
    Host::Deepseek,
    Host::Opencode,
    Host::Pi,
];
const MANIFEST_PREFIX: &str = "tkm-cli-v1:";

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub(crate) struct Manifest {
    pub mode: String,
    pub directories: Vec<PathBuf>,
    pub instructions_repos: Vec<PathBuf>,
    pub adapter: bool,
    pub skill: bool,
    pub service: Option<ServiceRecord>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct ServiceRecord {
    pub root: PathBuf,
    pub path: PathBuf,
    pub content: String,
    pub owned: bool,
    #[serde(default)]
    pub detached: bool,
    /// Startup was attempted but neither a service manager nor a detached child started.
    #[serde(default)]
    pub start_failed: bool,
    pub directories: Vec<PathBuf>,
}

pub(crate) fn failure(error: impl std::fmt::Display) -> CliError {
    io::Error::other(error.to_string().replace(['\r', '\n'], " ")).into()
}

pub(crate) fn adapter_error(error: AdapterError) -> CliError {
    // TOML parse diagnostics may quote a person's configuration. Only expose safe categories.
    match error {
        AdapterError::Io(error) => error.into(),
        AdapterError::UnsafeTarget { path, reason } => {
            failure(format!("{}: {reason}", path.display()))
        }
        AdapterError::Unsupported { operation } => failure(operation),
        _ => failure("invalid harness configuration or payload"),
    }
}

pub(crate) fn env<'a>(ctx: &'a Ctx<'_>, name: &str) -> Option<&'a str> {
    ctx.env
        .iter()
        .rev()
        .find(|(key, _)| key == name)
        .map(|(_, value)| value.as_str())
}

pub(crate) fn home(ctx: &Ctx<'_>) -> Result<PathBuf, CliError> {
    env(ctx, "HOME")
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
        .ok_or(CliError::PathsUnavailable)
}

pub(crate) fn host_home(home: &Path, host: Host) -> PathBuf {
    home.join(match host {
        Host::Claude => ".claude",
        Host::Codex => ".codex",
        Host::Agy => ".gemini",
        Host::Cursor => ".cursor",
        Host::Kimi => ".kimi",
        Host::Deepseek => ".codewhale",
        Host::Opencode => ".config/opencode",
        Host::Pi => ".pi/agent",
        _ => ".tokenimum-unknown",
    })
}

pub(crate) fn binaries(host: Host) -> &'static [&'static str] {
    match host {
        Host::Claude => &["claude"],
        Host::Codex => &["codex"],
        Host::Agy => &["agy"],
        Host::Cursor => &["agent", "cursor-agent"],
        Host::Kimi => &["kimi"],
        Host::Deepseek => &["codewhale", "deepseek"],
        Host::Opencode => &["opencode"],
        Host::Pi => &["pi"],
        _ => &[],
    }
}

pub(crate) fn executable(ctx: &Ctx<'_>, name: &str) -> Option<PathBuf> {
    std::env::split_paths(env(ctx, "PATH").unwrap_or_default()).find_map(|directory| {
        let directory = if directory.is_absolute() {
            directory
        } else {
            ctx.cwd.join(directory)
        };
        let path = directory.join(name);
        let metadata = fs::metadata(&path).ok()?;
        if !metadata.is_file() {
            return None;
        }
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if metadata.permissions().mode() & 0o111 == 0 {
                return None;
            }
        }
        Some(path)
    })
}

pub(crate) fn context(home: &Path, dry_run: bool) -> InstallContext {
    InstallContext {
        agent_home: home.to_path_buf(),
        repo_root: None,
        dry_run,
    }
}

pub(crate) fn metadata(record: &InstallRecord) -> Result<Manifest, CliError> {
    match record
        .entry_ids
        .iter()
        .find_map(|id| id.strip_prefix(MANIFEST_PREFIX))
    {
        Some(json) => serde_json::from_str(json).map_err(failure),
        None if record.entry_ids.is_empty() => Ok(Manifest::default()),
        None => Ok(Manifest {
            mode: "hooks".into(),
            adapter: true,
            ..Manifest::default()
        }),
    }
}

pub(crate) fn set_metadata(
    record: &mut InstallRecord,
    manifest: &Manifest,
) -> Result<(), CliError> {
    record
        .entry_ids
        .retain(|entry| !entry.starts_with(MANIFEST_PREFIX));
    record.entry_ids.push(format!(
        "{MANIFEST_PREFIX}{}",
        serde_json::to_string(manifest).map_err(failure)?
    ));
    Ok(())
}

pub(crate) fn records(connection: &rusqlite::Connection) -> Result<Vec<InstallRecord>, CliError> {
    let mut statement = connection.prepare("SELECT host, agent_home, files_written, entry_ids, backup_paths, shim_hashes FROM installs ORDER BY host, agent_home").map_err(failure)?;
    let rows = statement
        .query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
            ))
        })
        .map_err(failure)?;
    rows.map(|row| {
        let (host, home, files, entries, backups, hashes) = row.map_err(failure)?;
        Ok(InstallRecord {
            host: host.parse().map_err(failure)?,
            agent_home: home.into(),
            files_written: serde_json::from_str(&files).map_err(failure)?,
            entry_ids: serde_json::from_str(&entries).map_err(failure)?,
            backup_paths: serde_json::from_str(&backups).map_err(failure)?,
            shim_hashes: serde_json::from_str(&hashes).map_err(failure)?,
        })
    })
    .collect()
}

pub(crate) fn empty_record(host: Host, home: &Path) -> InstallRecord {
    InstallRecord {
        host,
        agent_home: home.to_path_buf(),
        files_written: Vec::new(),
        entry_ids: Vec::new(),
        backup_paths: Vec::new(),
        shim_hashes: BTreeMap::new(),
    }
}

pub(crate) fn extend_paths(target: &mut Vec<PathBuf>, paths: impl IntoIterator<Item = PathBuf>) {
    for path in paths {
        if !target.contains(&path) {
            target.push(path);
        }
    }
}

pub(crate) fn missing_directories(files: &[PathBuf]) -> Vec<PathBuf> {
    let mut directories = Vec::new();
    for file in files {
        let mut parent = file.parent();
        while let Some(directory) = parent.filter(|directory| !directory.exists()) {
            extend_paths(&mut directories, [directory.to_path_buf()]);
            parent = directory.parent();
        }
    }
    directories
}

pub(crate) fn remove_empty_directories(directories: &[PathBuf]) -> Result<(), CliError> {
    let mut directories = directories.to_vec();
    directories.sort_by_key(|directory| std::cmp::Reverse(directory.components().count()));
    for directory in directories {
        // Recorded directories are canonicalized after creation. A later symlink anywhere
        // on that path must not redirect cleanup into a directory we never created.
        if directory.ancestors().any(|ancestor| {
            fs::symlink_metadata(ancestor).is_ok_and(|meta| meta.file_type().is_symlink())
        }) {
            continue;
        }
        if let Err(error) = fs::remove_dir(directory)
            && !matches!(
                error.kind(),
                io::ErrorKind::NotFound | io::ErrorKind::DirectoryNotEmpty
            )
        {
            return Err(error.into());
        }
    }
    Ok(())
}

pub(crate) fn canonicalize_directories(directories: &mut [PathBuf]) {
    for directory in directories {
        if let Ok(canonical) = fs::canonicalize(&*directory) {
            *directory = canonical;
        }
    }
}

pub(crate) struct Snapshot {
    pub connection: rusqlite::Connection,
    _directory: TemporarySnapshot,
}

struct TemporarySnapshot(PathBuf);

impl Drop for TemporarySnapshot {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(crate) fn snapshot_database(path: &Path) -> Result<Snapshot, CliError> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let directory = loop {
        let path = std::env::temp_dir().join(format!(
            "tkm-uninstall-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let mut builder = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            builder.mode(0o700);
        }
        match builder.create(&path) {
            Ok(()) => break TemporarySnapshot(path),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {}
            Err(error) => return Err(error.into()),
        }
    };
    let copy = directory.0.join("snapshot.db");
    fs::copy(path, &copy)?;
    let mut wal = path.as_os_str().to_os_string();
    wal.push("-wal");
    if let Err(error) = fs::copy(PathBuf::from(wal), directory.0.join("snapshot.db-wal"))
        && error.kind() != io::ErrorKind::NotFound
    {
        return Err(error.into());
    }
    let connection = tkm_store::open_read_only(&copy).map_err(failure)?;
    Ok(Snapshot {
        connection,
        _directory: directory,
    })
}
