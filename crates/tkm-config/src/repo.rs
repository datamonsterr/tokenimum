// Ported from microsoft/TokenEconomizer tokeneconomizer/plugin/paths.py (commit
// d0675a3385948d330a793863794a432f3490b1ac), Copyright (c) Microsoft Corporation,
// Apache-2.0; modified.
//! Resolve shared repository identities without invoking git.
use std::{
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
};
use tkm_core::model::RepoKey;

/// Strategy used to derive a repository identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IdentitySource {
    /// A normalized remote URL.
    Remote,
    /// The main checkout's real path.
    Path,
    /// The starting directory's real path.
    Dir,
}
impl IdentitySource {
    /// Return the stable textual spelling of this strategy.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Remote => "remote",
            Self::Path => "path",
            Self::Dir => "dir",
        }
    }
}
/// The checkout and its shared identity.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepoIdentity {
    /// Checkout root, or the starting directory outside a repository.
    pub root: PathBuf,
    /// Stable identity shared by related worktrees.
    pub key: RepoKey,
    /// Human-readable name.
    pub label: String,
    /// Normalized remote, if present.
    pub remote: Option<String>,
    /// Identity strategy: `remote`, `path`, or `dir`.
    pub how: IdentitySource,
}
fn gitdir(dot: &Path) -> Option<PathBuf> {
    let content = fs::read_to_string(dot).ok()?;
    let path = Path::new(content.trim().strip_prefix("gitdir:")?.trim());
    Some(if path.is_absolute() {
        path.to_path_buf()
    } else {
        dot.parent()?.join(path)
    })
}
/// Whether a `.git` entry identifies a checkout.
#[must_use]
pub fn is_git_entry(dot: &Path) -> bool {
    if dot.is_dir() {
        dot.join("HEAD").is_file()
    } else {
        gitdir(dot).is_some_and(|p| p.is_dir())
    }
}
/// Find the nearest checkout, respecting the supplied ceilings and home.
#[must_use]
pub fn find_repo_root(start: &Path, ceilings: &str, home: Option<&Path>) -> Option<PathBuf> {
    let start = fs::canonicalize(start).ok()?;
    let home = home.and_then(|p| fs::canonicalize(p).ok());
    let ceilings: Vec<_> = ceilings
        .split(':')
        .filter(|s| !s.is_empty())
        .filter_map(|s| fs::canonicalize(s).ok())
        .collect();
    let mut current = start.as_path();
    loop {
        if is_git_entry(&current.join(".git")) {
            return Some(current.to_path_buf());
        }
        let parent = current.parent()?;
        if ceilings.iter().any(|c| c == parent) || home.as_deref() == Some(parent) {
            return None;
        }
        current = parent;
    }
}
fn common_dir(root: &Path) -> Option<PathBuf> {
    let dot = root.join(".git");
    if dot.is_dir() {
        return Some(dot);
    }
    let dir = gitdir(&dot)?;
    let common =
        fs::read_to_string(dir.join("commondir")).map_or(dir.clone(), |s| dir.join(s.trim()));
    fs::canonicalize(common).ok()
}
fn origin_url(root: &Path) -> Option<String> {
    let content = fs::read_to_string(common_dir(root)?.join("config")).ok()?;
    let mut current = None;
    let mut first = None;
    let mut origin = None;
    for line in content.lines().map(str::trim) {
        if line.starts_with('[') {
            current = line
                .strip_prefix("[remote ")
                .and_then(|s| s.strip_suffix(']'))
                .and_then(|s| s.trim().strip_prefix('"'))
                .and_then(|s| s.strip_suffix('"'));
        } else if let Some(name) = current
            && let Some((key, value)) = line.split_once('=')
            && key.trim() == "url"
            && !value.trim().is_empty()
        {
            let value = parse_config_value(value);
            if value.is_empty() {
                continue;
            }
            if first.is_none() {
                first = Some(value.clone());
            }
            if name == "origin" && origin.is_none() {
                origin = Some(value);
            }
        }
    }
    origin.or(first)
}
fn parse_config_value(raw: &str) -> String {
    let mut quoted = false;
    let mut escaped = false;
    let mut end = raw.len();
    for (index, ch) in raw.char_indices() {
        if escaped {
            escaped = false;
        } else if quoted && ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            quoted = !quoted;
        } else if !quoted && matches!(ch, '#' | ';') {
            end = index;
            break;
        }
    }
    let value = raw[..end].trim();
    let Some(inner) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) else {
        return value.to_owned();
    };
    let mut result = String::new();
    let mut chars = inner.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '\\' && matches!(chars.peek(), Some('"' | '\\')) {
            result.push(chars.next().unwrap_or(ch));
        } else {
            result.push(ch);
        }
    }
    result
}
/// Normalize a git remote to its canonical spelling.
#[must_use]
pub fn normalize_remote(url: &str) -> String {
    let mut value = url.trim();
    if let Some((_, rest)) = value.split_once("://") {
        value = rest;
    }
    if let Some((user, rest)) = value.split_once('@')
        && !user.contains('/')
    {
        value = rest;
    }
    let mut result = value.trim_end_matches('/').to_owned();
    if let Some((host, rest)) = result.split_once(':')
        && !host.contains('/')
        && rest
            .chars()
            .next()
            .is_some_and(|c| !c.is_ascii_digit() && c != '/')
    {
        result = format!("{host}/{rest}");
    }
    if let Some(stripped) = result.strip_suffix(".git") {
        result = stripped.to_owned();
    }
    result.to_lowercase()
}
/// Resolve a checkout identity from explicit start, ceiling and home inputs.
#[must_use]
pub fn repo_identity(start: &Path, ceilings: &str, home: Option<&Path>) -> RepoIdentity {
    let found = find_repo_root(start, ceilings, home);
    let root = found
        .clone()
        .unwrap_or_else(|| fs::canonicalize(start).unwrap_or_else(|_| start.to_path_buf()));
    let remote = found
        .as_ref()
        .and_then(|p| origin_url(p))
        .map(|s| normalize_remote(&s))
        .filter(|s| !s.is_empty());
    if let Some(remote) = remote {
        let label = remote
            .rsplit('/')
            .take(2)
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
            .collect::<Vec<_>>()
            .join("/");
        return RepoIdentity {
            root,
            key: RepoKey(remote.clone()),
            label,
            remote: Some(remote),
            how: IdentitySource::Remote,
        };
    }
    let path = found
        .as_ref()
        .and_then(|r| common_dir(r))
        .and_then(|c| {
            if c.file_name().is_some_and(|n| n == ".git") {
                c.parent().map(Path::to_path_buf)
            } else {
                None
            }
        })
        .unwrap_or_else(|| root.clone());
    let path = fs::canonicalize(&path).unwrap_or(path);
    let label = path
        .file_name()
        .map_or_else(|| "repo".to_owned(), |s| s.to_string_lossy().into_owned());
    let how = if found.is_some() {
        IdentitySource::Path
    } else {
        IdentitySource::Dir
    };
    RepoIdentity {
        root,
        key: RepoKey(format!("{}:{}", how.as_str(), path.display())),
        label,
        remote: None,
        how,
    }
}

/// Add the repository-local configuration file to the shared Git exclude file.
///
/// # Errors
/// Returns an error if the exclude file cannot be read or written.
pub fn exclude_local_config(repo_root: &Path) -> io::Result<bool> {
    let common = common_dir(repo_root)
        .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "Git common directory not found"))?;
    let info = common.join("info");
    fs::create_dir_all(&info)?;
    let path = info.join("exclude");
    let content = match fs::read_to_string(&path) {
        Ok(content) => content,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(error),
    };
    if content.lines().any(|line| line == "/.tokenimum.local.toml") {
        return Ok(false);
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    if !content.is_empty() && !content.ends_with('\n') {
        writeln!(file)?;
    }
    writeln!(file, "/.tokenimum.local.toml")?;
    Ok(true)
}
