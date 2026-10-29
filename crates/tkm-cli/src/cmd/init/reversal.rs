//! Host-independent recovery of global init effects (ADR 0033).

use super::state;
use crate::{ctx::Ctx, error::CliError};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

#[derive(Deserialize, Serialize)]
struct Reversal {
    config_path: PathBuf,
    config_before: Option<Vec<u8>>,
    config_mode: Option<u32>,
    created_key: Option<PathBuf>,
    directories: Vec<PathBuf>,
}

fn path(ctx: &Ctx<'_>) -> PathBuf {
    ctx.paths.data_dir.join("init-reversal.json")
}

fn read(ctx: &Ctx<'_>) -> Result<Option<Reversal>, CliError> {
    match fs::read(path(ctx)) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| state::failure("invalid init reversal manifest")),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

pub(super) fn record(ctx: &Ctx<'_>, new_key: Option<&Path>) -> Result<(), CliError> {
    if let Some(mut manifest) = read(ctx)? {
        if let Some(key) = new_key {
            state::extend_paths(
                &mut manifest.directories,
                state::missing_directories(&[key.to_path_buf()]),
            );
            replace(
                &path(ctx),
                &serde_json::to_vec(&manifest).map_err(state::failure)?,
                Some(0o600),
            )?;
        }
        return Ok(());
    }
    let config_path = if ctx.paths.global_config.is_absolute() {
        ctx.paths.global_config.clone()
    } else {
        ctx.cwd.join(&ctx.paths.global_config)
    };
    refuse_symlink(&config_path)?;
    let config_before = match fs::read(&config_path) {
        Ok(bytes) => Some(bytes),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return Err(error.into()),
    };
    #[cfg(unix)]
    let config_mode = {
        use std::os::unix::fs::PermissionsExt;
        fs::metadata(&config_path)
            .ok()
            .map(|m| m.permissions().mode())
    };
    #[cfg(not(unix))]
    let config_mode = None;
    let mut files = vec![config_path.clone()];
    if let Some(key) = new_key {
        files.push(key.to_path_buf());
    }
    let manifest = Reversal {
        config_path,
        config_before,
        config_mode,
        created_key: None,
        directories: state::missing_directories(&files),
    };
    fs::create_dir_all(&ctx.paths.data_dir)?;
    let bytes = serde_json::to_vec(&manifest).map_err(state::failure)?;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path(ctx))?;
    file.write_all(&bytes)?;
    file.sync_all()?;
    Ok(())
}

pub(super) fn created_key(ctx: &Ctx<'_>, key: &Path) -> Result<(), CliError> {
    let mut manifest =
        read(ctx)?.ok_or_else(|| state::failure("missing init reversal manifest"))?;
    manifest.created_key = Some(key.to_path_buf());
    replace(
        &path(ctx),
        &serde_json::to_vec(&manifest).map_err(state::failure)?,
        Some(0o600),
    )
}

pub(crate) fn restore(ctx: &Ctx<'_>, dry_run: bool) -> Result<(), CliError> {
    let Some(manifest) = read(ctx)? else {
        return Ok(());
    };
    if dry_run {
        return Ok(());
    }
    refuse_symlink(&manifest.config_path)?;
    if let Some(bytes) = &manifest.config_before {
        replace(&manifest.config_path, bytes, manifest.config_mode)?;
    } else {
        remove_if_present(&manifest.config_path)?;
    }
    if let Some(key) = &manifest.created_key {
        refuse_symlink(key)?;
        remove_if_present(key)?;
    }
    state::remove_empty_directories(&manifest.directories)?;
    remove_if_present(&path(ctx))
}

fn refuse_symlink(path: &Path) -> Result<(), CliError> {
    if path
        .ancestors()
        .any(|parent| fs::symlink_metadata(parent).is_ok_and(|m| m.file_type().is_symlink()))
    {
        return Err(state::failure("init reversal target contains a symlink"));
    }
    Ok(())
}

fn remove_if_present(path: &Path) -> Result<(), CliError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error.into()),
    }
}

fn replace(path: &Path, bytes: &[u8], mode: Option<u32>) -> Result<(), CliError> {
    refuse_symlink(path)?;
    let parent = path
        .parent()
        .ok_or_else(|| state::failure("reversal target has no parent"))?;
    fs::create_dir_all(parent)?;
    let temporary = path.with_extension(format!("tkm-{}.tmp", std::process::id()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(mode.unwrap_or(0o600));
    }
    #[cfg(not(unix))]
    let _ = mode;
    let mut file = options.open(&temporary)?;
    let result = (|| {
        file.write_all(bytes)?;
        #[cfg(unix)]
        if let Some(mode) = mode {
            use std::os::unix::fs::PermissionsExt;
            file.set_permissions(fs::Permissions::from_mode(mode))?;
        }
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    drop(file);
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result.map_err(Into::into)
}
