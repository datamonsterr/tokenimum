//! Dashboard discovery markers that every `tkmd` build can write (SPEC 11.4).
//!
//! A build with the `web` feature writes the live discovery file from [`crate::web`]. A build
//! without it writes a marker when `ui.web.enabled = true`, so `tkm dashboard` can name the
//! missing build feature instead of guessing.

use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

use tkm_config::paths::Paths;

/// Reason written when this binary lacks the `web` feature.
pub const FEATURE_MISSING: &str = "feature";

/// Write `tkmd-web.json` saying the dashboard cannot start, with `reason`.
///
/// # Errors
/// Returns an error when the marker cannot be written.
pub fn write_unavailable(paths: &Paths, reason: &str) -> std::io::Result<()> {
    write_marker(paths, &serde_json::json!({ "unavailable": reason }))
}

/// Write `tkmd-web.json` saying the dashboard could not bind `bind`, so `tkm dashboard` can name
/// the address and the fix. The `(os error N)` suffix is dropped from `error`.
///
/// # Errors
/// Returns an error when the marker cannot be written.
pub fn write_bind_failure(paths: &Paths, bind: &str, error: &str) -> std::io::Result<()> {
    let error = error.split(" (os error").next().unwrap_or(error);
    write_marker(
        paths,
        &serde_json::json!({ "unavailable": "bind", "bind": bind, "error": error }),
    )
}

fn write_marker(paths: &Paths, value: &serde_json::Value) -> std::io::Result<()> {
    let file = paths.socket.with_file_name("tkmd-web.json");
    let temp = paths
        .socket
        .with_file_name(format!("tkmd-web-{}.tmp", std::process::id()));
    let contents = serde_json::to_vec(value)?;
    let result = (|| {
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)?;
        output.set_permissions(std::fs::Permissions::from_mode(0o600))?;
        output.write_all(&contents)?;
        output.sync_all()?;
        std::fs::rename(&temp, &file)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temp);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bind_marker_records_address_and_error() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let mut paths = Paths::from_dirs(dir.path(), None, None, None);
        paths.socket = dir.path().join("tkmd.sock");
        write_bind_failure(
            &paths,
            "127.0.0.1:7878",
            "Address already in use (os error 98)",
        )?;
        let value: serde_json::Value =
            serde_json::from_slice(&std::fs::read(dir.path().join("tkmd-web.json"))?)?;
        assert_eq!(
            value,
            serde_json::json!({"unavailable":"bind","bind":"127.0.0.1:7878","error":"Address already in use"})
        );
        Ok(())
    }
    #[test]
    fn unavailable_marker_is_private_json() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let mut paths = Paths::from_dirs(dir.path(), None, None, None);
        paths.socket = dir.path().join("tkmd.sock");
        write_unavailable(&paths, FEATURE_MISSING)?;
        let file = dir.path().join("tkmd-web.json");
        let value: serde_json::Value = serde_json::from_slice(&std::fs::read(&file)?)?;
        assert_eq!(value, serde_json::json!({"unavailable": "feature"}));
        assert_eq!(
            std::fs::metadata(&file)?.permissions().mode() & 0o777,
            0o600
        );
        Ok(())
    }
}
