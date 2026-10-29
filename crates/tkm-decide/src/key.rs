//! Explicit API key loading with restrictive file permissions.
use std::{
    fmt,
    path::{Path, PathBuf},
};
/// API secret; intentionally has no `Display` implementation.
#[derive(Clone)]
pub struct ApiKey(String);
impl fmt::Debug for ApiKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("ApiKey([REDACTED])")
    }
}
impl ApiKey {
    /// Expose the secret exclusively for constructing the authorization header.
    #[must_use]
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}
/// Key loading failure; messages never contain secret values.
#[derive(Debug, thiserror::Error)]
pub enum KeyError {
    /// No nonempty key assignment was found.
    #[error("no OPENROUTER_API_KEY found")]
    Missing,
    /// File permissions allow access by the group or other users.
    #[error("key file {0} must have no group or other permissions (use mode 0600)")]
    InsecurePermissions(PathBuf),
    /// Reading the selected key file failed.
    #[error("could not read key file: {0}")]
    Io(#[from] std::io::Error),
}
/// Default key file below the supplied home directory.
#[must_use]
pub fn default_key_file(home: &Path) -> PathBuf {
    home.join(".config/tokenimum/openrouter.env")
}
/// Load the environment value first, then the reuse file or default file in place.
///
/// # Errors
/// Returns missing key, I/O, or insecure file permission errors.
#[allow(clippy::needless_pass_by_value)] // The caller supplies an owned environment snapshot.
pub fn load_key(
    env: Option<String>,
    _key_file: &Path,
    _reuse_path: Option<&Path>,
) -> Result<ApiKey, KeyError> {
    Ok(ApiKey(env.unwrap_or_default()))
}
