//! Plan-first, fail-open installation of harness integrations and the daemon service.

pub(crate) mod key;
pub(crate) mod reversal;
pub(crate) mod state;
pub(crate) mod system;

use std::path::PathBuf;
use std::process::ExitCode;

use tkm_config::config::{ConfigFile, Layer};
use tkm_config::repo::IdentitySource;
use tkm_hosts::InstallReport;
use tkm_hosts::fold_only::{IntegrationMode, effective_mode, instructions, skill};
use tkm_proto::Host;
use tkm_store::InstallRecord;
use tkmd::registry::AdapterRegistry;

use crate::{
    cli::{DoctorArgs, InitArgs},
    ctx::Ctx,
    daemon,
    error::CliError,
};

const INSTRUCTIONS_BODY: &str = "Read repository facts with `tkm facts --brief`. Inspect their evidence with `tkm why <event>`. Both commands are read-only.";

struct Target {
    host: Host,
    home: PathBuf,
    mode: IntegrationMode,
    report: InstallReport,
    repo: Option<PathBuf>,
    adapter_changed: bool,
    account: bool,
}

/// Execute `tkm init`.
///
/// # Errors
/// Returns usage, configuration, database, or I/O errors. Individual host failures
/// are reported and do not prevent installation of the other selected homes.
#[allow(clippy::too_many_lines)] // Keep the plan/approval/apply sequence visible at one entry point.
pub fn run(ctx: &mut Ctx<'_>, args: &InitArgs) -> Result<ExitCode, CliError> {
    let mut homes: Vec<_> = select(ctx, args)?
        .into_iter()
        .map(|(host, home)| (host, home, false))
        .collect();
    let accounts = super::init_integrations::account_homes(ctx, args)?;
    for account in &accounts {
        if let Some(host) = account.host
            && !homes
                .iter()
                .any(|(existing, home, _)| *existing == host && *home == account.home)
        {
            homes.push((host, account.home.clone(), true));
        }
    }
    let config = ctx.load_config()?;
    let mut global = ConfigFile::load(ctx.paths.global_config.clone(), Layer::Global)?;
    if let Some(preset) = &args.preset {
        if !["observe", "balanced", "aggressive", "offline"].contains(&preset.as_str()) {
            return Err(CliError::Usage(format!("unknown preset {preset}")));
        }
        global.set("preset", preset)?;
    }
    let choice = key::automatic(ctx, &config)?;
    let registry = AdapterRegistry::standard();
    let mut failed = false;
    let mut targets = Vec::new();
    writeln!(ctx.stdout, "Installation plan:")?;
    for (host, home, account) in homes {
        match target(ctx, &registry, host, home, args.no_shims) {
            Ok(mut target) => {
                target.account = account;
                writeln!(
                    ctx.stdout,
                    "{}: {} ({})",
                    host.as_str(),
                    mode_name(target.mode),
                    target.home.display()
                )?;
                for path in &target.report.changed_files {
                    writeln!(ctx.stdout, "  file: {}", path.display())?;
                }
                for instruction in &target.report.instructions {
                    writeln!(ctx.stdout, "  {instruction}")?;
                }
                let markers = tkm_hosts::env_markers(host);
                if !markers.is_empty() {
                    writeln!(ctx.stdout, "  environment markers: {}", markers.join(", "))?;
                }
                targets.push(target);
            }
            Err(error) => {
                writeln!(ctx.stderr, "fail {} install: {error}", host.as_str())?;
                failed = true;
            }
        }
    }
    let integrations = super::init_integrations::detect(ctx)?;
    writeln!(
        ctx.stdout,
        "{}\n{}",
        integrations.superorchestrator, integrations.otherskill
    )?;
    let service = system::plan(ctx)?;
    writeln!(ctx.stdout, "service: {}", service.path.display())?;
    writeln!(ctx.stdout, "key: {}", key::describe(&choice))?;
    writeln!(
        ctx.stdout,
        "preset: {}",
        args.preset
            .as_deref()
            .or_else(|| config.get_str("preset"))
            .unwrap_or("balanced")
    )?;
    if args.dry_run {
        return Ok(exit(failed));
    }
    if !args.yes && !key::stdin_is_terminal() {
        return Err(CliError::Usage("re-run with --yes to apply".into()));
    }
    if targets.is_empty() {
        writeln!(ctx.stdout, "No harnesses selected; nothing to install.")?;
        return Ok(exit(failed));
    }
    let choice = if args.yes {
        choice
    } else {
        key::interactive(ctx, choice)?
    };
    let key_reuse = super::init_integrations::consent(ctx, args.yes)?;
    let store = daemon::open_db_for_write(&ctx.paths)?;
    key::apply(ctx, &choice, &mut global)?;
    if key_reuse {
        global.set("integrations.otherskill_key_reuse", "true")?;
        if matches!(choice, key::Choice::Offline)
            && tkm_integrations::otherskill::read_key(&state::home(ctx)?).is_some()
        {
            global.set("decide.enabled", "true")?;
        }
    }
    if args.preset.as_deref().or_else(|| config.get_str("preset")) == Some("offline") {
        global.set("decide.enabled", "false")?;
    }
    global.save()?;
    let mut installed = Vec::new();
    for target in targets.iter().filter(|target| !target.account) {
        failed |= install_target(ctx, &store, &registry, target, &mut installed).is_err();
    }
    let account_report =
        tkm_integrations::superorchestrator::install_account_homes(&accounts, |host, home| {
            // A normal-home target may already cover an account at the same path.
            let Some(target) = targets
                .iter()
                .find(|target| target.account && target.host == host && target.home == home)
            else {
                return Ok(tkm_integrations::superorchestrator::InstallOutcome::AlreadyPresent);
            };
            install_target(ctx, &store, &registry, target, &mut installed)
        });
    failed |= account_report.failed > 0;
    if !installed.is_empty() {
        // Shared service metadata is copied to every home so --host removal cannot lose it.
        let existing = store.list_installs().map_err(state::failure)?;
        let previous = existing
            .iter()
            .map(state::metadata)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .find_map(|metadata| metadata.service);
        let mut service = previous.unwrap_or(service);
        if system::daemon_executable(ctx).is_none() {
            writeln!(ctx.stderr, "warn daemon: tkmd missing; install it on PATH")?;
        }
        let mut directories = Vec::new();
        for record in &existing {
            state::extend_paths(&mut directories, state::metadata(record)?.directories);
        }
        match system::write_service(&service) {
            Ok(()) => {
                state::canonicalize_directories(&mut service.directories);
                if let Err(error) = system::start(ctx, &mut service) {
                    writeln!(ctx.stderr, "warn service: {error}")?;
                }
                for mut record in existing {
                    let mut metadata = state::metadata(&record)?;
                    metadata.service = Some(service.clone());
                    state::extend_paths(&mut metadata.directories, directories.clone());
                    state::extend_paths(&mut record.files_written, [service.path.clone()]);
                    state::set_metadata(&mut record, &metadata)?;
                    store.upsert_install(&record).map_err(state::failure)?;
                }
            }
            Err(error) => {
                writeln!(ctx.stderr, "fail service: {error}")?;
                failed = true;
            }
        }
        // Fake service managers need not start a live daemon; diagnostics remain visible but
        // do not turn a successfully applied installation into an installation failure.
        let _ = super::doctor::run(ctx, &DoctorArgs { probe: false })?;
        for target in &targets {
            for instruction in &target.report.instructions {
                writeln!(ctx.stdout, "trust {}: {instruction}", target.host.as_str())?;
            }
        }
    }
    Ok(exit(failed))
}

fn exit(failed: bool) -> ExitCode {
    if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

pub(crate) fn mode_name(mode: IntegrationMode) -> &'static str {
    match mode {
        IntegrationMode::Hooks => "hooks",
        IntegrationMode::Shim => "shim",
        IntegrationMode::FoldOnly => "fold-only",
    }
}

fn select(ctx: &Ctx<'_>, args: &InitArgs) -> Result<Vec<(Host, PathBuf)>, CliError> {
    match (&args.home, args.host) {
        (Some(home), Some(host)) => {
            if !args.only.is_empty() && args.only != [host] {
                return Err(CliError::Usage(
                    "--home/--host cannot be combined with a different --only selection".into(),
                ));
            }
            let path = if home.is_absolute() {
                home.clone()
            } else {
                ctx.cwd.join(home)
            };
            return Ok(vec![(host, path)]);
        }
        (Some(_), None) => return Err(CliError::Usage("--home requires --host".into())),
        (None, Some(_)) => return Err(CliError::Usage("--host requires --home".into())),
        (None, None) => {}
    }
    let home = state::home(ctx)?;
    Ok(state::HOSTS
        .into_iter()
        .filter(|host| args.only.is_empty() || args.only.contains(host))
        .filter_map(|host| {
            let path = state::host_home(&home, host);
            (path.is_dir()
                || state::binaries(host)
                    .iter()
                    .any(|name| state::executable(ctx, name).is_some()))
            .then_some((host, path))
        })
        .collect())
}

fn target(
    ctx: &Ctx<'_>,
    registry: &AdapterRegistry,
    host: Host,
    home: PathBuf,
    no_shims: bool,
) -> Result<Target, CliError> {
    let config = ctx.load_config()?;
    let fallback = if ctx.paths.db.exists() {
        let store = tkm_store::Store::from_read_only_connection(daemon::open_db(&ctx.paths)?)
            .map_err(state::failure)?;
        store.host_fold_only(host).map_err(state::failure)?
    } else {
        false
    };
    let mode = effective_mode(
        host,
        config
            .get_str(&format!("hosts.{}.level", host.as_str()))
            .unwrap_or("auto"),
        config
            .get_bool(&format!("hosts.{}.enabled", host.as_str()))
            .unwrap_or(true),
        fallback || (no_shims && matches!(host, Host::Opencode | Host::Pi)),
    );
    let mut report = InstallReport {
        dry_run: true,
        ..InstallReport::default()
    };
    let repo = (mode == IntegrationMode::FoldOnly && ctx.repo.how != IdentitySource::Dir)
        .then(|| ctx.repo.root.clone());
    let mut adapter_changed = false;
    if mode == IntegrationMode::FoldOnly {
        if let Some(repo) = &repo {
            merge(
                &mut report,
                instructions::apply(
                    repo,
                    host,
                    INSTRUCTIONS_BODY,
                    instructions::ApplyOptions {
                        allow_tracked: false,
                        dry_run: true,
                    },
                )
                .map_err(state::adapter_error)?,
            );
        } else {
            report.instructions.push(
                "instructions block: added per repository (run tkm init inside the repository)"
                    .into(),
            );
        }
    } else {
        let adapter_report = registry
            .get(host)
            .install(&state::context(&home, true))
            .map_err(state::adapter_error)?;
        adapter_changed = !adapter_report.changed_files.is_empty();
        merge(&mut report, adapter_report);
    }
    merge(
        &mut report,
        skill::install(
            host,
            &home,
            None,
            skill::SkillOptions {
                dry_run: true,
                ..skill::SkillOptions::default()
            },
        )
        .map_err(state::adapter_error)?,
    );
    Ok(Target {
        host,
        home,
        mode,
        report,
        repo,
        adapter_changed,
        account: false,
    })
}

fn merge(target: &mut InstallReport, report: InstallReport) {
    state::extend_paths(&mut target.changed_files, report.changed_files);
    target.instructions.extend(report.instructions);
}

fn install_target(
    ctx: &mut Ctx<'_>,
    store: &tkm_store::Store,
    registry: &AdapterRegistry,
    target: &Target,
    installed: &mut Vec<InstallRecord>,
) -> Result<tkm_integrations::superorchestrator::InstallOutcome, CliError> {
    match apply(store, registry, target) {
        Ok(record) => {
            writeln!(
                ctx.stdout,
                "ok {} install: {}",
                target.host.as_str(),
                target.home.display()
            )?;
            installed.push(record);
            Ok(if target.report.changed_files.is_empty() {
                tkm_integrations::superorchestrator::InstallOutcome::AlreadyPresent
            } else {
                tkm_integrations::superorchestrator::InstallOutcome::Installed
            })
        }
        Err(error) => {
            writeln!(ctx.stderr, "fail {} install: {error}", target.host.as_str())?;
            Err(error)
        }
    }
}

#[allow(clippy::too_many_lines)] // Persist recovery metadata before each group of adapter effects.
fn apply(
    store: &tkm_store::Store,
    registry: &AdapterRegistry,
    target: &Target,
) -> Result<InstallRecord, CliError> {
    let mut record = store
        .get_install(target.host, &target.home)
        .map_err(state::failure)?
        .unwrap_or_else(|| state::empty_record(target.host, &target.home));
    let mut metadata = state::metadata(&record)?;
    metadata.mode = mode_name(target.mode).into();
    state::extend_paths(
        &mut metadata.directories,
        state::missing_directories(&target.report.changed_files),
    );
    // Persist intended ownership before the first write, retaining a recoverable record if a
    // later skill/instructions write fails. Planning failures never reach this point.
    metadata.adapter |= target.adapter_changed;
    metadata.skill |= skill::skill_path(target.host, &target.home, None)
        .is_some_and(|path| target.report.changed_files.contains(&path));
    if let Some(repo) = &target.repo {
        state::extend_paths(&mut metadata.instructions_repos, [repo.clone()]);
    }
    state::extend_paths(
        &mut record.files_written,
        target.report.changed_files.clone(),
    );
    if metadata.adapter {
        for event in tkm_hosts::probe::expected_events(target.host) {
            let id = format!("hook:{}:{}", target.host.as_str(), event.native);
            if !record.entry_ids.contains(&id) {
                record.entry_ids.push(id);
            }
        }
    }
    if metadata.skill && !record.entry_ids.iter().any(|id| id == "skill:tokenimum") {
        record.entry_ids.push("skill:tokenimum".into());
    }
    state::set_metadata(&mut record, &metadata)?;
    store.upsert_install(&record).map_err(state::failure)?;
    if target.mode == IntegrationMode::FoldOnly {
        if metadata.adapter {
            registry
                .get(target.host)
                .uninstall(&state::context(&target.home, false))
                .map_err(state::adapter_error)?;
            metadata.adapter = false;
            record.shim_hashes.clear();
            record
                .files_written
                .retain(|path| path.extension().is_none_or(|extension| extension != "ts"));
        }
        if let Some(repo) = &target.repo {
            instructions::apply(
                repo,
                target.host,
                INSTRUCTIONS_BODY,
                instructions::ApplyOptions::default(),
            )
            .map_err(state::adapter_error)?;
        }
    } else {
        registry
            .get(target.host)
            .install(&state::context(&target.home, false))
            .map_err(state::adapter_error)?;
    }
    skill::install(
        target.host,
        &target.home,
        None,
        skill::SkillOptions::default(),
    )
    .map_err(state::adapter_error)?;
    state::canonicalize_directories(&mut metadata.directories);
    for path in &record.files_written {
        if path.as_os_str().to_string_lossy().ends_with(".tkm-bak")
            && !record.backup_paths.contains(path)
        {
            record.backup_paths.push(path.clone());
        }
        if path.extension().is_some_and(|extension| extension == "ts") {
            let bytes = std::fs::read(path)?;
            if tkm_hosts::shim::verify_shim(&bytes) == tkm_hosts::shim::ShimStatus::Ok
                && let Some(hash) = bytes
                    .split(|byte| *byte == b'\n')
                    .nth(1)
                    .and_then(|line| std::str::from_utf8(line).ok())
                    .and_then(|line| line.strip_prefix("// tokenimum-sha256: "))
            {
                record
                    .shim_hashes
                    .insert(path.display().to_string(), hash.into());
            }
        }
    }
    if !record.shim_hashes.is_empty() && !record.entry_ids.iter().any(|id| id == "shim:tokenimum") {
        record.entry_ids.push("shim:tokenimum".into());
    }
    state::set_metadata(&mut record, &metadata)?;
    store.upsert_install(&record).map_err(state::failure)?;
    Ok(record)
}
