//! Secret and path redaction.
//! Paths support Linux and macOS; Windows path redaction is out of scope.
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
    ("authorization", r"(?im)\bauthorization[ \t]*:[^\r\n]*"),
    ("bearer", r"(?i)\bbearer\s+[A-Za-z0-9._~+/=-]{8,}"),
    (
        "url-credentials",
        r"([A-Za-z][A-Za-z0-9+.-]*://)[^\s/:@]+:[^\s/@]*@",
    ),
    (
        "assignment",
        r#"(?i)["']?[a-z0-9_]*(?:key|token|secret|password)[a-z0-9_]*["']?\s*[:=]\s*(?:"[^"]*"|'[^']*'|[^\r\n]+)"#,
    ),
];
// Any invalid pattern causes a fail-closed redaction, never partial coverage.
static COMPILED: LazyLock<Option<Vec<(&'static str, Regex)>>> = LazyLock::new(|| {
    PATTERNS
        .iter()
        .map(|(kind, pattern)| Regex::new(pattern).ok().map(|re| (*kind, re)))
        .collect()
});
static PRIVATE_KEYS: LazyLock<Option<Regex>> =
    LazyLock::new(|| Regex::new(r"-----BEGIN ([A-Z ]*PRIVATE KEY[A-Z ]*)-----").ok());
static PATHS: LazyLock<Option<Regex>> = LazyLock::new(|| {
    Regex::new(r#"(^|[\s"'=:(\[{,;|<>@&`])((?:/[^\s"'<>|,;:()\[\]{}&`]+)+/?)"#).ok()
});
/// Inputs used for redaction.
#[allow(clippy::module_name_repetitions)] // Public name identifies redactor inputs.
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
        let (Some(patterns), Some(paths), Some(private_keys)) =
            (&*COMPILED, &*PATHS, &*PRIVATE_KEYS)
        else {
            return "[redacted]".into();
        };
        let mut result = redact_private_keys(text, private_keys);
        // Longest values first prevent shorter values from exposing suffixes.
        let mut env: Vec<_> = self.context.env.iter().collect();
        env.sort_by_key(|(_, value)| std::cmp::Reverse(value.len()));
        for (name, value) in env {
            let secret_name = ["KEY", "TOKEN", "SECRET", "PASSWORD"]
                .iter()
                .any(|part| name.to_ascii_uppercase().contains(part));
            if (secret_name || (!value.starts_with('/') && !value.starts_with('~')))
                && (value.len() >= 8 || (secret_name && value.len() >= 4))
            {
                result = result.replace(value, &format!("[env:{name}]"));
            }
        }
        for (kind, re) in patterns {
            let replacement = if *kind == "url-credentials" {
                "${1}[redacted:url-credentials]@".to_owned()
            } else {
                format!("[redacted:{kind}]")
            };
            result = re.replace_all(&result, replacement.as_str()).into_owned();
        }
        let result = paths
            .replace_all(&result, |caps: &Captures<'_>| {
                let path = Path::new(&caps[2]);
                let prefix = &caps[1];
                // The slashes after a URL scheme are not a filesystem path.
                if prefix == ":" && caps[2].starts_with("//") {
                    return caps[0].to_owned();
                }
                if !self.context.repo_root.as_os_str().is_empty()
                    && let Ok(relative) = path.strip_prefix(&self.context.repo_root)
                    && !relative
                        .components()
                        .any(|part| matches!(part, std::path::Component::ParentDir))
                {
                    return format!("{prefix}{}", relative.to_string_lossy());
                }
                format!("{prefix}[outside-repo]")
            })
            .into_owned();
        let home = self.context.home.to_string_lossy();
        if home.is_empty() {
            result
        } else {
            result.replace(home.as_ref(), "~")
        }
    }
}
// Match end labels explicitly: the regex engine has no backreferences.
fn redact_private_keys(text: &str, starts: &Regex) -> String {
    let mut result = String::with_capacity(text.len());
    let mut cursor = 0;
    for captures in starts.captures_iter(text) {
        let Some(start) = captures.get(0) else {
            return "[redacted]".into();
        };
        if start.start() < cursor {
            continue;
        }
        let end_marker = format!("-----END {}-----", &captures[1]);
        let end = text[start.end()..]
            .find(&end_marker)
            .map_or(text.len(), |offset| start.end() + offset + end_marker.len());
        result.push_str(&text[cursor..start.start()]);
        result.push_str("[redacted:private-key]");
        cursor = end;
    }
    result.push_str(&text[cursor..]);
    result
}
/// Whether a path is a dotenv file.
#[must_use]
pub fn is_dotenv_path(path: &Path) -> bool {
    path.extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|extension| extension.eq_ignore_ascii_case("env"))
        || path
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|name| {
                name.eq_ignore_ascii_case(".env")
                    || name
                        .get(..5)
                        .is_some_and(|prefix| prefix.eq_ignore_ascii_case(".env."))
            })
}
#[cfg(test)]
mod tests {
    #[test]
    fn every_pattern_compiles() {
        assert!(
            super::COMPILED.is_some() && super::PATHS.is_some() && super::PRIVATE_KEYS.is_some()
        );
    }
}
