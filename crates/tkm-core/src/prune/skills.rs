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
/// is a single-field JSON object with a `skill` string matching a catalog name.
/// Other and ambiguous digest forms are ignored.
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
    let Ok(pattern) = regex::Regex::new(r#"^\s*\{\s*"skill"\s*:\s*"([A-Za-z0-9_:/.-]+)"\s*\}\s*$"#)
    else {
        return;
    };
    for call in session.turns.iter().flat_map(|turn| &turn.calls) {
        if !["Skill", "skill"].contains(&call.tool.as_str()) {
            continue;
        }
        if let Some(name) = pattern
            .captures(&call.args_digest)
            .and_then(|captures| captures.get(1))
            && names.contains(name.as_str())
            && let Some(item) = report.skills.get_mut(name.as_str())
        {
            item.invocations = item.invocations.saturating_add(1);
        }
    }
}
