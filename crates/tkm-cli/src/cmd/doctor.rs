//! Read-only installation diagnostics with bounded harness version checks.

mod models;

use std::process::ExitCode;

use tkm_config::config::EffectiveConfig;
use tkm_decide::key::KeyError;
use tkm_hosts::fold_only::{instructions, skill};
use tkm_hosts::shim::{ShimStatus, verify_shim};
use tkm_store::InstallRecord;
use tkmd::registry::AdapterRegistry;

use super::init::{key, state, system};
use crate::daemon::{self, DaemonState};
use crate::{cli::DoctorArgs, ctx::Ctx, error::CliError};

/// Execute `tkm doctor`.
///
/// # Errors
/// Returns output errors. Failed checks produce report lines and exit 1;
/// checks never start a daemon. With a key, explicit model probes record their
/// attempts in the database; other checks use read-only database access.
pub fn run(ctx: &mut Ctx<'_>, args: &DoctorArgs) -> Result<ExitCode, CliError> {
    let mut failed = false;
    match daemon::probe(&ctx.paths) {
        DaemonState::Running { version, .. } => line(
            ctx,
            "ok",
            "daemon",
            &format!("running (protocol {version})"),
        )?,
        DaemonState::Down { reason } => {
            line(ctx, "fail", "daemon", &reason)?;
            failed = true;
        }
    }
    line(
        ctx,
        if ctx.paths.socket.exists() {
            "ok"
        } else {
            "warn"
        },
        "socket",
        &format!(
            "{}; monitor {}",
            ctx.paths.socket.display(),
            ctx.paths
                .socket
                .with_file_name(tkm_proto::monitor::MONITOR_SOCKET_NAME)
                .display()
        ),
    )?;
    let connection = match daemon::open_db(&ctx.paths) {
        Ok(connection) => {
            line(ctx, "ok", "database", &ctx.paths.db.display().to_string())?;
            Some(connection)
        }
        Err(error) => {
            line(ctx, "fail", "database", &error.to_string())?;
            failed = true;
            None
        }
    };
    if let Some(connection) = &connection {
        match state::records(connection) {
            Ok(records) => {
                let registry = AdapterRegistry::standard();
                for record in &records {
                    match installed(ctx, &registry, connection, record) {
                        Ok(healthy) => failed |= !healthy,
                        Err(error) => {
                            line(
                                ctx,
                                "fail",
                                &format!("{} installation", record.host.as_str()),
                                &error.to_string(),
                            )?;
                            failed = true;
                        }
                    }
                }
            }
            Err(error) => {
                line(ctx, "fail", "installs", &error.to_string())?;
                failed = true;
            }
        }
    }
    match ctx.load_config() {
        Ok(config) => {
            failed |= !check_key(ctx, &config, args.probe)?;
        }
        Err(error) => {
            line(ctx, "fail", "configuration", &error.to_string())?;
            failed = true;
        }
    }
    Ok(if failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

fn line(ctx: &mut Ctx<'_>, level: &str, check: &str, detail: &str) -> Result<(), CliError> {
    writeln!(
        ctx.stdout,
        "{level} {check}: {}",
        detail.replace(['\r', '\n'], " ")
    )?;
    Ok(())
}

#[allow(clippy::too_many_lines)] // Each independent per-home check must continue after failures.
fn installed(
    ctx: &mut Ctx<'_>,
    registry: &AdapterRegistry,
    connection: &rusqlite::Connection,
    record: &InstallRecord,
) -> Result<bool, CliError> {
    let metadata = state::metadata(record)?;
    let host = record.host.as_str();
    let mut healthy = true;
    if metadata.mode == "fold-only" {
        line(
            ctx,
            "ok",
            &format!("{host} hooks"),
            "fold-only (live hooks disabled)",
        )?;
        for repo in &metadata.instructions_repos {
            if let Some(file) = instructions::instructions_file(record.host) {
                let text = std::fs::read_to_string(repo.join(file)).unwrap_or_default();
                let present =
                    instructions::validate(&text).is_ok() && instructions::remove(&text) != text;
                line(
                    ctx,
                    if present { "ok" } else { "fail" },
                    &format!("{host} instructions"),
                    &repo.display().to_string(),
                )?;
                healthy &= present;
            }
        }
    } else {
        match registry
            .get(record.host)
            .install(&state::context(&record.agent_home, true))
        {
            Ok(report) => {
                let present = report.changed_files.is_empty();
                line(
                    ctx,
                    if present { "ok" } else { "fail" },
                    &format!("{host} hooks"),
                    if present {
                        "present"
                    } else {
                        "missing or outdated entries; run tkm init"
                    },
                )?;
                healthy &= present;
            }
            Err(error) => {
                line(
                    ctx,
                    "fail",
                    &format!("{host} hooks"),
                    &state::adapter_error(error).to_string(),
                )?;
                healthy = false;
            }
        }
    }
    if metadata.skill
        && let Some(path) = skill::skill_path(record.host, &record.agent_home, None)
    {
        let present =
            std::fs::read_to_string(&path).is_ok_and(|text| text == skill::render(record.host));
        line(
            ctx,
            if present { "ok" } else { "fail" },
            &format!("{host} skill"),
            &path.display().to_string(),
        )?;
        healthy &= present;
    }
    match state::binaries(record.host)
        .iter()
        .find_map(|name| state::executable(ctx, name))
    {
        Some(program) => match system::version(ctx, &program) {
            Ok(version) => line(ctx, "ok", &format!("{host} version"), &version)?,
            Err(error) => {
                line(ctx, "fail", &format!("{host} version"), &error.to_string())?;
                healthy = false;
            }
        },
        None => line(
            ctx,
            "warn",
            &format!("{host} version"),
            "harness binary not found on PATH",
        )?,
    }
    for (path, recorded_hash) in &record.shim_hashes {
        let bytes = std::fs::read(path).unwrap_or_default();
        let hash_matches = bytes
            .split(|byte| *byte == b'\n')
            .nth(1)
            .and_then(|line| std::str::from_utf8(line).ok())
            .and_then(|line| line.strip_prefix("// tokenimum-sha256: "))
            == Some(recorded_hash.as_str());
        let status = verify_shim(&bytes);
        let ok = status == ShimStatus::Ok && hash_matches;
        line(
            ctx,
            if ok { "ok" } else { "fail" },
            &format!("{host} shim"),
            &format!(
                "{path}: {status:?}{}",
                if hash_matches {
                    ""
                } else {
                    " (recorded hash differs or file missing)"
                }
            ),
        )?;
        healthy &= ok;
    }
    events(ctx, connection, record)?;
    Ok(healthy)
}

fn events(
    ctx: &mut Ctx<'_>,
    connection: &rusqlite::Connection,
    record: &InstallRecord,
) -> Result<(), CliError> {
    let mut statement = connection.prepare("SELECT DISTINCT COALESCE(json_extract(e.inputs, '$.native'), json_extract(e.inputs, '$.event')) AS observed FROM events e JOIN sessions s ON s.id=e.session_id WHERE e.kind='hook.seen' AND s.host=?1 AND s.agent_home=?2 AND observed IS NOT NULL").map_err(state::failure)?;
    let seen = statement
        .query_map(
            rusqlite::params![record.host.as_str(), record.agent_home.to_string_lossy()],
            |row| row.get::<_, String>(0),
        )
        .map_err(state::failure)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(state::failure)?;
    let expected = tkm_hosts::probe::expected_events(record.host);
    let missing: Vec<_> = expected
        .iter()
        .filter(|event| {
            !seen
                .iter()
                .any(|kind| kind == event.native || kind == event.kind.as_str())
        })
        .map(|event| {
            format!(
                "{}{}",
                event.native,
                if event.required { "" } else { " (optional)" }
            )
        })
        .collect();
    line(
        ctx,
        if missing.is_empty() { "ok" } else { "warn" },
        &format!("{} events", record.host.as_str()),
        &format!(
            "seen [{}]; not observed [{}]",
            seen.join(", "),
            missing.join(", ")
        ),
    )
}

fn check_key(ctx: &mut Ctx<'_>, config: &EffectiveConfig, probe: bool) -> Result<bool, CliError> {
    match key::load(ctx, config) {
        Ok(key) => {
            line(ctx, "ok", "key", "present (value hidden)")?;
            if probe {
                if config.get_bool("decide.enabled") == Some(false) {
                    line(ctx, "warn", "model probes", "skipped (offline)")?;
                } else {
                    return models::probe(ctx, config, key);
                }
            }
            Ok(true)
        }
        Err(KeyError::Missing) => {
            line(ctx, "ok", "key", "offline (no key)")?;
            if probe {
                line(ctx, "warn", "model probes", "skipped (offline)")?;
            }
            Ok(true)
        }
        Err(error) => {
            line(ctx, "fail", "key", &error.to_string())?;
            Ok(false)
        }
    }
}
