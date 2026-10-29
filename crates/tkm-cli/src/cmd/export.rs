//! Repository exports with redacted, explicitly opt-in request digests.
use super::status::report;
use crate::{cli::ExportArgs, ctx::Ctx, error::CliError};
use serde::Serialize;
use std::collections::BTreeSet;
use std::process::ExitCode;
use tkm_decide::redact::Redactor;
use tkm_store::{
    DecisionRecord, DenylistEntry, FactRecord, InstallRecord, LedgerRecord, Repo, SessionRecord,
    ToolEvidence,
};

#[derive(Serialize)]
struct Export {
    repo: Option<Repo>,
    repo_key: String,
    sessions: Vec<SessionRecord>,
    facts: Vec<FactRecord>,
    decisions: Vec<DecisionRecord>,
    prune_evidence: Vec<ToolEvidence>,
    denylist: Vec<DenylistEntry>,
    installs: Vec<InstallRecord>,
    ledger: Vec<LedgerRecord>,
}

/// Export current repository state; digest fields require `--show-digests`.
///
/// # Errors
/// Returns an error when reading, serializing or writing the export fails.
pub fn run(ctx: &mut Ctx<'_>, args: &ExportArgs) -> Result<ExitCode, CliError> {
    let store = report::read_store(ctx)?;
    let load = || -> Result<Export, tkm_store::StoreError> {
        let sessions = store.list_sessions_by_repo(&ctx.repo.key, -1)?;
        let ids: BTreeSet<_> = sessions.iter().map(|s| s.id).collect();
        let mut decisions = store.list_recent_decisions(-1)?;
        decisions.retain(|d| {
            d.repo_key.as_ref() == Some(&ctx.repo.key)
                || d.repo_key.is_none() && d.session_id.is_some_and(|id| ids.contains(&id))
        });
        let mut ledger = Vec::new();
        let mut denylist = Vec::new();
        let mut checkouts = BTreeSet::new();
        checkouts.insert(ctx.cwd.clone());
        for session in &sessions {
            if let Some(row) = store.get_ledger(session.id)? {
                ledger.push(row);
            }
            checkouts.insert(session.cwd.clone());
        }
        for checkout in checkouts {
            denylist.extend(store.list_denylist(&checkout)?);
        }
        let installs = store
            .list_installs()?
            .into_iter()
            .filter(|install| {
                sessions.iter().any(|s| {
                    s.key.host == install.host
                        && std::path::Path::new(&s.key.agent_home) == install.agent_home
                }) || install
                    .files_written
                    .iter()
                    .any(|p| p.starts_with(&ctx.repo.root))
            })
            .collect::<Vec<_>>();
        let hosts: BTreeSet<_> = sessions
            .iter()
            .map(|s| s.key.host.as_str())
            .chain(installs.iter().map(|i| i.host.as_str()))
            .collect();
        let mut evidence = Vec::new();
        for host in hosts {
            if let Ok(host) = host.parse() {
                evidence.extend(store.list_tool_evidence(&ctx.repo.key, host)?);
            }
        }
        Ok(Export {
            repo: store.get_repo(&ctx.repo.key)?,
            repo_key: ctx.repo.key.0.clone(),
            sessions,
            facts: store.list_facts(&ctx.repo.key, None)?,
            decisions,
            prune_evidence: evidence,
            denylist,
            installs,
            ledger,
        })
    };
    let export = load().map_err(|e| report::store_error(ctx, e))?;
    let mut value = serde_json::to_value(&export).map_err(report::json_error)?;
    let redactor = report::redactor(ctx);
    redact_export(&mut value, &redactor, args.show_digests);
    let output = serde_json::to_string_pretty(&value).map_err(report::json_error)?;
    writeln!(ctx.stdout, "{output}")?;
    Ok(ExitCode::SUCCESS)
}
fn redact_export(value: &mut serde_json::Value, redactor: &Redactor, show_digests: bool) {
    match value {
        serde_json::Value::Object(fields) => {
            if !show_digests {
                fields.retain(|key, _| !key.contains("digest"));
            }
            *fields = std::mem::take(fields)
                .into_iter()
                .map(|(key, mut value)| {
                    redact_export(&mut value, redactor, show_digests);
                    (redactor.redact(&key), value)
                })
                .collect();
        }
        serde_json::Value::Array(values) => {
            for value in values {
                redact_export(value, redactor, show_digests);
            }
        }
        serde_json::Value::String(text) => {
            *text = redactor.redact(text);
        }
        _ => {}
    }
}
