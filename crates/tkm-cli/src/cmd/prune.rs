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
    time::{SystemTime, UNIX_EPOCH},
};
use tkm_core::{
    model::{Host, Timestamp},
    prune::{self, PruneConfig, ToolEvidence},
};
use tkm_hosts::{HostAdapter, claude::ClaudeAdapter, writer::deny::update_deny};

/// Show, apply, or restore deny-list proposals.
///
/// # Errors
/// Returns an error when evidence, settings, or output cannot be accessed.
pub fn run(ctx: &mut Ctx<'_>, args: &PruneArgs) -> Result<ExitCode, CliError> {
    let db = daemon::open_db(&ctx.paths)?;
    let config = ctx.load_config()?;
    let mut prune = tkmd::settings::Settings::from_config(&config).prune;
    prune.superorchestrator_adapter_enabled = true;
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
        Some(PruneCommand::Apply(_)) => apply(ctx, &db, &candidates, &prune)?,
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
fn now() -> Result<Timestamp, CliError> {
    let ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| CliError::Usage(format!("clock error: {e}")))?
        .as_millis();
    Ok(Timestamp(i64::try_from(ms).map_err(|_| {
        CliError::Usage("clock out of range".into())
    })?))
}
fn apply(
    ctx: &mut Ctx<'_>,
    db: &rusqlite::Connection,
    candidates: &BTreeMap<String, BTreeSet<String>>,
    config: &PruneConfig,
) -> Result<(), CliError> {
    use rusqlite::OptionalExtension;
    let repo: Option<i64> = db
        .query_row(
            "SELECT last_seen FROM repos WHERE repo_key = ?1",
            [&ctx.repo.key.0],
            |row| row.get(0),
        )
        .optional()
        .map_err(db_error)?;
    let current = now()?;
    let Some(last_seen) = repo else {
        return Err(CliError::Usage(
            "repository activity is unknown; cannot apply pruning".into(),
        ));
    };
    if i128::from(current.0) - i128::from(last_seen) < i128::from(config.quiet_minutes) * 60_000 {
        return Err(CliError::Usage(
            "repository has not been idle long enough to apply pruning".into(),
        ));
    }
    let store = daemon::open_db_for_write(&ctx.paths)?;
    let rows = store.list_denylist(&ctx.repo.root).map_err(store_error)?;
    for (host, desired) in candidates {
        if host != "claude" {
            writeln!(
                ctx.stdout,
                "{host}: apply not supported yet; proposal only (no file written)"
            )?;
            continue;
        }
        if !matches!(
            ClaudeAdapter::new().prune_target(),
            tkm_hosts::PruneTarget::SettingsDenyList { .. }
        ) {
            writeln!(
                ctx.stdout,
                "{host}: apply not supported yet; proposal only (no file written)"
            )?;
            continue;
        }
        let owned: BTreeSet<_> = rows
            .iter()
            .filter(|e| e.host == Host::Claude && !e.restored)
            .map(|e| e.tool.clone())
            .collect();
        let requested = owned.union(desired).cloned().collect();
        let report = update_deny(&ctx.repo.root, &owned, &requested, false)
            .map_err(|e| CliError::Usage(format!("cannot update deny list: {e}")))?;
        for tool in report.owned.difference(&owned) {
            store
                .record_denylist_write(&tkm_store::DenylistEntry {
                    checkout_path: ctx.repo.root.clone(),
                    host: Host::Claude,
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
    let owned: BTreeSet<_> = rows
        .iter()
        .filter(|e| e.host == Host::Claude && !e.restored)
        .map(|e| e.tool.clone())
        .collect();
    let desired: BTreeSet<_> = owned
        .iter()
        .filter(|name| tool.is_some_and(|tool| tool != name.as_str()))
        .cloned()
        .collect();
    if !owned.is_empty() {
        let report = update_deny(&ctx.repo.root, &owned, &desired, false)
            .map_err(|e| CliError::Usage(format!("cannot restore deny list: {e}")))?;
        for name in owned.difference(&report.owned) {
            store
                .mark_denylist_restored(&ctx.repo.root, Host::Claude, name)
                .map_err(store_error)?;
            writeln!(ctx.stdout, "restored {name}")?;
        }
    }
    for row in rows
        .iter()
        .filter(|e| e.host != Host::Claude && !e.restored && tool.is_none_or(|t| t == e.tool))
    {
        writeln!(
            ctx.stdout,
            "{}: restore not supported yet; no file written",
            row.host.as_str()
        )?;
    }
    Ok(())
}
