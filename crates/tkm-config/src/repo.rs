// Ported from microsoft/TokenEconomizer tokeneconomizer/plugin/paths.py (commit
// d0675a3385948d330a793863794a432f3490b1ac), Copyright (c) Microsoft Corporation,
// Apache-2.0; modified.
//! Resolve shared repository identities without invoking git.
use std::path::{Path, PathBuf};
use tkm_core::model::RepoKey;

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
    pub how: &'static str,
}
/// Whether a `.git` entry identifies a checkout.
#[must_use]
pub fn is_git_entry(dot: &Path) -> bool {
    unimplemented!()
}
/// Find the nearest checkout, respecting the supplied ceilings and home.
#[must_use]
pub fn find_repo_root(start: &Path, ceilings: &str, home: Option<&Path>) -> Option<PathBuf> {
    unimplemented!()
}
/// Normalize a git remote to its canonical spelling.
#[must_use]
pub fn normalize_remote(url: &str) -> String {
    unimplemented!()
}
/// Resolve a checkout identity from explicit start, ceiling and home inputs.
#[must_use]
pub fn repo_identity(start: &Path, ceilings: &str, home: Option<&Path>) -> RepoIdentity {
    unimplemented!()
}
