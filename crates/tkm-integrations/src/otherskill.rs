//! Prompt skill selection and ranking evidence.
use crate::{SkillCandidate, SkillSource, detect::AdapterHealth, process};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, HashSet},
    ffi::OsString,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tkm_core::model::RepoKey;
use tkm_proto::Host;
/// Selection adapter using an explicit executable search path.
pub struct OtherskillSource {
    path: OsString,
    health: Arc<AdapterHealth>,
    env: BTreeMap<String, String>,
}
impl OtherskillSource {
    /// Creates an adapter.
    #[must_use]
    pub fn new(path: OsString, health: Arc<AdapterHealth>) -> Self {
        Self::new_with_env(path, health, BTreeMap::new())
    }
    /// Creates an adapter with an explicit inherited environment snapshot.
    #[must_use]
    pub fn new_with_env(
        path: OsString,
        health: Arc<AdapterHealth>,
        env: BTreeMap<String, String>,
    ) -> Self {
        Self { path, health, env }
    }
}
#[derive(Deserialize)]
struct Route {
    status: String,
    selected: Vec<Item>,
}
#[derive(Deserialize)]
struct Item {
    name: String,
    description: String,
}
impl SkillSource for OtherskillSource {
    fn candidates(&self, repo: &RepoKey, host: Host, prompt: &str) -> Vec<SkillCandidate> {
        self.candidates_in(repo, host, prompt, Path::new("."))
    }
    fn candidates_in(
        &self,
        _: &RepoKey,
        host: Host,
        prompt: &str,
        cwd: &Path,
    ) -> Vec<SkillCandidate> {
        let agent = match host {
            Host::Claude => "claude",
            Host::Codex => "codex",
            Host::Agy => "antigravity",
            Host::Opencode => "opencode",
            _ => return Vec::new(),
        };
        if self.health.in_backoff() {
            return Vec::new();
        }
        let result = (|| {
            let bin = process::binary(&self.path, "otherskill")?;
            let mut cmd = std::process::Command::new(bin);
            cmd.envs(&self.env);
            cmd.args([
                "select",
                "--json",
                "--agent",
                agent,
                "--prompt",
                prompt,
                "--timeout-ms",
                "600",
            ])
            .env("PATH", &self.path)
            .current_dir(cwd);
            let bytes = process::output(cmd, Duration::from_millis(700)).ok()?;
            let route: Route = serde_json::from_slice(&bytes).ok()?;
            if !matches!(route.status.as_str(), "ok" | "partial") {
                return None;
            }
            Some(route)
        })();
        let Some(route) = result else {
            self.health.report_failure("otherskill selection failed");
            return Vec::new();
        };
        let mut seen = HashSet::new();
        route
            .selected
            .into_iter()
            .filter(|item| seen.insert(item.name.clone()))
            .map(|item| SkillCandidate {
                text: format!("{}: {}", item.name, item.description),
                id: item.name,
            })
            .collect()
    }
}
/// Aggregated skill use evidence.
#[derive(Debug, Clone, serde::Serialize)]
pub struct SkillEvidence {
    /// Skill name.
    pub name: String,
    /// Number of loads.
    pub loaded: u64,
    /// Number of uses.
    pub used: u64,
}
/// Builds the deterministic ranking prior.
#[must_use]
pub fn ranking_prior(evidence: &[SkillEvidence]) -> serde_json::Value {
    let mut skills = evidence.to_vec();
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    serde_json::json!({"version":1,"source":"tokenimum","skills":skills})
}
/// Returns the credential file location.
#[must_use]
pub fn key_path(home: &Path) -> PathBuf {
    home.join(".config/otherskill/credentials.json")
}
/// Returns the credential location only with consent.
#[must_use]
pub fn reuse_key(consent: bool, home: &Path) -> Option<PathBuf> {
    consent.then(|| key_path(home))
}

/// Reads a private, non-symlink otherskill credential file.
#[must_use]
pub fn read_key(home: &Path) -> Option<String> {
    use std::io::Read;
    #[cfg(unix)]
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let path = key_path(home);
    let before = std::fs::symlink_metadata(&path).ok()?;
    if !before.file_type().is_file() {
        return None;
    }
    let mut file = std::fs::File::open(path).ok()?;
    let metadata = file.metadata().ok()?;
    #[cfg(unix)]
    if metadata.permissions().mode() & 0o077 != 0
        || before.dev() != metadata.dev()
        || before.ino() != metadata.ino()
    {
        return None;
    }
    #[cfg(not(unix))]
    return None;
    let mut contents = String::new();
    file.read_to_string(&mut contents).ok()?;
    let value: serde_json::Value = serde_json::from_str(&contents).ok()?;
    value
        .get("openrouter")?
        .as_str()
        .filter(|key| !key.trim().is_empty())
        .map(str::to_owned)
}
