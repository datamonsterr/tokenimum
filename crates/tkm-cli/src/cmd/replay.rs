//! Recorded session replay command entry point.
use crate::{cli::ReplayArgs, ctx::Ctx, daemon, error::CliError};
use std::{
    collections::BTreeSet,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::ExitCode,
};
use tkm_core::{
    facts::FactsState,
    model::{Host, RepoKey},
};
use tkm_hosts::{HostAdapter, SessionSource};

/// Replay a recorded session without writing the database.
///
/// # Errors
/// Returns an error for unknown sources, unsupported formats, or output failures.
pub fn run(ctx: &mut Ctx<'_>, args: &ReplayArgs) -> Result<ExitCode, CliError> {
    if args.bench {
        let root = Path::new(&args.session_or_fixture);
        if args.record {
            super::bench::record(ctx, root)?;
        }
        let report = super::bench::load(root)?;
        write!(ctx.stdout, "{}", report.render())?;
        return Ok(ExitCode::SUCCESS);
    }
    let path = PathBuf::from(&args.session_or_fixture);
    let (host, source) = if path.is_file() {
        fixture_source(ctx, path)?
    } else {
        stored_source(ctx, &args.session_or_fixture)?
    };
    let adapter = adapter(host)?;
    let session = adapter
        .read_session(&source)
        .map_err(|e| CliError::Usage(format!("cannot read session: {e}")))?;
    let config = ctx.load_config()?;
    let settings = tkmd::settings::Settings::from_config(&config);
    let report = tkmd::fold::replay_fold(
        &session,
        FactsState::default(),
        &BTreeSet::new(),
        &settings.facts,
    );
    writeln!(
        ctx.stdout,
        "replay {} session {}",
        host.as_str(),
        session.session_id
    )?;
    if report.events.is_empty() {
        writeln!(ctx.stdout, "no fact actions")?;
    }
    for event in &report.events {
        writeln!(ctx.stdout, "{:?}: {}", event.kind, event.fact_id)?;
    }
    for (tool, evidence) in &report.prune.tools {
        writeln!(
            ctx.stdout,
            "tool {tool}: unused in {} prompts / {} sessions",
            evidence.prompts, evidence.sessions
        )?;
    }
    Ok(ExitCode::SUCCESS)
}
fn fixture_source(ctx: &Ctx<'_>, path: PathBuf) -> Result<(Host, SessionSource), CliError> {
    let file = std::fs::File::open(&path)?;
    let mut host = None;
    let mut id = None;
    for line in BufReader::new(file).lines().take(64) {
        let line = line?;
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&line) else {
            continue;
        };
        if let Some(session) = value.get("sessionId").and_then(serde_json::Value::as_str) {
            host = Some(Host::Claude);
            id = Some(session.to_owned());
            break;
        }
        if value.get("type").and_then(serde_json::Value::as_str) == Some("session_meta") {
            host = Some(Host::Codex);
            id = value
                .pointer("/payload/id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);
            break;
        }
        if value.get("type").and_then(serde_json::Value::as_str) == Some("session")
            && value.get("version").is_some()
        {
            host = Some(Host::Pi);
            id = value
                .get("id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned);
            break;
        }
    }
    let host =
        host.ok_or_else(|| CliError::Usage(format!("cannot infer host from {}", path.display())))?;
    let session_id = id
        .or_else(|| path.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .ok_or_else(|| CliError::Usage("cannot infer session identifier".into()))?;
    Ok((
        host,
        SessionSource {
            agent_home: ctx.cwd.clone(),
            path,
            session_id,
            repo_key: ctx.repo.key.clone(),
            cwd: ctx.cwd.clone(),
        },
    ))
}
fn stored_source(ctx: &Ctx<'_>, id: &str) -> Result<(Host, SessionSource), CliError> {
    let db = daemon::open_db(&ctx.paths)?;
    let mut stmt = db.prepare("SELECT host, agent_home, session_id, repo_key, cwd FROM sessions WHERE session_id = ?1 OR CAST(id AS TEXT) = ?1").map_err(db_error)?;
    let rows = stmt
        .query_map([id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
                r.get::<_, String>(4)?,
            ))
        })
        .map_err(db_error)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(db_error)?;
    let [(host, home, session_id, repo, cwd)] = rows.as_slice() else {
        return Err(CliError::Usage(format!(
            "session {id} not found or ambiguous"
        )));
    };
    let host = host
        .parse::<Host>()
        .map_err(|e| CliError::Usage(e.to_string()))?;
    if host != Host::Claude {
        return Err(CliError::Usage(format!(
            "stored {} session has no recorded log path",
            host.as_str()
        )));
    }
    let slug: String = cwd
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let path = Path::new(home)
        .join("projects")
        .join(slug)
        .join(format!("{session_id}.jsonl"));
    Ok((
        host,
        SessionSource {
            agent_home: PathBuf::from(home.as_str()),
            path,
            session_id: session_id.clone(),
            repo_key: RepoKey(repo.clone()),
            cwd: PathBuf::from(cwd.as_str()),
        },
    ))
}
fn adapter(host: Host) -> Result<Box<dyn HostAdapter>, CliError> {
    Ok(match host {
        Host::Claude => Box::new(tkm_hosts::claude::ClaudeAdapter::new()),
        Host::Codex => Box::new(tkm_hosts::codex::CodexAdapter::new()),
        Host::Pi => Box::new(tkm_hosts::pi::PiAdapter::new()),
        _ => {
            return Err(CliError::Usage(format!(
                "cannot infer a readable source for {}",
                host.as_str()
            )));
        }
    })
}
fn db_error(error: rusqlite::Error) -> CliError {
    CliError::Usage(format!("database query failed: {error}"))
}
