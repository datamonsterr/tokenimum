//! Skill generation (stub for the red commit).
use crate::{AdapterError, InstallReport};
use std::path::{Path, PathBuf};
use tkm_proto::Host;

/// Stub.
#[must_use]
pub fn render(_host: Host) -> String {
    String::new()
}
/// Stub.
#[must_use]
pub fn skill_path(_host: Host, _home: &Path, _repo: Option<&Path>) -> Option<PathBuf> {
    None
}
/// Stub.
///
/// # Errors
/// Always unsupported.
pub fn install(
    _h: Host,
    _a: &Path,
    _r: Option<&Path>,
    _d: bool,
) -> Result<InstallReport, AdapterError> {
    Err(AdapterError::Unsupported {
        operation: "stub".into(),
    })
}
/// Stub.
///
/// # Errors
/// Always unsupported.
pub fn uninstall(
    _h: Host,
    _a: &Path,
    _r: Option<&Path>,
    _d: bool,
) -> Result<InstallReport, AdapterError> {
    Err(AdapterError::Unsupported {
        operation: "stub".into(),
    })
}
