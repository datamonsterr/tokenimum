//! Skill evidence reconstructed from recorded sessions, without daemon or store writes.

use std::{
    path::{Component, PathBuf},
    process::ExitCode,
};
use tkm_core::prune::skills::{SkillReport, fold_skills};
use tkm_hosts::{HostAdapter, SessionSource, claude::ClaudeAdapter};
use tkm_integrations::otherskill::{SkillEvidence, ranking_prior};
use tkm_store::SessionRecord;

use super::status::report;
use crate::{cli::SkillsArgs, ctx::Ctx, error::CliError};

const RECENT_SESSIONS: i64 = 30;
const MAX_LOG_BYTES: u64 = 4 * 1024 * 1024;

/// Print the otherskill ranking prior, or a readable evidence table.
///
/// Evidence covers at most thirty recent sessions in the current repository.
/// Only Claude logs have a derivable stored path today; other hosts, missing logs,
/// partial sessions and logs over 4 MiB are skipped. Loaded counts require a host
/// reader to expose skill catalog entries; invocations use core's `fold_skills`.
///
/// # Errors
/// Returns an error if the database or command output cannot be read or written.
pub fn run(ctx: &mut Ctx<'_>, args: &SkillsArgs) -> Result<ExitCode, CliError> {
    let (evidence, skipped) = evidence(ctx)?;
    if skipped > 0 {
        writeln!(
            ctx.stderr,
            "tkm skills: skipped {skipped} sessions (unsupported host, missing, partial, unsafe or oversized log)"
        )?;
    }
    if args.json {
        let json = ranking_prior(&evidence);
        writeln!(
            ctx.stdout,
            "{}",
            serde_json::to_string(&json).map_err(report::json_error)?
        )?;
    } else {
        writeln!(ctx.stdout, "{:<40} {:>8} {:>8}", "Skill", "Loaded", "Used")?;
        for skill in &evidence {
            writeln!(
                ctx.stdout,
                "{:<40} {:>8} {:>8}",
                report::line(&skill.name),
                skill.loaded,
                skill.used
            )?;
        }
        if evidence.is_empty() {
            writeln!(ctx.stdout, "No recorded skill evidence.")?;
        }
        writeln!(
            ctx.stdout,
            "Evidence: up to 30 recent sessions in this repository; loaded counts require host skill catalogs."
        )?;
    }
    Ok(ExitCode::SUCCESS)
}

fn evidence(ctx: &Ctx<'_>) -> Result<(Vec<SkillEvidence>, usize), CliError> {
    let store = match report::read_store(ctx) {
        Ok(store) => store,
        Err(CliError::MissingDatabase(_)) => return Ok((Vec::new(), 0)),
        Err(error) => return Err(error),
    };
    let sessions = store
        .list_sessions_by_repo(&ctx.repo.key, RECENT_SESSIONS)
        .map_err(|error| report::store_error(ctx, error))?;
    let adapter = ClaudeAdapter::new();
    let mut skills = SkillReport::default();
    let mut skipped = 0;
    for record in sessions {
        let session = source(&record).and_then(|source| adapter.read_session(&source).ok());
        if let Some(session) = session.filter(|session| !session.partial) {
            fold_skills(&mut skills, &session);
        } else {
            skipped += 1;
        }
    }
    let evidence = skills
        .skills
        .into_iter()
        .map(|(name, usage)| SkillEvidence {
            name,
            loaded: usage.loaded_sessions,
            used: usage.invocations,
        })
        .collect();
    Ok((evidence, skipped))
}

fn source(record: &SessionRecord) -> Option<SessionSource> {
    if record.key.host != tkm_proto::Host::Claude || record.ended.is_none() {
        return None;
    }
    // Session ids form one filename, never a relative path into another directory.
    let mut components = std::path::Path::new(&record.key.session_id).components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return None;
    }
    let agent_home = PathBuf::from(&record.key.agent_home);
    let slug: String = record
        .cwd
        .to_string_lossy()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let path = agent_home
        .join("projects")
        .join(slug)
        .join(format!("{}.jsonl", record.key.session_id));
    let metadata = std::fs::symlink_metadata(&path).ok()?;
    if !metadata.is_file()
        || metadata.len() > MAX_LOG_BYTES
        || !path
            .canonicalize()
            .ok()?
            .starts_with(agent_home.canonicalize().ok()?)
    {
        return None;
    }
    Some(SessionSource {
        agent_home,
        path,
        session_id: record.key.session_id.clone(),
        repo_key: record.repo_key.clone(),
        cwd: record.cwd.clone(),
    })
}
