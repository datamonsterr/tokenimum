//! Read-only repository reports, with explicit time for deterministic callers.
#![allow(clippy::too_many_lines)]

pub(crate) mod report;
use super::why;
use crate::{
    cli::StatusArgs,
    ctx::Ctx,
    daemon::{self, DaemonState},
    error::CliError,
};
use serde::Serialize;
use std::collections::BTreeSet;
use std::fmt::Write as _;
use std::process::ExitCode;
use tkm_config::config::EffectiveConfig;
use tkm_core::explain::ActionKind;
use tkm_core::ledger::{SavingsEstimate, SavingsSession, estimate_savings};
use tkm_core::model::{Host, RepoKey, Timestamp};
use tkm_store::{EventRecord, FactState, Store, StoreError};

#[derive(Serialize)]
struct StatusReport {
    daemon: DaemonReport,
    preset: String,
    adapters: Vec<AdapterReport>,
    repositories: Vec<RepoReport>,
}
#[derive(Serialize)]
struct DaemonReport {
    state: String,
    version: String,
    uptime: String,
}
#[derive(Serialize)]
struct AdapterReport {
    name: String,
    enabled: bool,
    reason: String,
}
#[derive(Serialize)]
struct RepoReport {
    repo_key: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    config_source: Option<&'static str>,
    harnesses: Vec<HarnessReport>,
    facts: FactCounts,
    loop_guard: LoopReport,
    pruning: PruneReport,
    spend: SpendReport,
    savings: Vec<HostSavings>,
    last_actions: Vec<ActionReport>,
}
#[derive(Serialize)]
struct HarnessReport {
    host: String,
    level: String,
    enabled: bool,
    reason: String,
    last_event: String,
    events_missing: Vec<String>,
    coverage: String,
}
#[derive(Serialize)]
struct FactCounts {
    active: usize,
    candidates: usize,
    retired_last_7_days: usize,
}
#[derive(Serialize)]
struct LoopReport {
    mode: String,
    envelopes: Vec<EnvelopeReport>,
    redirects_last_7_days: usize,
    escalations_last_7_days: usize,
}
#[derive(Serialize)]
struct EnvelopeReport {
    host: String,
    calls: Option<f64>,
    samples: usize,
}
#[derive(Serialize)]
struct PruneReport {
    mode: String,
    pending_proposals: usize,
}
#[derive(Serialize)]
struct SpendReport {
    decisions_today_usd: f64,
    daily_cap_usd: f64,
    scope: &'static str,
}
#[derive(Serialize)]
struct HostSavings {
    host: String,
    estimate: Option<SavingsEstimate>,
}
#[derive(Serialize)]
struct ActionReport {
    event_id: i64,
    time: String,
    explanation: String,
}

/// Execute `tkm status` using the current UTC time.
///
/// # Errors
/// Returns an error on configuration, database or output failures.
pub fn run(ctx: &mut Ctx<'_>, args: &StatusArgs) -> Result<ExitCode, CliError> {
    run_at(ctx, args, report::now_ms())
}

/// Execute `tkm status` with a caller-supplied Unix-millisecond clock.
///
/// # Errors
/// Returns an error on configuration, database or output failures.
pub fn run_at(ctx: &mut Ctx<'_>, args: &StatusArgs, now_ms: i64) -> Result<ExitCode, CliError> {
    let store = match report::read_store(ctx) {
        Ok(store) => store,
        Err(CliError::MissingDatabase(_)) => {
            if args.json {
                writeln!(
                    ctx.stdout,
                    "{{\"message\":\"No database yet; run tkm init.\",\"repositories\":[]}}"
                )?;
            } else {
                writeln!(ctx.stdout, "No database yet; run tkm init.")?;
            }
            return Ok(ExitCode::SUCCESS);
        }
        Err(error) => return Err(error),
    };
    let config = ctx.load_config()?;
    let daemon = match daemon::probe(&ctx.paths) {
        DaemonState::Running {
            daemon_version,
            started_at_ms,
            ..
        } => DaemonReport {
            state: "running".into(),
            version: daemon_version.unwrap_or_else(|| "unknown".into()),
            uptime: started_at_ms.filter(|start| *start <= now_ms).map_or_else(
                || "unknown".into(),
                |start| format!("{}s", now_ms.saturating_sub(start) / 1000),
            ),
        },
        DaemonState::Down { .. } => DaemonReport {
            state: "down (read-only DB)".into(),
            version: "unknown".into(),
            uptime: "unknown".into(),
        },
    };
    let adapters = ["superorchestrator", "otherskill"]
        .into_iter()
        .map(|name| {
            let mode = config
                .get_str(&format!("integrations.{name}"))
                .unwrap_or("auto");
            AdapterReport {
                name: name.into(),
                enabled: false,
                reason: if mode == "off" {
                    "disabled in configuration".into()
                } else {
                    format!("adapter unavailable; configured {mode}")
                },
            }
        })
        .collect();
    let keys = if args.all {
        store
            .list_repos()
            .map_err(|e| report::store_error(ctx, e))?
            .into_iter()
            .map(|r| r.repo_key)
            .collect()
    } else {
        vec![ctx.repo.key.clone()]
    };
    let events = store
        .list_recent_events(-1)
        .map_err(|e| report::store_error(ctx, e))?;
    let day_start = now_ms.saturating_sub(now_ms.rem_euclid(86_400_000));
    let today = store
        .list_recent_decisions(-1)
        .map_err(|e| report::store_error(ctx, e))?
        .iter()
        .filter(|d| {
            d.created_at
                .is_some_and(|t| t.0 >= day_start && t.0 <= now_ms)
        })
        .map(|d| d.cost_usd)
        .sum();
    let mut repositories = Vec::new();
    let redactor = report::redactor(ctx);
    for repo in &keys {
        let (repo_config, source) = if args.all {
            let root = checkout(ctx, &store, repo)?;
            let source = if root.is_some() {
                "global + repository"
            } else {
                "global only (checkout unknown)"
            };
            (ctx.load_config_for_repo(root)?, Some(source))
        } else {
            (ctx.load_config()?, None)
        };
        let mut report = repo_report(
            &store,
            repo,
            &repo_config,
            &events,
            now_ms,
            today,
            &redactor,
        )
        .map_err(|e| report::store_error(ctx, e))?;
        report.config_source = source;
        repositories.push(report);
    }
    let report = StatusReport {
        daemon,
        preset: config.get_str("preset").unwrap_or("balanced").into(),
        adapters,
        repositories,
    };
    let output = if args.json {
        serde_json::to_string_pretty(&report).map_err(report::json_error)? + "\n"
    } else {
        render(&report)
    };
    ctx.stdout.write_all(output.as_bytes())?;
    Ok(ExitCode::SUCCESS)
}

fn checkout(
    ctx: &Ctx<'_>,
    store: &Store,
    repo: &RepoKey,
) -> Result<Option<std::path::PathBuf>, CliError> {
    use tkm_config::repo::{IdentitySource, repo_identity};
    let mut paths = vec![ctx.cwd.clone()];
    paths.extend(
        store
            .list_sessions_by_repo(repo, -1)
            .map_err(|e| report::store_error(ctx, e))?
            .into_iter()
            .map(|s| s.cwd),
    );
    let home = super::init::state::home(ctx).ok();
    let ceilings = super::init::state::env(ctx, "GIT_CEILING_DIRECTORIES").unwrap_or_default();
    for path in paths {
        if path.is_dir() {
            let identity = repo_identity(&path, ceilings, home.as_deref());
            if identity.key == *repo && identity.how != IdentitySource::Dir {
                return Ok(Some(identity.root));
            }
        }
    }
    Ok(None)
}

fn repo_report(
    store: &Store,
    repo: &RepoKey,
    config: &EffectiveConfig,
    events: &[EventRecord],
    now: i64,
    today: f64,
    redactor: &tkm_decide::redact::Redactor,
) -> Result<RepoReport, StoreError> {
    let sessions = store.list_sessions_by_repo(repo, -1)?;
    let ids: BTreeSet<_> = sessions.iter().map(|s| s.id).collect();
    let events: Vec<_> = events
        .iter()
        .filter(|e| ids.contains(&e.session_id))
        .collect();
    let recent = |time: Option<Timestamp>| {
        time.is_some_and(|t| t.0 >= now.saturating_sub(7 * 86_400_000) && t.0 <= now)
    };
    let facts = store.list_facts(repo, None)?;
    let installs = store.list_installs()?;
    let hosts: BTreeSet<_> = sessions
        .iter()
        .map(|s| s.key.host.as_str())
        .chain(installs.iter().map(|i| i.host.as_str()))
        .collect();
    let mut harnesses = Vec::new();
    let mut envelopes = Vec::new();
    let mut savings = Vec::new();
    let mut samples = Vec::new();
    let mut pending_tools = 0;
    let prune_config = tkm_core::prune::PruneConfig {
        evidence_prompts: u64::try_from(config.get_int("prune.evidence_prompts").unwrap_or(10))
            .unwrap_or(10),
        min_sessions: u64::try_from(config.get_int("prune.min_sessions").unwrap_or(2)).unwrap_or(2),
        sessions_any: u64::try_from(config.get_int("prune.sessions_any").unwrap_or(5)).unwrap_or(5),
        ..tkm_core::prune::PruneConfig::default()
    };
    let mut applied = Vec::new();
    for session in &sessions {
        applied.extend(store.list_denylist(&session.cwd)?);
    }

    for session in &sessions {
        if let Some(ledger) = store.get_ledger(session.id)? {
            samples.push(SavingsSession {
                repo: repo.clone(),
                host: session.key.host,
                started: session.started,
                shadow: session.shadow,
                cost: ledger.priced_cost + ledger.overhead_cost,
            });
        }
    }
    let first_action = events
        .iter()
        .filter(|e| {
            why::action_kind(e).is_some()
                && e.inputs.get("shadow").and_then(serde_json::Value::as_bool) != Some(true)
        })
        .filter_map(|e| e.created_at)
        .min_by_key(|t| t.0);
    for host in hosts {
        let Ok(host_id) = host.parse::<Host>() else {
            continue;
        };
        let host_sessions: Vec<_> = sessions.iter().filter(|s| s.key.host == host_id).collect();
        let host_ids: BTreeSet<_> = host_sessions.iter().map(|s| s.id).collect();
        let seen: Vec<_> = events
            .iter()
            .filter(|e| host_ids.contains(&e.session_id) && e.kind == "hook.seen")
            .collect();
        let names: BTreeSet<_> = seen
            .iter()
            .filter_map(|e| e.inputs.get("native").and_then(serde_json::Value::as_str))
            .collect();
        let missing = tkm_hosts::probe::expected_events(host_id)
            .iter()
            .filter(|e| e.required && !names.contains(e.native))
            .map(|e| e.native.to_owned())
            .collect();
        harnesses.push(HarnessReport {
            host: host.into(),
            level: host_sessions
                .first()
                .map_or("unknown", |s| s.integration_level.as_str())
                .into(),
            enabled: config
                .get_bool(&format!("hosts.{host}.enabled"))
                .unwrap_or(true),
            reason: if config.get_bool(&format!("hosts.{host}.enabled")) == Some(false) {
                "disabled in configuration"
            } else {
                "enabled in configuration"
            }
            .into(),
            last_event: report::time(seen.iter().filter_map(|e| e.created_at).max_by_key(|t| t.0)),
            events_missing: missing,
            coverage: if seen.is_empty() {
                "unknown (no hook observations)"
            } else {
                "observed"
            }
            .into(),
        });
        if config.get_str("prune.mode") != Some("off") {
            for evidence in store.list_tool_evidence(repo, host_id)? {
                let counts = tkm_core::prune::ToolEvidence {
                    prompts: u64::from(evidence.prompts_unused),
                    sessions: u64::from(evidence.sessions_unused),
                    ..tkm_core::prune::ToolEvidence::default()
                };
                if evidence.last_used.is_none()
                    && tkm_core::prune::eligible(&counts, &prune_config)
                    && !tkm_core::prune::is_protected(&evidence.tool, host_id, &prune_config)
                    && !applied
                        .iter()
                        .any(|a| a.host == host_id && a.tool == evidence.tool && !a.restored)
                {
                    pending_tools += 1;
                }
            }
        }
        let lengths = store.envelope_samples(repo, host_id)?;
        envelopes.push(EnvelopeReport {
            host: host.into(),
            calls: envelope(&lengths, config),
            samples: lengths.len(),
        });
        savings.push(HostSavings {
            host: host.into(),
            estimate: estimate_savings(&samples, repo, host_id, first_action),
        });
    }
    let pending = events
        .iter()
        .filter(|e| why::action_kind(e) == Some(ActionKind::DenyListProposal))
        .filter(|proposal| {
            !events.iter().any(|later| {
                later.id > proposal.id
                    && later.session_id == proposal.session_id
                    && matches!(
                        why::action_kind(later),
                        Some(ActionKind::DenyListWrite | ActionKind::Restore)
                    )
            })
        })
        .count();
    let mut actions = Vec::new();
    for event in events
        .iter()
        .filter(|e| why::action_kind(e).is_some())
        .take(5)
    {
        if let Some(session) = sessions.iter().find(|s| s.id == event.session_id)
            && let Some(explanation) = why::explanation(store, event, session, redactor)?
        {
            let rendered = tkm_core::explain::render(&explanation);
            let summary = rendered
                .split("What happened: ")
                .nth(1)
                .and_then(|s| s.split("\n\nWhy:").next())
                .unwrap_or_default();
            actions.push(ActionReport {
                event_id: event.id,
                time: report::time(event.created_at),
                explanation: report::line(summary),
            });
        }
    }
    Ok(RepoReport {
        repo_key: repo.0.clone(),
        config_source: None,
        harnesses,
        facts: FactCounts {
            active: facts
                .iter()
                .filter(|f| f.state == FactState::Active)
                .count(),
            candidates: facts
                .iter()
                .filter(|f| f.state == FactState::Candidate)
                .count(),
            retired_last_7_days: facts
                .iter()
                .filter(|f| f.state == FactState::Retired && recent(f.state_changed_at))
                .count(),
        },
        loop_guard: LoopReport {
            mode: config.get_str("loop.mode").unwrap_or("off").into(),
            envelopes,
            redirects_last_7_days: events
                .iter()
                .filter(|e| recent(e.created_at) && e.kind == "loop.redirect")
                .count(),
            escalations_last_7_days: events
                .iter()
                .filter(|e| recent(e.created_at) && e.kind == "loop.escalation")
                .count(),
        },
        pruning: PruneReport {
            mode: config.get_str("prune.mode").unwrap_or("off").into(),
            pending_proposals: pending.max(pending_tools),
        },
        spend: SpendReport {
            decisions_today_usd: today,
            daily_cap_usd: config.get_float("decide.budget_daily_usd").unwrap_or(0.0),
            scope: "global",
        },
        savings,
        last_actions: actions,
    })
}
fn envelope(samples: &[u32], config: &EffectiveConfig) -> Option<f64> {
    if samples.is_empty()
        || i64::try_from(samples.len()).ok()? < config.get_int("loop.envelope_min_samples")?
    {
        return None;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let last = u32::try_from(sorted.len() - 1).ok()?;
    let percentile = u32::try_from(config.get_int("loop.envelope_pct")?).ok()?;
    let margin = config.get_float("loop.envelope_margin")?;
    if percentile > 100 || !margin.is_finite() || margin <= 0.0 {
        return None;
    }
    let index = f64::from(percentile) / 100.0 * f64::from(last);
    let lo = (0..=last).take_while(|n| f64::from(*n) <= index).last()?;
    let lower = f64::from(*sorted.get(usize::try_from(lo).ok()?)?);
    let upper = f64::from(*sorted.get(usize::try_from((lo + 1).min(last)).ok()?)?);
    let value = (lower + (index - f64::from(lo)) * (upper - lower)) * margin;
    (value.is_finite() && value > 0.0).then_some(value)
}
fn render(report: &StatusReport) -> String {
    let mut text = format!(
        "Daemon: {}; version {}; uptime {}\nPreset: {}\n",
        report.daemon.state,
        report::line(&report.daemon.version),
        report.daemon.uptime,
        report.preset
    );
    let _ = writeln!(
        text,
        "Adapters: {}",
        report
            .adapters
            .iter()
            .map(|a| format!(
                "{} {} ({})",
                a.name,
                if a.enabled { "on" } else { "off" },
                a.reason
            ))
            .collect::<Vec<_>>()
            .join("; ")
    );
    for repo in &report.repositories {
        let _ = writeln!(text, "Repository: {}", report::line(&repo.repo_key));
        if let Some(source) = repo.config_source {
            let _ = writeln!(text, "  Configuration: {source}");
        }
        if repo.harnesses.is_empty() {
            text.push_str("  Harnesses: none observed\n");
        }
        for host in &repo.harnesses {
            let _ = writeln!(
                text,
                "  Harness {}: {}; {} ({}); last event {}; missing {}; {}",
                host.host,
                report::line(&host.level),
                if host.enabled { "on" } else { "off" },
                host.reason,
                host.last_event,
                host.events_missing.join(", "),
                host.coverage
            );
        }
        let f = &repo.facts;
        let _ = writeln!(
            text,
            "  Facts: {} active, {} candidates, {} retired in last 7 days",
            f.active, f.candidates, f.retired_last_7_days
        );
        let l = &repo.loop_guard;
        let envelopes = l
            .envelopes
            .iter()
            .map(|e| {
                format!(
                    "{} {} (n={})",
                    e.host,
                    e.calls
                        .map_or_else(|| "unknown".into(), |v| format!("{v:.1}")),
                    e.samples
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        let _ = writeln!(
            text,
            "  Loop guard: {}; envelope {}; {} redirects, {} escalations in last 7 days",
            l.mode,
            if envelopes.is_empty() {
                "unknown"
            } else {
                &envelopes
            },
            l.redirects_last_7_days,
            l.escalations_last_7_days
        );
        let _ = writeln!(
            text,
            "  Pruning: {}; {} pending proposals",
            repo.pruning.mode, repo.pruning.pending_proposals
        );
        let _ = writeln!(
            text,
            "  Spend: ${:.6} decisions today / ${:.6} daily cap ({})",
            repo.spend.decisions_today_usd, repo.spend.daily_cap_usd, repo.spend.scope
        );
        if repo.savings.is_empty() {
            text.push_str("  Savings: no priced sessions (baseline 0, treatment 0)\n");
        }
        for savings in &repo.savings {
            if let Some(estimate) = &savings.estimate {
                let _ = writeln!(
                    text,
                    "  Savings {}: {} (baseline {}, treatment {}); 95% interval {}",
                    savings.host,
                    estimate
                        .difference
                        .map_or_else(|| "unknown".into(), |n| format!("${n:.6}")),
                    estimate.baseline_count,
                    estimate.treatment_count,
                    estimate.band.map_or_else(
                        || "unknown".into(),
                        |b| format!("${:.6} to ${:.6}", b.lower, b.upper)
                    )
                );
            } else {
                let _ = writeln!(
                    text,
                    "  Savings {}: unavailable (invalid cost samples)",
                    savings.host
                );
            }
        }
        if repo.last_actions.is_empty() {
            text.push_str("  Last actions: none\n");
        }
        for action in &repo.last_actions {
            let _ = writeln!(
                text,
                "  Event {} ({}): {}",
                action.event_id, action.time, action.explanation
            );
        }
    }
    text
}
