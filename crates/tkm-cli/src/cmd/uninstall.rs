//! Manifest-driven removal with host filtering and preservation of foreign entries.

use std::fs;
use std::process::ExitCode;

use tkm_hosts::fold_only::{instructions, skill};
use tkm_store::InstallRecord;
use tkmd::registry::AdapterRegistry;

use super::init::{state, system};
use crate::{cli::UninstallArgs, ctx::Ctx, daemon, error::CliError};

/// Execute `tkm uninstall`.
///
/// # Errors
/// Returns database or output errors. Per-home removal errors are reported and
/// retain their manifest for a later retry while other homes continue.
#[allow(clippy::too_many_lines)] // Keep host reversal and final shared-service recovery together.
pub fn run(ctx: &mut Ctx<'_>, args: &UninstallArgs) -> Result<ExitCode, CliError> {
    if !ctx.paths.db.try_exists()? {
        super::init::reversal::restore(ctx, args.dry_run)?;
        writeln!(ctx.stdout, "No recorded installations.")?;
        return Ok(ExitCode::SUCCESS);
    }
    let records = if args.dry_run {
        let snapshot = state::snapshot_database(&ctx.paths.db)?;
        state::records(&snapshot.connection)?
    } else {
        let connection = daemon::open_db(&ctx.paths)?;
        state::records(&connection)?
    };
    let selected: Vec<_> = records
        .iter()
        .filter(|record| args.host.is_none_or(|host| host == record.host))
        .collect();
    let registry = AdapterRegistry::standard();
    let store = if args.dry_run {
        None
    } else {
        Some(daemon::open_db_for_write(&ctx.paths)?)
    };
    let mut failed = false;
    let mut remaining = records.clone();
    let mut last_removed = None;
    for record in &selected {
        match remove(ctx, &registry, record, &remaining, args.dry_run) {
            Ok(()) => {
                if let Some(store) = &store {
                    store
                        .delete_install(record.host, &record.agent_home)
                        .map_err(state::failure)?;
                }
                remaining.retain(|other| {
                    other.host != record.host || other.agent_home != record.agent_home
                });
                last_removed = Some((**record).clone());
                writeln!(
                    ctx.stdout,
                    "ok {} uninstall: {}{}",
                    record.host.as_str(),
                    record.agent_home.display(),
                    if args.dry_run { " (dry-run)" } else { "" }
                )?;
            }
            Err(error) => {
                writeln!(
                    ctx.stderr,
                    "fail {} uninstall: {error}",
                    record.host.as_str()
                )?;
                failed = true;
            }
        }
    }
    if remaining.is_empty() && !failed {
        let mut services = Vec::new();
        for record in &records {
            if let Some(service) = state::metadata(record)?.service
                && !services
                    .iter()
                    .any(|existing: &state::ServiceRecord| existing.path == service.path)
            {
                services.push(service);
            }
        }
        for service in services {
            writeln!(ctx.stdout, "service removal: {}", service.path.display())?;
            if args.dry_run || !service.owned {
                continue;
            }
            match remove_service(ctx, &service) {
                Ok(()) => {}
                Err(error) => {
                    writeln!(ctx.stderr, "fail service uninstall: {error}")?;
                    // Preserve a recovery anchor when service removal fails after host reversal.
                    if let (Some(store), Some(record)) = (&store, &last_removed) {
                        store.upsert_install(record).map_err(state::failure)?;
                    }
                    failed = true;
                }
            }
        }
    }
    if remaining.is_empty() && !failed {
        super::init::reversal::restore(ctx, args.dry_run)?;
    }
    Ok(if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

fn remove(
    ctx: &mut Ctx<'_>,
    registry: &AdapterRegistry,
    record: &InstallRecord,
    all: &[InstallRecord],
    dry_run: bool,
) -> Result<(), CliError> {
    let metadata = state::metadata(record)?;
    // Verify every adapter target before changing any part of this home. An edited shim
    // prevents removal of its skill and retains the complete record for recovery.
    if metadata.adapter {
        let report = registry
            .get(record.host)
            .uninstall(&state::context(&record.agent_home, true))
            .map_err(state::adapter_error)?;
        for path in report.changed_files {
            writeln!(ctx.stdout, "  remove: {}", path.display())?;
        }
    }
    if metadata.skill {
        skill::uninstall(
            record.host,
            &record.agent_home,
            None,
            skill::SkillOptions {
                dry_run: true,
                ..skill::SkillOptions::default()
            },
        )
        .map_err(state::adapter_error)?;
    }
    let mut repos = Vec::new();
    for repo in &metadata.instructions_repos {
        let shared = all
            .iter()
            .filter(|other| other.host != record.host || other.agent_home != record.agent_home)
            .any(|other| {
                state::metadata(other)
                    .is_ok_and(|metadata| metadata.instructions_repos.contains(repo))
            });
        if shared {
            continue;
        }
        instructions::remove_from(
            repo,
            record.host,
            instructions::ApplyOptions {
                allow_tracked: false,
                dry_run: true,
            },
        )
        .map_err(state::adapter_error)?;
        repos.push(repo);
    }
    if dry_run {
        return Ok(());
    }
    if metadata.adapter {
        registry
            .get(record.host)
            .uninstall(&state::context(&record.agent_home, false))
            .map_err(state::adapter_error)?;
    }
    if metadata.skill {
        skill::uninstall(
            record.host,
            &record.agent_home,
            None,
            skill::SkillOptions::default(),
        )
        .map_err(state::adapter_error)?;
    }
    for repo in repos {
        instructions::remove_from(repo, record.host, instructions::ApplyOptions::default())
            .map_err(state::adapter_error)?;
    }
    state::remove_empty_directories(&metadata.directories)?;
    Ok(())
}

fn remove_service(ctx: &mut Ctx<'_>, service: &state::ServiceRecord) -> Result<(), CliError> {
    tkm_hosts::writer::safe_target(&service.root, &service.path).map_err(state::adapter_error)?;
    if fs::symlink_metadata(&service.path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(state::failure("service path is a symlink"));
    }
    let exists = match fs::read_to_string(&service.path) {
        Ok(content) if content == service.content => true,
        Ok(_) => {
            return Err(state::failure(
                "service file changed; refusing to remove it",
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(error.into()),
    };
    system::stop(ctx, service)?;
    if exists {
        fs::remove_file(&service.path)?;
    }
    state::remove_empty_directories(&service.directories)
}
