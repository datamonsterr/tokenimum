//! Report-only skill usage. No skill is proposed for removal.
use crate::model::{Session, ToolKind};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Deserialize)]
struct SkillArgs {
    skill: String,
}

#[derive(Deserialize)]
struct ReadArgs {
    file_path: String,
}

/// Skill name for a `.../<name>/SKILL.md` path. otherskill bank directories carry a
/// `--<hex>` suffix after the name, which is dropped.
fn skill_file_name(path: &str) -> Option<&str> {
    let directory = path.strip_suffix("/SKILL.md")?.rsplit('/').next()?;
    let name = directory
        .rsplit_once("--")
        .filter(|(_, hash)| !hash.is_empty() && hash.bytes().all(|b| b.is_ascii_hexdigit()))
        .map_or(directory, |(name, _)| name);
    (!name.is_empty()).then_some(name)
}

/// Counts of sessions in which a skill was loaded and calls that invoked it.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillUsage {
    /// Sessions listing the skill.
    pub loaded_sessions: u64,
    /// Calls recognized as invoking the skill.
    pub invocations: u64,
}
/// Skill evidence for one repository, across hosts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SkillReport {
    /// Per-name skill evidence, suitable for JSON output.
    pub skills: BTreeMap<String, SkillUsage>,
}
/// Fold one complete session. Callers keep one report per repository and fold
/// each session once. A `Skill` or `skill` call counts only when its digest
/// has an object digest with exactly one nonempty string `skill` field. Extra
/// fields are ignored; malformed and ambiguous digests do not count. Invocations
/// are recorded even when the session catalog omits the skill. A `Read` or `read` of a
/// `<name>/SKILL.md` file also counts, but only for a skill the session catalog lists,
/// since otherskill tells the model to read a selected skill's file instead of calling `Skill`.
pub fn fold_skills(report: &mut SkillReport, session: &Session) {
    if session.partial {
        return;
    }
    let names: BTreeSet<_> = session
        .tool_catalog
        .iter()
        .filter(|t| t.kind == ToolKind::Skill)
        .map(|t| t.name.as_str())
        .collect();
    for name in &names {
        let item = report.skills.entry((*name).to_owned()).or_default();
        item.loaded_sessions = item.loaded_sessions.saturating_add(1);
    }
    for call in session.turns.iter().flat_map(|turn| &turn.calls) {
        if ["Read", "read"].contains(&call.tool.as_str())
            && let Ok(args) = serde_json::from_str::<ReadArgs>(&call.args_digest)
            && let Some(name) = skill_file_name(&args.file_path).filter(|name| names.contains(name))
        {
            let item = report.skills.entry(name.to_owned()).or_default();
            item.invocations = item.invocations.saturating_add(1);
            continue;
        }
        if !["Skill", "skill"].contains(&call.tool.as_str())
            || !call.args_digest.trim_start().starts_with('{')
        {
            continue;
        }
        if let Ok(args) = serde_json::from_str::<SkillArgs>(&call.args_digest)
            && !args.skill.trim().is_empty()
        {
            let item = report.skills.entry(args.skill).or_default();
            item.invocations = item.invocations.saturating_add(1);
        }
    }
}
