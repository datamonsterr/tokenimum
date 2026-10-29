//! Secret and path redaction.
//!
//! Moonshot and `DeepSeek` use `sk-` credentials, which are indistinguishable
//! from legacy `OpenAI` keys: <https://platform.kimi.ai/docs/get-your-api-key>
//! and <https://api-docs.deepseek.com/>. Cursor documents API key authentication
//! but does not promise a prefix: <https://cursor.com/docs/cloud-agent/api/endpoints>.
//! Named assignments are therefore also removed, regardless of vendor prefix.
use regex::{Captures, Regex};
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

const PATTERNS: &[(&str, &str)] = &[
    (
        "private-key",
        r"(?s)-----BEGIN [A-Z ]*PRIVATE KEY-----.*?-----END [A-Z ]*PRIVATE KEY-----",
    ),
    ("aws", r"\b(?:AKIA|ASIA)[A-Z0-9]{16}\b"),
    (
        "aws-secret",
        r#"(?i)aws_secret_access_key\s*[=:]\s*[^\s,;"']+"#,
    ),
    (
        "github",
        r"\b(?:gh[pousr]_[A-Za-z0-9_]+|github_pat_[A-Za-z0-9_]+)",
    ),
    ("openrouter", r"\bsk-or-v1-[A-Za-z0-9_-]+"),
    ("anthropic", r"\bsk-ant-[A-Za-z0-9_-]+"),
    ("openai", r"\bsk-(?:proj-|svcacct-)[A-Za-z0-9_-]+"),
    ("cursor", r"\b(?:cursor|key)-[A-Za-z0-9_-]{16,}"),
    (
        "jwt",
        r"\beyJ[A-Za-z0-9_-]+\.eyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+",
    ),
    ("openai", r"\bsk-[A-Za-z0-9_-]{12,}"),
    (
        "assignment",
        r#"(?i)["']?[a-z0-9_]*(?:key|token|secret|password)[a-z0-9_]*["']?\s*[:=]\s*(?:"[^"]*"|'[^']*'|[^\s,;]+)"#,
    ),
];
// Any invalid pattern causes a fail-closed redaction, never partial coverage.
static COMPILED: LazyLock<Option<Vec<(&'static str, Regex)>>> = LazyLock::new(|| {
    PATTERNS
        .iter()
        .map(|(kind, pattern)| Regex::new(pattern).ok().map(|re| (*kind, re)))
        .collect()
});
static PATHS: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r#"(^|[\s"'=:(\[{])((?:/[^\s"'<>|,;:()\[\]{}]+)+/?)"#).ok());
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
    /// Redacts sensitive text. Invalid built-in patterns fail closed.
    #[must_use]
    pub fn redact(&self, text: &str) -> String {
        let (Some(patterns), Some(paths)) = (&*COMPILED, &*PATHS) else {
            return "[redacted]".into();
        };
        let mut result = text.to_owned();
        // Longest values first prevent shorter values from exposing suffixes.
        let mut env: Vec<_> = self.context.env.iter().collect();
        env.sort_by_key(|(_, value)| std::cmp::Reverse(value.len()));
        for (name, value) in env {
            let secret_name = ["KEY", "TOKEN", "SECRET", "PASSWORD"]
                .iter()
                .any(|part| name.to_ascii_uppercase().contains(part));
            if value.len() >= 8 || (secret_name && value.len() >= 4) {
                result = result.replace(value, &format!("[env:{name}]"));
            }
        }
        for (kind, re) in patterns {
            result = re
                .replace_all(&result, format!("[redacted:{kind}]").as_str())
                .into_owned();
        }
        paths
            .replace_all(&result, |caps: &Captures<'_>| {
                let path = Path::new(&caps[2]);
                let prefix = &caps[1];
                if let Ok(relative) = path.strip_prefix(&self.context.repo_root) {
                    if !relative
                        .components()
                        .any(|part| matches!(part, std::path::Component::ParentDir))
                    {
                        return format!("{prefix}{}", relative.to_string_lossy());
                    }
                }
                format!("{prefix}[outside-repo]")
            })
            .into_owned()
    }
}
/// Whether a path is a dotenv file.
#[must_use]
pub fn is_dotenv_path(path: &Path) -> bool {
    path.file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name == ".env" || name.starts_with(".env.") || name.ends_with(".env"))
}
#[cfg(test)]
mod tests {
    #[test]
    fn every_pattern_compiles() {
        assert!(super::COMPILED.is_some() && super::PATHS.is_some());
    }
}
