//! Instructions-file injection (stub for the red commit).
use crate::{AdapterError, InstallReport};
use std::path::Path;
use tkm_proto::Host;

/// Options for applying or removing the block.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ApplyOptions {
    /// Allow a git-tracked file.
    pub allow_tracked: bool,
    /// Compute without writing.
    pub dry_run: bool,
}
/// Stub.
#[must_use]
pub fn render_block(_body: &str) -> String {
    String::new()
}
/// Stub.
#[must_use]
pub fn splice(existing: &str, _body: &str) -> String {
    existing.to_owned()
}
/// Stub.
#[must_use]
pub fn remove(existing: &str) -> String {
    existing.to_owned()
}
/// Stub.
#[must_use]
pub const fn instructions_file(_host: Host) -> Option<&'static str> {
    None
}
/// Stub.
///
/// # Errors
/// Always unsupported.
pub fn apply(
    _r: &Path,
    _h: Host,
    _b: &str,
    _o: ApplyOptions,
) -> Result<InstallReport, AdapterError> {
    Err(AdapterError::Unsupported {
        operation: "stub".into(),
    })
}
/// Stub.
///
/// # Errors
/// Always unsupported.
pub fn remove_from(_r: &Path, _h: Host, _o: ApplyOptions) -> Result<InstallReport, AdapterError> {
    Err(AdapterError::Unsupported {
        operation: "stub".into(),
    })
}
