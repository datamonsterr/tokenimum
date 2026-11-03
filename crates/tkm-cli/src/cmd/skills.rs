//! Skill evidence reconstructed from recorded sessions, without daemon or store writes.

use std::{
    path::{Component, PathBuf},
    process::ExitCode,
};
use tkm_core::prune::skills::fold_skills;
use tkm_hosts::{HostAdapter, SessionSource, claude::ClaudeAdapter};
use tkm_integrations::otherskill::{SkillEvidence, ranking_prior};
use tkm_store::SessionRecord;

use super::status::report;
use crate::{cli::SkillsArgs, ctx::Ctx, error::CliError};

const RECENT_SESSIONS: i64 = 30;
// Real Claude Code transcripts commonly run to several MiB; the reader also streams with its own
// per-record and total bounds.
const MAX_LOG_BYTES: u64 = 64 * 1024 * 1024;

/// Print the otherskill ranking prior, or a readable evidence table.
///
/// Evidence covers at most thirty recent sessions in the current repository.
/// Only Claude logs have a derivable stored path today; other hosts, open sessions, missing
/// logs, partial sessions and logs over 64 MiB are skipped, each with one stderr line naming the
/// session and the reason. Loaded counts come from the host reader's skill catalog entries;
/// invocations use core's `fold_skills`.
///
/// # Errors
/// Returns an error if the database or command output cannot be read or written.
pub fn run(ctx: &mut Ctx<'_>, args: &SkillsArgs) -> Result<ExitCode, CliError> {
    let (evidence, skipped) = evidence(ctx)?;
    for (session, reason) in &skipped {
        writeln!(
            ctx.stderr,
            "tkm skills: skipped session {}: {reason}",
            report::line(session)
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
            "Evidence: folded sessions plus up to 30 recent legacy sessions; loaded counts require host skill catalogs."
        )?;
    }
    Ok(ExitCode::SUCCESS)
}

type Skipped = Vec<(String, String)>;

fn evidence(ctx: &Ctx<'_>) -> Result<(Vec<SkillEvidence>, Skipped), CliError> {
    let store = match report::read_store(ctx) {
        Ok(store) => store,
        Err(CliError::MissingDatabase(_)) => return Ok((Vec::new(), Vec::new())),
        Err(error) => return Err(error),
    };
    let sessions = store
        .list_sessions_by_repo(&ctx.repo.key, RECENT_SESSIONS)
        .map_err(|error| report::store_error(ctx, error))?;
    let adapter = ClaudeAdapter::new();
    let mut skills = store
        .skill_evidence(&ctx.repo.key)
        .map_err(|error| report::store_error(ctx, error))?;
    let mut skipped = Vec::new();
    for record in sessions {
        if store
            .has_skill_evidence(record.id)
            .map_err(|error| report::store_error(ctx, error))?
        {
            continue;
        }
        let session = source(&record).and_then(|source| {
            adapter
                .read_session(&source)
                .map_err(|error| format!("unreadable log: {error}"))
        });
        match session {
            Ok(session) if session.partial => skipped.push((
                record.key.session_id.clone(),
                format!(
                    "partial log: {} of {} records not recognized",
                    session.skipped_records, session.total_records
                ),
            )),
            Ok(session) => fold_skills(&mut skills, &session),
            Err(reason) => skipped.push((record.key.session_id.clone(), reason)),
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

fn source(record: &SessionRecord) -> Result<SessionSource, String> {
    if record.key.host != tkm_proto::Host::Claude {
        return Err(format!(
            "no session log reader for host {}",
            record.key.host.as_str()
        ));
    }
    if record.ended.is_none() {
        return Err("session has not ended".into());
    }
    // Session ids form one filename, never a relative path into another directory.
    let mut components = std::path::Path::new(&record.key.session_id).components();
    if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
        return Err("unsafe session id".into());
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
    let metadata =
        std::fs::symlink_metadata(&path).map_err(|_| format!("log missing: {}", path.display()))?;
    if !metadata.is_file() {
        return Err(format!("log is not a regular file: {}", path.display()));
    }
    if metadata.len() > MAX_LOG_BYTES {
        return Err(format!(
            "log over 64 MiB ({} MiB)",
            metadata.len() / (1024 * 1024)
        ));
    }
    let inside = path
        .canonicalize()
        .ok()
        .zip(agent_home.canonicalize().ok())
        .is_some_and(|(path, home)| path.starts_with(home));
    if !inside {
        return Err("log outside the agent home".into());
    }
    Ok(SessionSource {
        agent_home,
        path,
        session_id: record.key.session_id.clone(),
        repo_key: record.repo_key.clone(),
        cwd: record.cwd.clone(),
    })
}
