// Portions ported from microsoft/TokenEconomizer (tokeneconomizer/toolprune/stage.py),
// Copyright (c) Microsoft Corporation, licensed under the Apache License, Version 2.0.
//! Pure tool pruning and skill usage evidence.
pub mod skills;

use crate::model::{Host, RepoKey, Session, Timestamp, ToolKind};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
/// Pruning mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PruneMode {
    /// Disable pruning.
    Off,
    /// Report evidence.
    Report,
    /// Propose changes.
    #[default]
    Propose,
    /// Apply changes after the quiet window.
    Apply,
}
/// Evidence and scheduling configuration (SPEC 4.3).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PruneConfig {
    /// Prompt threshold.
    pub evidence_prompts: u64,
    /// Session minimum.
    pub min_sessions: u64,
    /// Alternate session threshold.
    pub sessions_any: u64,
    /// Idle minutes.
    pub quiet_minutes: u64,
    /// Requested mode.
    pub mode: PruneMode,
    /// User-protected names or prefix patterns.
    pub protect: Vec<String>,
    /// Whether the delegation adapter is enabled.
    pub superorchestrator_adapter_enabled: bool,
}
impl Default for PruneConfig {
    fn default() -> Self {
        Self {
            evidence_prompts: 10,
            min_sessions: 2,
            sessions_any: 5,
            quiet_minutes: 60,
            mode: PruneMode::Propose,
            protect: Vec::new(),
            superorchestrator_adapter_enabled: false,
        }
    }
}
/// Counts since the last use.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolEvidence {
    /// Unused loaded prompts.
    pub prompts: u64,
    /// Unused loaded sessions.
    pub sessions: u64,
    /// Total prompts where this tool was loaded.
    pub loaded_prompts: u64,
    /// Total prompts containing a call to the tool.
    pub used_prompts: u64,
    /// Total sessions where this tool was loaded.
    pub loaded_sessions: u64,
    /// Total sessions containing a call to the tool.
    pub used_sessions: u64,
}
/// Evidence for a single repository and host.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PruneEvidence {
    /// Repository identity fixed by the first complete session.
    pub repo_key: Option<RepoKey>,
    /// Host identity fixed by the first complete session.
    pub host: Option<Host>,
    /// Tools and their unused streaks.
    pub tools: BTreeMap<String, ToolEvidence>,
}
/// Fold a completed session in chronological order. State binds to the first complete session’s repository and host; mismatched
/// and partial sessions are ignored. Callers fold each session once in chronological order.
pub fn fold(state: &mut PruneEvidence, session: &Session) {
    if session.partial {
        return;
    }
    if state
        .repo_key
        .as_ref()
        .is_some_and(|key| key != &session.repo_key)
        || state.host.is_some_and(|host| host != session.host)
    {
        return;
    }
    state.repo_key = Some(session.repo_key.clone());
    state.host = Some(session.host);
    let loaded: std::collections::BTreeSet<_> = session
        .tool_catalog
        .iter()
        .filter(|tool| tool.kind != ToolKind::Skill)
        .map(|tool| tool.name.as_str())
        .collect();
    for name in &loaded {
        state.tools.entry((*name).to_owned()).or_default();
    }
    for turn in &session.turns {
        let used: std::collections::BTreeSet<_> =
            turn.calls.iter().map(|call| call.tool.as_str()).collect();
        for (name, e) in &mut state.tools {
            if used.contains(name.as_str()) {
                e.prompts = 0;
                e.sessions = 0;
                e.used_prompts = e.used_prompts.saturating_add(1);
            } else if loaded.contains(name.as_str()) {
                e.prompts = e.prompts.saturating_add(1);
            }
            if loaded.contains(name.as_str()) {
                e.loaded_prompts = e.loaded_prompts.saturating_add(1);
            }
        }
    }
    for (name, e) in &mut state.tools {
        let used = session
            .turns
            .iter()
            .flat_map(|turn| &turn.calls)
            .any(|call| call.tool == *name);
        if used {
            e.used_sessions = e.used_sessions.saturating_add(1);
        }
        if loaded.contains(name.as_str()) {
            e.loaded_sessions = e.loaded_sessions.saturating_add(1);
            if !used
                || session
                    .turns
                    .last()
                    .is_some_and(|turn| !turn.calls.iter().any(|call| call.tool == *name))
            {
                e.sessions = e.sessions.saturating_add(1);
            }
        }
    }
}
/// Whether a tool is protected by TE, the host or user patterns.
#[must_use]
pub fn is_protected(name: &str, host: Host, config: &PruneConfig) -> bool {
    const PROTECTED: &[&str] = &[
        "bash",
        "view",
        "grep",
        "glob",
        "edit",
        "create",
        "str_replace",
        "read_bash",
        "stop_bash",
        "list_bash",
        "read_file",
        "shell",
        "apply_patch",
        "Bash",
        "Edit",
        "Write",
        "Read",
        "Grep",
        "Glob",
        "MultiEdit",
        "LS",
        "NotebookRead",
        "BashOutput",
        "KillShell",
        "KillBash",
        "AskUserQuestion",
        "EnterPlanMode",
        "ExitPlanMode",
        "TodoWrite",
        "TaskCreate",
        "TaskUpdate",
        "TaskGet",
        "TaskList",
        "TaskOutput",
        "TaskStop",
        "Agent",
        "Task",
        "Skill",
        "ToolSearch",
        "SlashCommand",
        "Monitor",
        "WebFetch",
        "WebSearch",
    ];
    PROTECTED.contains(&name)
        || matches!(host, Host::Opencode) && ["skill", "task"].contains(&name)
        || matches!(host, Host::Pi) && ["read", "bash", "edit", "write"].contains(&name)
        || name.starts_with("mcp__headroom__")
        || config.superorchestrator_adapter_enabled && name.starts_with("mcp__superorchestrator__")
        || config.protect.iter().any(|p| {
            p.strip_suffix('*')
                .map_or(name == p, |prefix| name.starts_with(prefix))
        })
}
/// Whether an unused streak meets either threshold (SPEC 4.3).
#[must_use]
pub fn eligible(evidence: &ToolEvidence, config: &PruneConfig) -> bool {
    evidence.prompts >= config.evidence_prompts && evidence.sessions >= config.min_sessions
        || evidence.sessions >= config.sessions_any
}
/// Whether the idle window or 24-hour cap permits a write (SPEC 7.3).
#[must_use]
pub fn write_allowed(
    last_activity: Timestamp,
    ready_since: Timestamp,
    now: Timestamp,
    config: &PruneConfig,
) -> bool {
    let idle = i128::from(now.0) - i128::from(last_activity.0);
    let ready = i128::from(now.0) - i128::from(ready_since.0);
    idle >= i128::from(config.quiet_minutes) * 60_000 || ready >= 86_400_000
}
/// Effective mode after host and delegation restrictions.
#[must_use]
pub fn effective_mode(
    host: Host,
    config: &PruneConfig,
    last_delegated_run: Option<Timestamp>,
    now: Timestamp,
) -> PruneMode {
    if config.mode != PruneMode::Apply {
        return config.mode;
    }
    if !matches!(
        host,
        Host::Claude | Host::Cursor | Host::Opencode | Host::Pi
    ) {
        return PruneMode::Report;
    }
    if config.superorchestrator_adapter_enabled
        && last_delegated_run.is_some_and(|last| {
            let age = i128::from(now.0) - i128::from(last.0);
            (0..=2_592_000_000).contains(&age)
        })
    {
        return PruneMode::Propose;
    }
    PruneMode::Apply
}
/// A pure decision; it never writes settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PruneDecision {
    /// Effective mode.
    pub mode_effective: PruneMode,
    /// Eligible unprotected tools.
    pub candidates: Vec<String>,
    /// Eligible protected tools.
    pub protected_skipped: Vec<String>,
}
/// Decide from evidence.
#[must_use]
pub fn decide(
    state: &PruneEvidence,
    host: Host,
    config: &PruneConfig,
    last_delegated_run: Option<Timestamp>,
    now: Timestamp,
) -> PruneDecision {
    let mode_effective = effective_mode(host, config, last_delegated_run, now);
    let mut result = PruneDecision {
        mode_effective,
        candidates: Vec::new(),
        protected_skipped: Vec::new(),
    };
    if mode_effective == PruneMode::Off {
        return result;
    }
    for (name, evidence) in &state.tools {
        if eligible(evidence, config) {
            if is_protected(name, host, config) {
                result.protected_skipped.push(name.clone());
            } else {
                result.candidates.push(name.clone());
            }
        }
    }
    result
}
