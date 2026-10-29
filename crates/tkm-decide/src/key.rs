//! Explicit API key loading with restrictive file permissions.
use std::{
    fmt,
    path::{Path, PathBuf},
};
/// API secret; intentionally has no `Display` implementation.
#[derive(Clone)]
#[allow(clippy::module_name_repetitions)] // Public names follow the decision-client contract.
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
#[allow(clippy::module_name_repetitions)] // Public names follow the decision-client contract.
pub enum KeyError {
    /// No nonempty key assignment was found.
    #[error("no OPENROUTER_API_KEY found")]
    Missing,
    /// File permissions allow access by the group or other users.
    #[error("key file {0} must have no group or other permissions (use mode 0600)")]
    InsecurePermissions(
        /// Selected key file with overly broad permissions.
        PathBuf,
    ),
    /// Reading the selected key file failed.
    #[error("could not read key file: {0}")]
    Io(
        /// Underlying filesystem failure, excluding file contents.
        #[from]
        std::io::Error,
    ),
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
pub fn load_key(
    env: Option<String>,
    key_file: &Path,
    reuse_path: Option<&Path>,
) -> Result<ApiKey, KeyError> {
    if let Some(value) = env.filter(|value| !value.trim().is_empty()) {
        return key_from_value(value);
    }
    let path = reuse_path.unwrap_or(key_file);
    let mut file = std::fs::File::open(path).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            KeyError::Missing
        } else {
            KeyError::Io(error)
        }
    })?;
    // Inspect the opened file rather than re-statting its path before reading.
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if file.metadata()?.permissions().mode() & 0o077 != 0 {
            return Err(KeyError::InsecurePermissions(path.to_path_buf()));
        }
    }
    let mut text = String::new();
    std::io::Read::read_to_string(&mut file, &mut text)?;
    // Dotenv assignments override earlier ones, ignoring empty or invalid values.
    for line in text.lines().rev() {
        let line = line.trim();
        let line = line.strip_prefix("export ").map_or(line, str::trim_start);
        let Some((name, value)) = line.split_once('=') else {
            continue;
        };
        if name.trim() != "OPENROUTER_API_KEY" {
            continue;
        }
        if let Ok(key) = parse_assignment(value).and_then(key_from_value) {
            return Ok(key);
        }
    }
    Err(KeyError::Missing)
}
fn key_from_value(value: String) -> Result<ApiKey, KeyError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err(KeyError::Missing);
    }
    if trimmed.len() == value.len() {
        Ok(ApiKey(value))
    } else {
        Ok(ApiKey(trimmed.to_owned()))
    }
}
fn parse_assignment(value: &str) -> Result<String, KeyError> {
    let value = value.trim_start();
    if let Some(quote) = value.chars().next().filter(|c| matches!(c, '\'' | '"')) {
        let rest = &value[quote.len_utf8()..];
        let Some(end) = rest.find(quote) else {
            return Err(KeyError::Missing);
        };
        let trailing = rest[end + quote.len_utf8()..].trim_start();
        if !trailing.is_empty() && !trailing.starts_with('#') {
            return Err(KeyError::Missing);
        }
        Ok(rest[..end].to_owned())
    } else {
        Ok(value
            .split('#')
            .next()
            .unwrap_or_default()
            .trim()
            .to_owned())
    }
}
