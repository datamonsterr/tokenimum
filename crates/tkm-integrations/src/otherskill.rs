//! Prompt skill selection and ranking evidence.
use crate::{SkillCandidate, SkillSource, detect::AdapterHealth, process};
use serde::Deserialize;
use std::{
    collections::HashSet,
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
}
impl OtherskillSource {
    /// Creates an adapter.
    pub fn new(path: OsString, health: Arc<AdapterHealth>) -> Self {
        Self { path, health }
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
    fn candidates(&self, _: &RepoKey, host: Host, prompt: &str) -> Vec<SkillCandidate> {
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
            cmd.args([
                "select",
                "--json",
                "--agent",
                agent,
                "--prompt",
                prompt,
                "--timeout-ms",
                "900",
            ])
            .env("PATH", &self.path);
            let bytes = process::output(cmd, Duration::from_secs(1)).ok()?;
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
pub fn ranking_prior(evidence: &[SkillEvidence]) -> serde_json::Value {
    let mut skills = evidence.to_vec();
    skills.sort_by(|a, b| a.name.cmp(&b.name));
    serde_json::json!({"version":1,"source":"tokenimum","skills":skills})
}
/// Returns the credential file location.
pub fn key_path(home: &Path) -> PathBuf {
    home.join(".config/otherskill/credentials.json")
}
/// Returns the credential location only with consent.
pub fn reuse_key(consent: bool, home: &Path) -> Option<PathBuf> {
    consent.then(|| key_path(home))
}
