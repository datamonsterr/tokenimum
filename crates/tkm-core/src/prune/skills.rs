//! Report-only skill usage. No skill is proposed for removal.
use crate::model::{Session, ToolKind};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

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
/// are recorded even when the session catalog omits the skill.
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
    #[derive(Deserialize)]
    struct SkillArgs {
        skill: String,
    }
    for call in session.turns.iter().flat_map(|turn| &turn.calls) {
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
