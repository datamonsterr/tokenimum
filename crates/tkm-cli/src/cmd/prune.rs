//! Tool deny-list management command entry point.
use crate::{
    cli::{PruneArgs, PruneCommand},
    ctx::Ctx,
    daemon,
    error::CliError,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    process::ExitCode,
};
use tkm_core::{
    model::{Host, Timestamp},
    prune::{self, PruneConfig, PruneMode, ToolEvidence},
};
use tkm_hosts::writer::deny::update_host_deny;

/// Show, apply, or restore deny-list proposals.
///
/// # Errors
/// Returns an error when evidence, settings, or output cannot be accessed.
pub fn run(ctx: &mut Ctx<'_>, args: &PruneArgs) -> Result<ExitCode, CliError> {
    run_with_clock(ctx, args, &tkmd::clock::SystemClock)
}

/// Show or mutate pruning settings using an injected clock.
///
/// # Errors
/// Returns an error for invalid evidence, unsafe settings, or failed I/O.
pub fn run_with_clock(
    ctx: &mut Ctx<'_>,
    args: &PruneArgs,
    clock: &dyn tkmd::clock::Clock,
) -> Result<ExitCode, CliError> {
    let current = Timestamp(
        i64::try_from(clock.now_ms()).map_err(|_| CliError::Usage("clock out of range".into()))?,
    );
    if args.command.is_some() {
        daemon::open_db_for_write(&ctx.paths)?;
    }
    let db = daemon::open_db(&ctx.paths)?;
    let config = ctx.load_config()?;
    let mut prune = tkmd::settings::Settings::from_config(&config).prune;
    let probe = tkm_integrations::detect::Probe::new(
        super::init::state::env(ctx, "PATH")
            .unwrap_or_default()
            .into(),
        super::init::state::home(ctx)?,
        ctx.cwd.clone(),
        ctx.env.iter().cloned().collect(),
    );
    let mode = config
        .get_str("integrations.superorchestrator")
        .and_then(|value| value.parse().ok())
        .unwrap_or_default();
    prune.superorchestrator_adapter_enabled = probe.detect("superorchestrator", mode).state
        == tkm_integrations::detect::AdapterState::Enabled;
    writeln!(ctx.stdout, "Readiness is based on unused counts only.")?;
    let mut statement = db.prepare("SELECT host, tool, prompts_unused, sessions_unused, last_used FROM tool_evidence WHERE repo_key = ?1 ORDER BY host, tool").map_err(db_error)?;
    let rows = statement
        .query_map([&ctx.repo.key.0], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, u32>(2)?,
                r.get::<_, u32>(3)?,
                r.get::<_, Option<i64>>(4)?,
            ))
        })
        .map_err(db_error)?;
    let mut candidates: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for row in rows {
        let (host, tool, prompts, sessions, last) = row.map_err(db_error)?;
        let Some(kind) = host_kind(&host) else {
            continue;
        };
        if prune::is_protected(&tool, kind, &prune) {
            continue;
        }
        let evidence = ToolEvidence {
            prompts: u64::from(prompts),
            sessions: u64::from(sessions),
            ..ToolEvidence::default()
        };
        if prune::eligible(&evidence, &prune) {
            writeln!(
                ctx.stdout,
                "{host}: {tool}: unused in {prompts} prompts / {sessions} sessions; last used {}",
                last.map_or_else(|| "never".into(), |v| v.to_string())
            )?;
            candidates.entry(host).or_default().insert(tool);
        }
    }
    drop(statement);
    match &args.command {
        None => {}
        Some(PruneCommand::Apply(args)) => {
            if let Some(tool) = &args.tool {
                for tools in candidates.values_mut() {
                    tools.retain(|name| name == tool);
                }
            }
            apply(ctx, &db, &candidates, &prune, current)?;
        }
        Some(PruneCommand::Restore(args)) => restore(ctx, args.tool.as_deref())?,
    }
    Ok(ExitCode::SUCCESS)
}
fn host_kind(name: &str) -> Option<Host> {
    match name {
        "claude" => Some(Host::Claude),
        "codex" => Some(Host::Codex),
        "agy" => Some(Host::Agy),
        "cursor" => Some(Host::Cursor),
        "kimi" => Some(Host::Kimi),
        "deepseek" => Some(Host::Deepseek),
        "opencode" => Some(Host::Opencode),
        "pi" => Some(Host::Pi),
        _ => None,
    }
}
fn db_error(e: rusqlite::Error) -> CliError {
    CliError::Usage(format!("database query failed: {e}"))
}
fn store_error(e: tkm_store::StoreError) -> CliError {
    CliError::Usage(format!("database operation failed: {e}"))
}
fn apply(
    ctx: &mut Ctx<'_>,
    db: &rusqlite::Connection,
    candidates: &BTreeMap<String, BTreeSet<String>>,
    config: &PruneConfig,
    current: Timestamp,
) -> Result<(), CliError> {
    let repo: Option<i64> = db
        .query_row(
            "SELECT MAX(activity) FROM (SELECT last_seen AS activity FROM repos WHERE repo_key = ?1 UNION ALL SELECT started_at FROM sessions WHERE repo_key = ?1 UNION ALL SELECT ended_at FROM sessions WHERE repo_key = ?1 UNION ALL SELECT e.created_at FROM events e JOIN sessions s ON s.id = e.session_id WHERE s.repo_key = ?1)",
            [&ctx.repo.key.0],
            |row| row.get(0),
        )
        .map_err(db_error)?;
    let Some(last_seen) = repo else {
        return Err(CliError::Usage(
            "repository activity is unknown; cannot apply pruning".into(),
        ));
    };
    let store = daemon::open_db_for_write(&ctx.paths)?;
    let last_delegated = store
        .last_delegated_run_within(&ctx.repo.key, current)
        .map_err(store_error)?;
    let requested = PruneConfig {
        mode: PruneMode::Apply,
        ..config.clone()
    };
    if prune::effective_mode(Host::Claude, &requested, last_delegated, current)
        == PruneMode::Propose
    {
        writeln!(
            ctx.stdout,
            "proposal only: delegated run in the last 30 days; no deny settings written"
        )?;
        return Ok(());
    }
    let rows = store.list_denylist(&ctx.repo.root).map_err(store_error)?;
    for (host, desired) in candidates {
        let Some(kind @ (Host::Claude | Host::Cursor | Host::Opencode | Host::Pi)) =
            host_kind(host)
        else {
            writeln!(
                ctx.stdout,
                "{host}: proposal only (no verified deny target)"
            )?;
            continue;
        };
        store
            .refresh_prune_readiness(&ctx.repo.key, kind, config, current)
            .map_err(store_error)?;
        let mut ready = BTreeSet::new();
        for tool in desired {
            let Some(since) = store
                .prune_ready_since(&ctx.repo.key, kind, tool)
                .map_err(store_error)?
            else {
                continue;
            };
            if prune::write_allowed(Timestamp(last_seen), since, current, config) {
                ready.insert(tool.clone());
            }
        }
        if ready.is_empty() && !desired.is_empty() {
            return Err(CliError::Usage("repository has not been idle long enough to apply pruning; candidate deadline not reached".into()));
        }
        let owned: BTreeSet<_> = rows
            .iter()
            .filter(|e| e.host == kind && !e.restored)
            .map(|e| e.tool.clone())
            .collect();
        let requested = owned.union(&ready).cloned().collect();
        let report = update_host_deny(&ctx.repo.root, kind, &owned, &requested, false)
            .map_err(|e| CliError::Usage(format!("cannot update deny list: {e}")))?;
        for tool in report.owned.difference(&owned) {
            store
                .record_denylist_write(&tkm_store::DenylistEntry {
                    checkout_path: ctx.repo.root.clone(),
                    host: kind,
                    tool: tool.clone(),
                    written_at: current,
                    restored: false,
                })
                .map_err(store_error)?;
        }
    }
    Ok(())
}
fn restore(ctx: &mut Ctx<'_>, tool: Option<&str>) -> Result<(), CliError> {
    let store = daemon::open_db_for_write(&ctx.paths)?;
    let rows = store.list_denylist(&ctx.repo.root).map_err(store_error)?;
    for kind in [Host::Claude, Host::Cursor, Host::Opencode, Host::Pi] {
        let owned: BTreeSet<_> = rows
            .iter()
            .filter(|e| e.host == kind && !e.restored)
            .map(|e| e.tool.clone())
            .collect();
        let desired = owned
            .iter()
            .filter(|name| tool.is_some_and(|tool| tool != name.as_str()))
            .cloned()
            .collect();
        if owned.is_empty() {
            continue;
        }
        let report = update_host_deny(&ctx.repo.root, kind, &owned, &desired, false)
            .map_err(|e| CliError::Usage(format!("cannot restore deny list: {e}")))?;
        for name in owned.difference(&report.owned) {
            store
                .mark_denylist_restored(&ctx.repo.root, kind, name)
                .map_err(store_error)?;
            writeln!(ctx.stdout, "{}: restored {name}", kind.as_str())?;
        }
    }
    Ok(())
}
