//! Secret and path redaction.
use std::path::{Path, PathBuf};
/// Inputs used for redaction.
pub struct RedactContext {
    /// Repository root.
    pub repo_root: PathBuf,
    /// Home directory.
    pub home: PathBuf,
    /// Environment values.
    pub env: Vec<(String, String)>,
}
/// Reusable redactor.
pub struct Redactor {
    context: RedactContext,
}
impl Redactor {
    /// Creates a redactor.
    #[must_use]
    pub fn new(context: RedactContext) -> Self {
        Self { context }
    }
    /// Redacts sensitive text.
    #[must_use]
    pub fn redact(&self, text: &str) -> String {
        text.to_owned()
    }
}
/// Whether a path is a dotenv file.
#[must_use]
pub fn is_dotenv_path(_path: &Path) -> bool {
    false
}
