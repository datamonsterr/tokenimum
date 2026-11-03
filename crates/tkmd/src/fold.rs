//! E4.5 fold workers: session-end fold, idle-fold watcher, repository and session resolution
//! (SPEC 6.2 rules, 7.1, 7.3, 7.5).
//!
//! A fold runs detached on a blocking task. Every error is logged to `errors.log` and recorded
//! as `fold_status = 'error'`; nothing here can panic the daemon (a panicking task becomes a
//! `JoinError`).

use std::collections::{BTreeMap, BTreeSet, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use notify::{RecursiveMode, Watcher};
use tkm_config::repo::repo_identity;
use tkm_core::facts::lifecycle::{Lifecycle, Status};
use tkm_core::facts::{
    self, Candidate, Fact, FactEvent, FactEventKind, FactsState, Params, SessionEvidence,
};
use tkm_core::model::{Host, RepoKey, StopReason, Timestamp};
use tkm_core::prune::{self, PruneEvidence};
use tkm_decide::api::{Answer, NoulCriteria, Question};
use tkm_decide::cache::Purpose;
use tkm_decide::client::{DecideRequest, Outcome};
use tkm_decide::digest::{DigestInput, build_digest};
use tkm_decide::redact::{RedactContext, Redactor};
use tkm_hosts::{HostAdapter, SessionSource};
use tkm_proto::monitor::{FactNotice, MonitorMessage};
use tkm_store::{
    CalibrationRecord, FactRecord, FactState, LedgerRecord, SessionKey, SessionRecord, Store,
};
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::backing::Backing;
use crate::daemon::{Daemon, HResult, HandlerError, SessionSeed, lock};
use crate::persist::SessionUpsert;
use crate::settings::Settings;
use crate::state::{StateMap, Tag};

/// Fraction of skipped records above which a session is partial (SPEC 6.4): ledger only.
const PARTIAL_PERCENT: u64 = 20;

/// Result of resolving a session's repository, settings and stored snapshot.
pub(crate) struct Resolved {
    pub repo: RepoKey,
    pub label: String,
    pub root: PathBuf,
    pub settings: Arc<Settings>,
    pub turn_base: i64,
    pub envelope: Vec<u32>,
    pub calibration: Vec<CalibrationRecord>,
    pub stale: Vec<SessionRecord>,
    pub error: Option<String>,
}

/// Everything a fold needs, captured when the session leaves memory.
pub(crate) struct FoldJob {
    pub key: SessionKey,
    pub host: Host,
    pub repo: RepoKey,
    pub cwd: PathBuf,
    pub agent_home: PathBuf,
    pub log_path: Option<PathBuf>,
    pub settings: Arc<Settings>,
    pub injected: BTreeSet<String>,
    pub overhead: (u64, Option<f64>),
    pub ended_ms: i64,
}

/// What a fold did.
pub(crate) struct FoldReport {
    pub status: &'static str,
    pub events: Vec<FactEvent>,
}

/// Filesystem watcher over session log directories (activity signal for idle detection).
#[derive(Default)]
pub struct LogWatch {
    watcher: Option<notify::RecommendedWatcher>,
    dirs: HashSet<PathBuf>,
}

fn json_string<T: serde::Serialize>(value: &T) -> Option<String> {
    match serde_json::to_value(value).ok()? {
        serde_json::Value::String(text) => Some(text),
        _ => None,
    }
}

/// Maps a stored fact to the core fact; `None` when the stored JSON no longer parses.
pub(crate) fn fact_from_record(record: &FactRecord) -> Option<Fact> {
    let family = serde_json::from_value(serde_json::Value::String(record.family.clone())).ok()?;
    let scope = serde_json::from_str(&record.scope).ok()?;
    let params: Params = serde_json::from_value(record.text_params.get("params")?.clone()).ok()?;
    let streak = record
        .text_params
        .get("contradiction_streak")
        .and_then(serde_json::Value::as_u64)
        .and_then(|v| u32::try_from(v).ok())
        .unwrap_or(0);
    let status = match record.state {
        FactState::Candidate => Status::Candidate,
        FactState::Active => Status::Active,
        FactState::Retired => Status::Retired,
    };
    Some(Fact {
        candidate: Candidate {
            id: record.fact_id.clone(),
            family,
            params,
            scope,
            evidence: Vec::new(),
        },
        life: Lifecycle {
            status,
            confirmations: record.confirmations,
            contradictions: record.contradictions,
            score: record.score,
            fire_num: record.fire_count,
            fire_den: record.fire_sessions,
            contradiction_streak: streak,
        },
    })
}

fn record_from_fact(repo: &RepoKey, fact: &Fact) -> Option<FactRecord> {
    Some(FactRecord {
        repo_key: repo.clone(),
        fact_id: fact.candidate.id.clone(),
        family: json_string(&fact.candidate.family)?,
        scope: serde_json::to_string(&fact.candidate.scope).ok()?,
        text_params: text_params(fact)?,
        state: match fact.life.status {
            Status::Candidate => FactState::Candidate,
            Status::Active => FactState::Active,
            Status::Retired => FactState::Retired,
        },
        score: fact.life.score,
        confirmations: fact.life.confirmations,
        contradictions: fact.life.contradictions,
        fire_count: fact.life.fire_num,
        fire_sessions: fact.life.fire_den,
        state_changed_at: None,
    })
}

fn text_params(fact: &Fact) -> Option<serde_json::Value> {
    Some(serde_json::json!({
        "params": serde_json::to_value(&fact.candidate.params).ok()?,
        "contradiction_streak": fact.life.contradiction_streak,
    }))
}

fn persist_facts(
    store: &Store,
    repo: &RepoKey,
    before: &[FactRecord],
    after: &FactsState,
    at: Timestamp,
) -> HResult<()> {
    for fact in after.facts.values() {
        let Some(record) = record_from_fact(repo, fact) else {
            continue;
        };
        let existing = before.iter().find(|r| r.fact_id == record.fact_id);
        if let Some(old) = existing {
            {
                if let Some(params) = text_params(fact) {
                    store.update_fact_params(repo, &record.fact_id, &params)?;
                }
                store.update_fact_stats(
                    repo,
                    &record.fact_id,
                    record.score,
                    record.confirmations,
                    record.contradictions,
                    record.fire_count,
                    record.fire_sessions,
                )?;
                if old.state != record.state && old.state != FactState::Retired {
                    store.transition_fact_at(repo, &record.fact_id, record.state, at)?;
                }
            }
        } else {
            {
                store.insert_candidate_fact(&record)?;
                if record.state != FactState::Candidate {
                    store.transition_fact_at(repo, &record.fact_id, record.state, at)?;
                }
            }
        }
    }
    Ok(())
}

/// The pure results of folding one session.
pub struct ReplayReport {
    /// Updated facts and replay protection.
    pub facts: FactsState,
    /// Fact lifecycle actions generated by the fold.
    pub events: Vec<FactEvent>,
    /// Tool usage evidence generated by the fold.
    pub prune: PruneEvidence,
}

/// Run the daemon's session-fold computations without persistence or network access.
#[must_use]
pub fn replay_fold(
    session: &tkm_core::model::Session,
    state: FactsState,
    injected: &BTreeSet<String>,
    config: &tkm_core::facts::FactsConfig,
) -> ReplayReport {
    let mut normalized = session.clone();
    normalized.partial |= normalized.total_records > 0
        && normalized.skipped_records.saturating_mul(100)
            > normalized.total_records.saturating_mul(PARTIAL_PERCENT);
    let mut prune = PruneEvidence::default();
    if !normalized.partial {
        prune::fold(&mut prune, &normalized);
    }
    let evidence = SessionEvidence {
        session: normalized,
        injected_ids: injected.clone(),
        decisions: None,
    };
    let (facts, events) = facts::update(state, &evidence, config);
    ReplayReport {
        facts,
        events,
        prune,
    }
}

#[cfg(test)]
fn prepare_fact_questions(
    session: &tkm_core::model::Session,
    state: &FactsState,
    injected: &BTreeSet<String>,
) -> (BTreeMap<String, Question>, BTreeMap<String, Question>) {
    let redactor = Redactor::new(RedactContext {
        repo_root: session.cwd.clone(),
        home: PathBuf::new(),
        env: Vec::new(),
    });
    fact_questions(session, state, injected, &redactor)
}

fn fact_questions(
    session: &tkm_core::model::Session,
    state: &FactsState,
    injected: &BTreeSet<String>,
    redactor: &Redactor,
) -> (BTreeMap<String, Question>, BTreeMap<String, Question>) {
    let observations = facts::observe::observe(session);
    let mut filters = BTreeMap::new();
    for candidate in facts::extract::extract(&observations) {
        let claim = redactor.redact(&facts::render::claim(&candidate));
        for (prefix, instructions, yes, no) in [
            (
                "stable",
                "Does this fact hold for this repository regardless of task?",
                "repository-wide",
                "task-specific",
            ),
            (
                "safe",
                "Is this fact harmless and free of destructive, secret-bearing or completion advice?",
                "harmless",
                "unsafe or completion advice",
            ),
        ] {
            filters.insert(
                format!("{prefix}_{}", candidate.id),
                Question::Noul {
                    instructions: format!("{instructions} Fact: {claim}"),
                    criteria: Some(NoulCriteria {
                        when_true: yes.into(),
                        when_false: no.into(),
                    }),
                },
            );
        }
    }
    let contradictions = facts::exercised_ids(session, state, injected)
        .into_iter()
        .filter_map(|id| {
            let fact = state.facts.get(&id)?;
            let claim = redactor.redact(&facts::render::claim(&fact.candidate));
            Some((
                format!("contradicts_{id}"),
                Question::Noul {
                    instructions: format!("Does this session contradict the fact: {claim}?"),
                    criteria: Some(NoulCriteria {
                        when_true: "the session contradicts it".into(),
                        when_false: "the session does not contradict it".into(),
                    }),
                },
            ))
        })
        .collect();
    (filters, contradictions)
}

struct FoldDecider<'a> {
    daemon: &'a Daemon,
    runtime: tokio::runtime::Handle,
}

fn decide_facts(
    store: &Store,
    record: &SessionRecord,
    session: &tkm_core::model::Session,
    state: &FactsState,
    job: &FoldJob,
    context: &FoldDecider<'_>,
) -> Option<facts::Decisions> {
    if !job.settings.decide.enabled || !job.settings.facts_enabled {
        return None;
    }
    let home = context
        .daemon
        .env
        .iter()
        .rev()
        .find(|(k, _)| k == "HOME")
        .map(|(_, v)| PathBuf::from(v))
        .unwrap_or_default();
    let redactor = Redactor::new(RedactContext {
        repo_root: job.cwd.clone(),
        home,
        env: context.daemon.env.clone(),
    });
    let (filters, contradictions) = fact_questions(session, state, &job.injected, &redactor);
    let calls: Vec<_> = session
        .turns
        .iter()
        .flat_map(|turn| turn.calls.iter().cloned())
        .collect();
    let prompt = session
        .turns
        .iter()
        .map(|turn| turn.prompt.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let digest = build_digest(
        DigestInput {
            prompt: &prompt,
            calls: &calls,
            tests: None,
        },
        &redactor,
    );
    let mut scores = BTreeMap::<String, facts::DecisionScores>::new();
    for (purpose, questions) in [
        (Purpose::FactFilter, filters),
        (Purpose::FactContradiction, contradictions),
    ] {
        if questions.is_empty() {
            continue;
        }
        let request = DecideRequest {
            purpose,
            session_id: format!("{}:{}", job.host.as_str(), job.key.session_id),
            state: digest.clone(),
            questions: questions.clone(),
        };
        let Outcome::Decision(decision) = context
            .runtime
            .block_on(context.daemon.decider.decide(request))
        else {
            continue;
        };
        let rows = store.list_calibration().unwrap_or_default();
        let calibrated = rows.iter().find(|row| {
            row.model_version == decision.model_version
                && row.purpose == purpose.as_str()
                && crate::decider::calibration_qualified(row)
        });
        let now = i64::try_from(context.daemon.clock.now_ms()).unwrap_or(i64::MAX);
        let shadow = crate::decider::purpose_shadow(
            store,
            purpose.as_str(),
            now,
            job.settings.shadow_days,
            job.settings.shadow_turns,
        );
        let thresholds = calibrated.and_then(|row| {
            Some((
                row.thresholds.get("t_low")?.as_f64()?,
                row.thresholds.get("t_high")?.as_f64()?,
            ))
        });
        let thresholds = thresholds.filter(|(low, high)| {
            low.is_finite()
                && high.is_finite()
                && (0.0..=1.0).contains(low)
                && (0.0..=1.0).contains(high)
                && low <= high
        });
        let valid = questions.keys().all(|key| matches!(decision.answers.get(key), Some(Answer::Noul { noul }) if noul.is_finite() && (0.0..=1.0).contains(noul)));
        let acting = thresholds.is_some() && !shadow && valid;
        let item = tkm_store::NewDecision {
            model: decision.model_version.clone(),
            purpose: purpose.as_str().into(),
            questions: serde_json::to_value(&questions).unwrap_or_default(),
            answers: serde_json::to_value(&decision.answers).unwrap_or_default(),
            cost_usd: if decision.cached {
                Some(0.0)
            } else {
                decision.usage.cost
            },
            latency_ms: u32::try_from(decision.latency_ms).unwrap_or(u32::MAX),
            shadow: !acting,
        };
        if store
            .insert_decision_with_context(
                &item,
                Some(Timestamp(now)),
                Some(&job.repo),
                Some(record.id),
                Some(&digest),
            )
            .is_err()
            || !acting
        {
            continue;
        }
        let Some((low, high)) = thresholds else {
            continue;
        };
        for (key, answer) in &decision.answers {
            let Answer::Noul { noul } = answer else {
                continue;
            };
            if let Some(id) = key.strip_prefix("stable_") {
                scores.entry(id.to_owned()).or_default().stable =
                    Some(if *noul >= low { 1.0 } else { 0.0 });
            }
            if let Some(id) = key.strip_prefix("safe_") {
                scores.entry(id.to_owned()).or_default().safe =
                    Some(if *noul >= high { 1.0 } else { 0.0 });
            }
            if let Some(id) = key.strip_prefix("contradicts_") {
                scores.entry(id.to_owned()).or_default().contradicts =
                    Some(if *noul >= high { 1.0 } else { 0.0 });
            }
        }
    }
    (!scores.is_empty()).then_some(facts::Decisions {
        t_low: 0.3,
        t_high: 0.8,
        scores,
    })
}

fn fold_prune(
    store: &Store,
    session: &tkm_core::model::Session,
    ended: Timestamp,
    evidence: &PruneEvidence,
) -> HResult<()> {
    for name in &evidence.used_tools {
        store.record_tool_used(&session.repo_key, session.host, name, ended)?;
    }
    for (name, e) in &evidence.tools {
        if evidence.used_tools.contains(name) {
            continue;
        }
        for _ in 0..e.prompts.min(1_000) {
            store.record_tool_unused_prompt(&session.repo_key, session.host, name)?;
        }
        if e.sessions > 0 {
            store.record_tool_unused_session(&session.repo_key, session.host, name)?;
        }
    }
    Ok(())
}

// Labels are saved on the original batch, preserving the identity of every question.
// Fact predictions train only on later sessions; relevance trains on its own session.
fn label_decisions(
    store: &Store,
    record: &SessionRecord,
    session: &tkm_core::model::Session,
    state: &FactsState,
    injected: &BTreeSet<String>,
    verdicts: &[FactEvent],
) -> HResult<()> {
    let mut outcomes = tkm_decide::labels::session_outcomes(session, state, injected);
    for verdict in verdicts
        .iter()
        .filter(|v| v.kind == FactEventKind::Contradicted)
    {
        outcomes.insert(verdict.fact_id.clone(), false);
    }
    for decision in store.list_recent_decisions(-1)? {
        if decision.repo_key.as_ref() != Some(&record.repo_key) {
            continue;
        }
        let relevance = decision.purpose == "relevance";
        let facts = matches!(
            decision.purpose.as_str(),
            "fact_filter" | "fact_contradiction"
        );
        if !(relevance || facts)
            || (relevance && decision.session_id != Some(record.id))
            || (facts
                && (decision.session_id == Some(record.id)
                    || decision
                        .created_at
                        .is_none_or(|at| at.0 >= record.started.0)))
        {
            continue;
        }
        let Some(questions) = decision.questions.as_object() else {
            continue;
        };
        let mut batch_outcomes = outcomes.clone();
        if relevance {
            for (name, question) in questions {
                let Some(id) = name.strip_prefix("rel_memory:") else {
                    continue;
                };
                let reference = question
                    .get("instructions")
                    .and_then(serde_json::Value::as_str)
                    .and_then(|text| {
                        text.split_once("\nReference: ")
                            .map(|(_, reference)| reference)
                    });
                if let Some(label) = reference
                    .and_then(|reference| tkm_decide::labels::reference_outcome(session, reference))
                {
                    batch_outcomes.insert(format!("memory:{id}"), label);
                }
            }
        }
        let mut labels = decision
            .outcome_label
            .as_deref()
            .and_then(|text| serde_json::from_str::<BTreeMap<String, bool>>(text).ok())
            .unwrap_or_default();
        for (question, label) in tkm_decide::labels::question_labels(
            questions.keys().map(String::as_str),
            &batch_outcomes,
        ) {
            // A later contradiction revokes an earlier confirmation.
            labels
                .entry(question)
                .and_modify(|old| {
                    if decision.purpose == "fact_contradiction" {
                        *old |= label;
                    } else {
                        *old &= label;
                    }
                })
                .or_insert(label);
        }
        if !labels.is_empty() {
            store.set_decision_outcome(
                decision.id,
                &serde_json::to_string(&labels).map_err(tkm_store::StoreError::from)?,
            )?;
        }
    }
    Ok(())
}

fn fold_inner(
    store: &Store,
    record: &SessionRecord,
    adapter: &dyn HostAdapter,
    job: &FoldJob,
    context: Option<&FoldDecider<'_>>,
) -> HResult<FoldReport> {
    let Some(path) = job.log_path.clone().or_else(|| {
        // A derived location that does not exist is a session without a log, not a read error.
        derived_log_path(job.host, &job.agent_home, &job.cwd, &job.key.session_id)
            .filter(|path| path.exists())
    }) else {
        store.set_session_fold_status(record.id, "skipped")?;
        return Ok(FoldReport {
            status: "skipped",
            events: Vec::new(),
        });
    };
    let source = SessionSource {
        agent_home: job.agent_home.clone(),
        path,
        session_id: job.key.session_id.clone(),
        repo_key: job.repo.clone(),
        cwd: job.cwd.clone(),
    };
    let mut session = adapter.read_session(&source)?;
    session.repo_key = job.repo.clone();
    session.host = job.host;
    let skipped_too_many = session.total_records > 0
        && session.skipped_records.saturating_mul(100)
            > session.total_records.saturating_mul(PARTIAL_PERCENT);
    session.partial = session.partial || skipped_too_many;

    if job.settings.ledger_enabled {
        store.upsert_ledger(&LedgerRecord {
            session_id: record.id,
            usage: session.usage,
            priced_cost: job.settings.pricing.price_session(&session, None),
            overhead_tokens: job.overhead.0,
            overhead_cost: job.overhead.1,
        })?;
    }
    if session.partial {
        store.set_session_fold_status(record.id, "partial")?;
        return Ok(FoldReport {
            status: "partial",
            events: Vec::new(),
        });
    }
    let mut skills = tkm_core::prune::skills::SkillReport::default();
    tkm_core::prune::skills::fold_skills(&mut skills, &session);
    for name in job
        .injected
        .iter()
        .filter_map(|id| id.strip_prefix("skill:"))
    {
        skills
            .skills
            .entry(name.to_owned())
            .or_default()
            .loaded_sessions = 1;
    }
    store.replace_skill_evidence(record.id, &skills)?;
    let mut events = Vec::new();
    if record.fold_status != "folded" {
        let before = store.list_facts(&job.repo, None)?;
        let mut state = FactsState::default();
        for fact in before.iter().filter_map(fact_from_record) {
            state.facts.insert(fact.candidate.id.clone(), fact);
        }
        for earlier in store.list_sessions_by_repo(&job.repo, 10_000)? {
            if earlier.fold_status == "folded" && earlier.id != record.id {
                state.processed.insert((
                    earlier.key.host.as_str().to_owned(),
                    PathBuf::from(&earlier.key.agent_home),
                    earlier.key.session_id,
                ));
            }
        }
        let decisions =
            context.and_then(|context| decide_facts(store, record, &session, &state, job, context));
        let mut result = replay_fold(&session, state.clone(), &job.injected, &job.settings.facts);
        if decisions.is_some() {
            let evidence = SessionEvidence {
                session: session.clone(),
                injected_ids: job.injected.clone(),
                decisions,
            };
            (result.facts, result.events) =
                facts::update(state.clone(), &evidence, &job.settings.facts);
        }
        label_decisions(
            store,
            record,
            &session,
            &state,
            &job.injected,
            &result.events,
        )?;
        if job.settings.facts_enabled {
            persist_facts(
                store,
                &job.repo,
                &before,
                &result.facts,
                Timestamp(job.ended_ms),
            )?;
            events = result.events;
            crate::persist::fact_actions(
                store,
                &job.repo,
                record.id,
                Timestamp(job.ended_ms),
                &events,
            )?;
        }
        fold_prune(store, &session, Timestamp(job.ended_ms), &result.prune)?;
        store.refresh_prune_readiness(
            &session.repo_key,
            session.host,
            &job.settings.prune,
            Timestamp(job.ended_ms),
        )?;
    }
    store.set_session_fold_status(record.id, "folded")?;
    Ok(FoldReport {
        status: "folded",
        events,
    })
}

/// Runs one fold on the calling (blocking) thread.
fn run_fold(
    backing: &dyn Backing,
    adapter: &dyn HostAdapter,
    job: &FoldJob,
    context: Option<&FoldDecider<'_>>,
) -> HResult<FoldReport> {
    let store = backing.open()?;
    let Some(record) = store.find_session(&job.key)? else {
        return Err(HandlerError::Other(format!(
            "fold: session {} has no row",
            job.key.session_id
        )));
    };
    let result = fold_inner(&store, &record, adapter, job, context);
    if result.is_err() {
        let _ = store.set_session_fold_status(record.id, "error");
    }
    result
}

fn derived_log_path(
    host: Host,
    agent_home: &Path,
    cwd: &Path,
    session_id: &str,
) -> Option<PathBuf> {
    // IDs enter from host payloads; never let them escape a session store.
    if session_id.is_empty()
        || (session_id.contains('/') || session_id.contains('\\'))
        || session_id == "."
        || session_id == ".."
    {
        return None;
    }
    let file = |path: PathBuf| path.is_file().then_some(path);
    match host {
        Host::Claude => {
            let slug: String = cwd
                .to_string_lossy()
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                .collect();
            Some(
                agent_home
                    .join("projects")
                    .join(slug)
                    .join(format!("{session_id}.jsonl")),
            )
        }
        Host::Codex => find_session(&agent_home.join("sessions"), session_id, 4, Host::Codex),
        Host::Agy => {
            let logs = agent_home
                .join("antigravity-cli/brain")
                .join(session_id)
                .join(".system_generated/logs");
            file(logs.join("transcript_full.jsonl")).or_else(|| file(logs.join("transcript.jsonl")))
        }
        Host::Cursor => find_session(&agent_home.join("projects"), session_id, 5, Host::Cursor),
        Host::Kimi => file(
            tkm_hosts::kimi::session_dir(agent_home, &cwd.to_string_lossy(), session_id)
                .join("context.jsonl"),
        ),
        Host::Deepseek => file(
            agent_home
                .join("sessions")
                .join(format!("{session_id}.json")),
        ),
        Host::Opencode => agent_home.parent().and_then(Path::parent).map(|home| {
            let data = home.join(".local/share/opencode");
            let db = data.join("opencode.db");
            if db.is_file() { db } else { data }
        }),
        Host::Pi => find_session(&agent_home.join("sessions"), session_id, 2, Host::Pi),
        _ => None,
    }
}

fn find_session(root: &Path, session_id: &str, depth: usize, host: Host) -> Option<PathBuf> {
    fn collect(
        root: &Path,
        session_id: &str,
        depth: usize,
        host: Host,
        matches: &mut Vec<PathBuf>,
    ) {
        if depth == 0 || (host != Host::Codex && matches.len() > 1) {
            return;
        }
        let Ok(entries) = std::fs::read_dir(root) else {
            return;
        };
        for entry in entries.flatten() {
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            let path = entry.path();
            if kind.is_dir() {
                collect(&path, session_id, depth - 1, host, matches);
            } else if kind.is_file() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                let is_match = match host {
                    Host::Codex => {
                        name.starts_with("rollout-")
                            && [
                                format!("-{session_id}.jsonl"),
                                format!("-{session_id}.jsonl.zst"),
                            ]
                            .iter()
                            .any(|suffix| name.ends_with(suffix))
                    }
                    Host::Cursor => {
                        name == format!("{session_id}.jsonl")
                            && path
                                .parent()
                                .and_then(Path::file_name)
                                .is_some_and(|name| name == session_id)
                    }
                    Host::Pi => name.ends_with(&format!("_{session_id}.jsonl")),
                    _ => false,
                };
                if is_match {
                    matches.push(path);
                }
            }
        }
    }
    let mut matches = Vec::new();
    collect(root, session_id, depth, host, &mut matches);
    // Rollout snapshots can share a session ID; the later dated path is current.
    if host == Host::Codex {
        matches.sort();
        return matches.pop();
    }
    // An ambiguous ID must not fold another session.
    if matches.len() == 1 {
        matches.pop()
    } else {
        None
    }
}

impl Daemon {
    /// Repository identity, settings and stored snapshot for a session. Blocking.
    pub(crate) fn resolve_blocking(&self, seed: &SessionSeed) -> Resolved {
        let cached = lock(&self.repo_cache).get(&seed.cwd).cloned();
        let (repo, label, root) = cached.unwrap_or_else(|| {
            let var = |name: &str| {
                self.env
                    .iter()
                    .rev()
                    .find(|(k, _)| k == name)
                    .map(|(_, v)| v.clone())
            };
            let home = var("HOME").map(PathBuf::from);
            let identity = repo_identity(
                &seed.cwd,
                &var("GIT_CEILING_DIRECTORIES").unwrap_or_default(),
                home.as_deref(),
            );
            let entry = (identity.key, identity.label, identity.root);
            lock(&self.repo_cache).insert(seed.cwd.clone(), entry.clone());
            entry
        });
        let settings = self.settings_source.settings_for(Some(&root));
        let mut resolved = Resolved {
            repo: repo.clone(),
            label,
            root,
            settings,
            turn_base: 0,
            envelope: Vec::new(),
            calibration: Vec::new(),
            stale: Vec::new(),
            error: None,
        };
        let outcome = (|| -> Result<(), tkm_store::StoreError> {
            let store = self.backing.open()?;
            self.refresh_host_health(&store, seed.host)?;
            resolved.envelope = store.envelope_samples(&repo, seed.host)?;
            resolved.calibration = store.list_calibration()?;
            let key = SessionKey {
                host: seed.host,
                agent_home: seed.agent_home.to_string_lossy().into_owned(),
                session_id: seed.session_id.clone(),
            };
            if let Some(record) = store.find_session(&key)? {
                resolved.turn_base = store
                    .list_turns(record.id)?
                    .last()
                    .map_or(0, |turn| turn.turn_idx.saturating_add(1));
            }
            let now = i64::try_from(self.clock.now_ms()).unwrap_or(i64::MAX);
            let grace = i64::try_from(self.stale_after.as_millis()).unwrap_or(i64::MAX);
            resolved.stale = store
                .list_sessions_by_repo(&repo, 50)?
                .into_iter()
                .filter(|r| {
                    r.key.session_id != seed.session_id
                        && matches!(r.fold_status.as_str(), "open" | "pending")
                        && now.saturating_sub(r.ended.unwrap_or(r.started).0) >= grace
                })
                .collect();
            Ok(())
        })();
        if let Err(error) = outcome {
            resolved.error = Some(format!("storage: {error}"));
        }
        resolved
    }

    /// Removes a live session from memory and folds it in the background.
    pub(crate) fn fold_live_session(self: &Arc<Self>, tag: &Tag, ended: bool) {
        let job = {
            let mut guard = lock(&self.state);
            let mut outbox = lock(&self.outbox);
            self.finish_turn(&mut guard, tag, StopReason::Interrupted, &mut outbox);
            let StateMap {
                sessions,
                turn_bases,
                ..
            } = &mut *guard;
            let Some(session) = sessions.remove(tag) else {
                return;
            };
            self.escalator
                .forget_session(session.host, &session.session_id);
            turn_bases.insert(tag.clone(), session.next_turn_idx);
            let now = i64::try_from(self.clock.now_ms()).unwrap_or(i64::MAX);
            let ended_ms = if ended {
                now
            } else {
                i64::try_from(session.last_activity_ms).unwrap_or(now)
            };
            outbox.push_session(SessionUpsert {
                key: session.key.clone(),
                repo_key: session.repo.clone(),
                cwd: session.cwd.clone(),
                started: session.started,
                ended: Some(Timestamp(ended_ms)),
                fold_status: "pending".into(),
                shadow: session.shadow,
                integration_level: session.integration_level.into(),
            });
            self.publish(|| {
                MonitorMessage::Session(Self::session_update(
                    &session,
                    if ended { "ended" } else { "idle" },
                ))
            });
            let overhead = self.ledger.session_overhead(&format!(
                "{}:{}",
                session.host.as_str(),
                session.session_id
            ));
            FoldJob {
                key: session.key.clone(),
                host: session.host,
                repo: session.repo.clone(),
                cwd: session.cwd.clone(),
                agent_home: session.agent_home.clone(),
                log_path: session.log_path.clone(),
                settings: session.settings.clone(),
                injected: session.injected.clone(),
                overhead,
                ended_ms,
            }
        };
        self.spawn_fold(job);
    }

    /// Folds earlier unfinished sessions found in the database (not live in memory).
    pub(crate) fn fold_stored_sessions(
        self: &Arc<Self>,
        resolved: &Resolved,
        seed: &SessionSeed,
        _now: u64,
    ) {
        for record in &resolved.stale {
            if !resolved.settings.host(record.key.host).enabled {
                continue;
            }
            let tag: Tag = (record.key.host, record.key.session_id.clone());
            if lock(&self.state).sessions.contains_key(&tag) {
                continue;
            }
            let agent_home = PathBuf::from(&record.key.agent_home);
            self.spawn_fold(FoldJob {
                key: record.key.clone(),
                host: record.key.host,
                repo: record.repo_key.clone(),
                cwd: record.cwd.clone(),
                log_path: self.stored_log_path(record),
                agent_home,
                settings: resolved.settings.clone(),
                injected: BTreeSet::new(),
                overhead: self.ledger.session_overhead(&format!(
                    "{}:{}",
                    record.key.host.as_str(),
                    record.key.session_id
                )),
                ended_ms: record.ended.unwrap_or(record.started).0,
            });
            let _ = seed;
        }
    }

    /// Log location of a stored session, which keeps no transcript path of its own.
    fn stored_log_path(&self, record: &SessionRecord) -> Option<PathBuf> {
        if record.key.host == Host::Opencode {
            let var = |key: &str| {
                self.env
                    .iter()
                    .rev()
                    .find(|(name, _)| name == key)
                    .map(|(_, value)| value)
            };
            let db = var("OPENCODE_DB")
                .map(|value| PathBuf::from(value.as_str()))
                .or_else(|| {
                    var("XDG_DATA_HOME")
                        .map(|path| PathBuf::from(path.as_str()).join("opencode/opencode.db"))
                });
            if db.is_some() {
                return db;
            }
        }
        derived_log_path(
            record.key.host,
            Path::new(&record.key.agent_home),
            &record.cwd,
            &record.key.session_id,
        )
    }

    /// Re-prices ledger rows that have tokens but no price, for example rows written before a
    /// model ID could be matched to `pricing.toml`. Reads each session log again and updates
    /// only its ledger row; facts, pruning and fold status are untouched. The maintenance loop
    /// runs it once per daemon start; it is idempotent. Returns the number of rows priced.
    pub async fn reprice_ledger(self: &Arc<Self>) -> usize {
        let me = self.clone();
        tokio::task::spawn_blocking(move || me.reprice_ledger_blocking())
            .await
            .unwrap_or(0)
    }

    fn reprice_ledger_blocking(&self) -> usize {
        let pricing = &self.default_settings.pricing;
        let records = match self
            .backing
            .open()
            .and_then(|store| Ok((store.list_unpriced_ledger_sessions()?, store)))
        {
            Ok(found) => found,
            Err(error) => {
                self.log_error(&format!("reprice ledger: {error}"));
                return 0;
            }
        };
        let (records, store) = records;
        let mut priced = 0;
        for record in records {
            let label = format!("{} {}", record.key.host.as_str(), record.key.session_id);
            let Some(path) = self.stored_log_path(&record) else {
                self.log_error(&format!("reprice ledger {label}: no session log path"));
                continue;
            };
            let source = SessionSource {
                agent_home: PathBuf::from(&record.key.agent_home),
                path,
                session_id: record.key.session_id.clone(),
                repo_key: record.repo_key.clone(),
                cwd: record.cwd.clone(),
            };
            let session = match self.adapters.get(record.key.host).read_session(&source) {
                Ok(session) => session,
                Err(error) => {
                    self.log_error(&format!("reprice ledger {label}: unreadable log: {error}"));
                    continue;
                }
            };
            let Some(cost) = pricing.price_session(&session, None) else {
                let models: Vec<&str> = session.usage_by_model.keys().map(String::as_str).collect();
                self.log_error(&format!(
                    "reprice ledger {label}: no price for models {models:?} (unknown, ambiguous, or usage not attributed to a model)"
                ));
                continue;
            };
            match store.reprice_ledger(record.id, session.usage, cost) {
                Ok(true) => priced += 1,
                Ok(false) => {}
                Err(error) => self.log_error(&format!(
                    "reprice ledger {} {}: {error}",
                    record.key.host.as_str(),
                    record.key.session_id
                )),
            }
        }
        priced
    }

    pub(crate) fn spawn_fold(self: &Arc<Self>, job: FoldJob) {
        if !job.settings.host(job.host).enabled {
            return;
        }
        self.folds.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let me = self.clone();
        tokio::spawn(async move {
            me.flush_now().await;
            let worker = me.clone();
            let session_id = job.key.session_id.clone();
            let repo = job.repo.clone();
            let host = job.host;
            let runtime = tokio::runtime::Handle::current();
            let joined = tokio::task::spawn_blocking(move || {
                let adapter = worker.adapters.get(job.host);
                let report = run_fold(
                    worker.backing.as_ref(),
                    adapter.as_ref(),
                    &job,
                    Some(&FoldDecider {
                        daemon: &worker,
                        runtime,
                    }),
                )?;
                let store = worker.backing.open()?;
                let durable = if let Some(record) = store.find_session(&job.key)? {
                    store
                        .list_turns(record.id)?
                        .last()
                        .map_or(0, |turn| turn.turn_idx.saturating_add(1))
                } else {
                    0
                };
                Ok::<_, HandlerError>((report, durable))
            })
            .await;
            match joined {
                Ok(Ok((report, durable))) => {
                    let tag = (host, session_id.clone());
                    let mut state = lock(&me.state);
                    if !state.sessions.contains_key(&tag)
                        && state
                            .turn_bases
                            .get(&tag)
                            .is_some_and(|base| *base <= durable)
                    {
                        state.turn_bases.remove(&tag);
                    }
                    drop(state);
                    for event in report.events {
                        me.publish(|| {
                            MonitorMessage::Fact(FactNotice {
                                repo: repo.0.clone(),
                                fact_id: event.fact_id.clone(),
                                kind: match event.kind {
                                    FactEventKind::Confirmed => "confirmed",
                                    FactEventKind::Contradicted => "contradicted",
                                    FactEventKind::Promoted => "promoted",
                                    FactEventKind::Retired => "retired",
                                }
                                .to_owned(),
                            })
                        });
                    }
                    me.publish(|| {
                        MonitorMessage::Session(tkm_proto::monitor::SessionUpdate {
                            host,
                            session_id: session_id.clone(),
                            turn_id: None,
                            repo: Some(repo.0.clone()),
                            phase: "folded".into(),
                            turn_calls: 0,
                            envelope: None,
                            envelope_samples: None,
                            envelope_min_samples: None,
                            loop_state: report.status.to_owned(),
                        })
                    });
                }
                Ok(Err(error)) => {
                    me.log_error(&format!("fold {} {session_id}: {error}", host.as_str()));
                }
                Err(error) => me.log_error(&format!("fold task {session_id}: {error}")),
            }
            me.folds.fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
        });
    }

    /// Folds every session idle for at least `idle_after`.
    #[allow(clippy::unused_async, clippy::unused_async_trait_impl)]
    pub async fn scan_idle(self: &Arc<Self>) {
        let now = self.clock.now_ms();
        let idle = u64::try_from(self.idle_after.as_millis()).unwrap_or(u64::MAX);
        let due: Vec<Tag> = lock(&self.state)
            .sessions
            .iter()
            .filter(|(_, s)| now.saturating_sub(s.last_activity_ms) >= idle)
            .map(|(tag, _)| tag.clone())
            .collect();
        for tag in due {
            self.fold_live_session(&tag, false);
        }
    }

    /// Wakes every `scan_interval` while sessions exist; sleeps until a session appears otherwise.
    pub(crate) async fn idle_loop(self: Arc<Self>) {
        loop {
            if lock(&self.state).sessions.is_empty() {
                self.activity.notified().await;
            } else {
                tokio::select! {
                    () = tokio::time::sleep(self.scan_interval) => {}
                    () = self.activity.notified() => {}
                }
            }
            self.scan_idle().await;
        }
    }

    /// Registers the directory of a session log with the filesystem watcher, if one runs.
    pub(crate) fn watch_log(&self, seed: &SessionSeed) {
        let Some(dir) = seed.log_path.as_deref().and_then(Path::parent) else {
            return;
        };
        let mut watch = lock(&self.log_watch);
        let LogWatch { watcher, dirs } = &mut *watch;
        let Some(watcher) = watcher.as_mut() else {
            return;
        };
        // Best effort: a directory that does not exist yet (or cannot be watched) falls back to
        // the periodic scan, which needs no log line.
        if dirs.insert(dir.to_path_buf()) {
            let _ = watcher.watch(dir, RecursiveMode::NonRecursive);
        }
    }

    /// A log file changed: the session behind it is active.
    pub(crate) fn touch_log(&self, path: &Path) {
        let now = self.clock.now_ms();
        for session in lock(&self.state).sessions.values_mut() {
            if session.log_path.as_deref() == Some(path) {
                session.last_activity_ms = now;
            }
        }
    }
}

/// Starts the filesystem watcher. When it cannot start, the periodic scan of
/// [`Daemon::idle_loop`] remains the only activity source.
pub(crate) fn spawn_log_watcher(daemon: Arc<Daemon>) -> JoinHandle<()> {
    tokio::spawn(async move {
        let (tx, mut rx) = mpsc::unbounded_channel::<PathBuf>();
        let created = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            if let Ok(event) = event {
                for path in event.paths {
                    let _ = tx.send(path);
                }
            }
        });
        match created {
            Ok(watcher) => lock(&daemon.log_watch).watcher = Some(watcher),
            Err(error) => {
                daemon.log_error(&format!(
                    "log watcher unavailable, using the 60 s scan: {error}"
                ));
                return;
            }
        }
        while let Some(path) = rx.recv().await {
            daemon.touch_log(&path);
        }
    })
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]
    use super::*;
    use tkm_core::facts::Family;

    #[tokio::test]
    async fn redetection_refreshes_live_session_prune_protection() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use tkm_integrations::{EscalationEvent, Escalator};
        use tkm_proto::{EventKind, PROTOCOL_VERSION, Request};
        struct Adapter(AtomicBool);
        impl Escalator for Adapter {
            fn superorchestrator_enabled(&self) -> bool {
                self.0.load(Ordering::SeqCst)
            }
            fn handles(&self, _: Host, _: &str) -> bool {
                false
            }
            fn escalate(&self, _: &EscalationEvent) {}
        }
        let dir = tempfile::tempdir().unwrap();
        let paths = tkm_config::paths::Paths::from_dirs(
            dir.path(),
            Some(&dir.path().join("data")),
            Some(&dir.path().join("config")),
            Some(&dir.path().join("run")),
        );
        let adapter = Arc::new(Adapter(AtomicBool::new(false)));
        let mut options = crate::daemon::DaemonOptions::new(paths);
        options.escalator = adapter.clone();
        let daemon = Daemon::new(options);
        daemon.handle(Request { version: PROTOCOL_VERSION, host: Host::Claude, event: EventKind::SessionStart, host_event: Some("SessionStart".into()), payload: serde_json::json!({"session_id":"s", "hook_event_name":"SessionStart", "cwd":"/tmp"}), env: Default::default(), cwd: None }).await;
        adapter.0.store(true, Ordering::SeqCst);
        daemon.refresh_integration_settings();
        let state = lock(&daemon.state);
        let session = state.sessions.get(&(Host::Claude, "s".into())).unwrap();
        let mut evidence = PruneEvidence::default();
        evidence.tools.insert(
            "mcp__superorchestrator__report_progress".into(),
            tkm_core::prune::ToolEvidence {
                prompts: 20,
                sessions: 5,
                ..Default::default()
            },
        );
        let decision = prune::decide(
            &evidence,
            Host::Claude,
            &session.settings.prune,
            None,
            Timestamp(0),
        );
        assert!(
            decision
                .candidates
                .iter()
                .all(|name| !name.starts_with("mcp__superorchestrator__"))
        );
        assert!(
            decision
                .protected_skipped
                .contains(&"mcp__superorchestrator__report_progress".to_owned())
        );
    }

    #[tokio::test]
    async fn folded_sessions_release_turn_bases() {
        use tkm_proto::{EventKind, PROTOCOL_VERSION, Request};
        let dir = tempfile::tempdir().unwrap();
        let paths = tkm_config::paths::Paths::from_dirs(
            dir.path(),
            Some(&dir.path().join("data")),
            Some(&dir.path().join("config")),
            Some(&dir.path().join("run")),
        );
        let daemon = Daemon::new(crate::daemon::DaemonOptions::new(paths));
        for index in 0..20 {
            let session_id = format!("session-{index}");
            for (event, native) in [
                (EventKind::SessionStart, "SessionStart"),
                (EventKind::SessionEnd, "SessionEnd"),
            ] {
                let request = Request {
                    version: PROTOCOL_VERSION,
                    host: Host::Claude,
                    event,
                    host_event: Some(native.into()),
                    payload: serde_json::json!({"session_id": session_id, "hook_event_name": native, "cwd": "/tmp"}),
                    env: std::collections::BTreeMap::default(),
                    cwd: None,
                };
                daemon.handle(request).await;
            }
            daemon.settle().await;
        }
        assert!(lock(&daemon.state).turn_bases.is_empty());
    }

    #[test]
    fn fact_round_trips_through_the_store_record() {
        let mut params = Params::default();
        params.hosts.insert("example.com".into(), 2);
        let fact = Fact {
            candidate: Candidate {
                id: "boundary:example.com".into(),
                family: Family::Boundary,
                params,
                scope: tkm_core::facts::Scope::Neutral,
                evidence: Vec::new(),
            },
            life: Lifecycle {
                status: Status::Active,
                confirmations: 3,
                contradictions: 1,
                score: 2.0,
                fire_num: 3,
                fire_den: 4,
                contradiction_streak: 1,
            },
        };
        let record = record_from_fact(&RepoKey("r".into()), &fact).unwrap();
        assert_eq!(record.family, "boundary");
        assert_eq!(record.scope, "\"neutral\"");
        let back = fact_from_record(&record).unwrap();
        assert_eq!(back, fact);
    }

    #[test]
    fn claude_log_path_is_derived_from_the_cwd() {
        let path = derived_log_path(
            Host::Claude,
            Path::new("/h/.claude"),
            Path::new("/a/b.c"),
            "s1",
        )
        .unwrap();
        assert_eq!(path, Path::new("/h/.claude/projects/-a-b-c/s1.jsonl"));
    }

    #[test]
    fn discovers_host_session_files_from_fixture_homes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let cwd = root.join("repo");
        let cases = [
            (
                Host::Claude,
                root.join(".claude"),
                root.join(".claude/projects")
                    .join(
                        cwd.to_string_lossy()
                            .replace(|c: char| !c.is_ascii_alphanumeric(), "-"),
                    )
                    .join("s.jsonl"),
            ),
            (
                Host::Codex,
                root.join(".codex"),
                root.join(".codex/sessions/2026/01/02/rollout-2026-s.jsonl"),
            ),
            (
                Host::Agy,
                root.join(".gemini"),
                root.join(
                    ".gemini/antigravity-cli/brain/s/.system_generated/logs/transcript_full.jsonl",
                ),
            ),
            (
                Host::Cursor,
                root.join(".cursor"),
                root.join(".cursor/projects/project/agent-transcripts/s/s.jsonl"),
            ),
            (
                Host::Kimi,
                root.join(".kimi"),
                tkm_hosts::kimi::session_dir(&root.join(".kimi"), cwd.to_str().unwrap(), "s")
                    .join("context.jsonl"),
            ),
            (
                Host::Deepseek,
                root.join(".codewhale"),
                root.join(".codewhale/sessions/s.json"),
            ),
            (
                Host::Opencode,
                root.join(".config/opencode"),
                root.join(".local/share/opencode/opencode.db"),
            ),
            (
                Host::Pi,
                root.join(".pi/agent"),
                root.join(".pi/agent/sessions/--project--/2026_s.jsonl"),
            ),
        ];
        for (host, home, file) in cases {
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, "fixture").unwrap();
            let found = derived_log_path(host, &home, &cwd, "s").unwrap();
            assert_eq!(found, file, "{host:?}");
        }
        assert!(derived_log_path(Host::Codex, &root.join(".codex"), &cwd, "missing").is_none());
        let newer = root.join(".codex/sessions/2026/01/03/rollout-2026-s.jsonl");
        std::fs::create_dir_all(newer.parent().unwrap()).unwrap();
        std::fs::write(&newer, "newer").unwrap();
        assert_eq!(
            derived_log_path(Host::Codex, &root.join(".codex"), &cwd, "s"),
            Some(newer)
        );
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod replay_tests {
    use super::*;
    use tkm_hosts::claude::ClaudeAdapter;
    #[test]
    fn fold_discovers_kimi_fixture_without_hook_path() {
        let scratch = tempfile::tempdir().unwrap();
        let home = scratch.path().join(".kimi");
        let cwd = PathBuf::from("/home/user/project");
        let session_id = "9a1b2c3d-4e5f-6a7b-8c9d-0e1f2a3b4c5d";
        let destination = tkm_hosts::kimi::session_dir(&home, cwd.to_str().unwrap(), session_id);
        std::fs::create_dir_all(&destination).unwrap();
        let fixture = Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../fixtures/kimi/sessions/90722f2638004be06d790eaac9ac1f8a/9a1b2c3d-4e5f-6a7b-8c9d-0e1f2a3b4c5d"
        ));
        for name in ["context.jsonl", "wire.jsonl"] {
            std::fs::copy(fixture.join(name), destination.join(name)).unwrap();
        }
        let store = Store::open_in_memory().unwrap();
        let repo = RepoKey("fixture".into());
        store
            .upsert_repo(&tkm_store::Repo {
                repo_key: repo.clone(),
                display_name: "fixture".into(),
                first_seen: Timestamp(0),
                last_seen: Timestamp(0),
                delegated_runs: 0,
            })
            .unwrap();
        let key = SessionKey {
            host: Host::Kimi,
            agent_home: home.to_string_lossy().into_owned(),
            session_id: session_id.into(),
        };
        let id = store
            .upsert_session(&tkm_store::NewSession {
                key: key.clone(),
                repo_key: repo.clone(),
                cwd: cwd.clone(),
                started: Timestamp(0),
                ended: Some(Timestamp(1)),
                fold_status: "pending".into(),
                shadow: false,
                integration_level: "hooks".into(),
            })
            .unwrap();
        let job = FoldJob {
            key,
            host: Host::Kimi,
            repo,
            cwd,
            agent_home: home,
            log_path: None,
            settings: Arc::new(Settings::default()),
            injected: BTreeSet::new(),
            overhead: (0, Some(0.0)),
            ended_ms: 1,
        };
        let record = store.get_session(id).unwrap().unwrap();
        let report = fold_inner(
            &store,
            &record,
            &tkm_hosts::kimi::KimiAdapter::new(),
            &job,
            None,
        )
        .unwrap();
        assert_eq!(report.status, "folded");
        assert_eq!(
            store.get_session(id).unwrap().unwrap().fold_status,
            "folded"
        );
    }

    #[test]
    fn fold_discovers_every_other_host_without_hook_path() {
        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path();
        let fixtures = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures"));
        let claude = "e928a2cf-e31b-4154-805b-919991d042ea";
        let codex = "01a18562-fb1c-7522-9939-dda89fa83662";
        let agy = "1def34bb-9ae9-4260-99c1-e7f59d3ef14d";
        let cursor = "c1a2b3c4-d5e6-7f8a-9b0c-1d2e3f4a5b6c";
        let codewhale = "550e8400-e29b-41d4-a716-446655440000";
        let pi = "00000000-0000-4000-8000-000000000001";
        let codex_file =
            format!("codex/sessions/2026/10/29/rollout-2026-10-29T09-00-20-{codex}.jsonl");
        let agy_file = format!("agy/brain/{agy}/.system_generated/logs/transcript_full.jsonl");
        let cursor_file = format!("cursor/transcripts/{cursor}.jsonl");
        let codewhale_file = format!("codewhale/sessions/{codewhale}.json");
        let cases = [
            (
                Host::Claude,
                root.join(".claude"),
                claude,
                fixtures
                    .join("claude/sessions")
                    .join(format!("{claude}.jsonl")),
                root.join(".claude/projects/-home-user-project")
                    .join(format!("{claude}.jsonl")),
            ),
            (
                Host::Codex,
                root.join(".codex"),
                codex,
                fixtures.join(&codex_file),
                root.join(".codex/sessions/2026/10/29")
                    .join(Path::new(&codex_file).file_name().unwrap()),
            ),
            (
                Host::Agy,
                root.join(".gemini"),
                agy,
                fixtures.join(&agy_file),
                root.join(".gemini/antigravity-cli/brain")
                    .join(agy)
                    .join(".system_generated/logs/transcript_full.jsonl"),
            ),
            (
                Host::Cursor,
                root.join(".cursor"),
                cursor,
                fixtures.join(&cursor_file),
                root.join(".cursor/projects/project/agent-transcripts")
                    .join(cursor)
                    .join(format!("{cursor}.jsonl")),
            ),
            (
                Host::Deepseek,
                root.join(".codewhale"),
                codewhale,
                fixtures.join(&codewhale_file),
                root.join(".codewhale/sessions")
                    .join(format!("{codewhale}.json")),
            ),
            (
                Host::Pi,
                root.join(".pi/agent"),
                pi,
                PathBuf::from(concat!(
                    env!("CARGO_MANIFEST_DIR"),
                    "/../tkm-hosts/tests/fixtures/pi/session.jsonl"
                )),
                root.join(".pi/agent/sessions/--project--")
                    .join(format!("2026_{pi}.jsonl")),
            ),
        ];
        let registry = crate::registry::AdapterRegistry::standard();
        for (host, home, session_id, fixture, destination) in cases {
            std::fs::create_dir_all(destination.parent().unwrap()).unwrap();
            std::fs::copy(fixture, destination).unwrap();
            assert_fixture_fold(&registry, host, home, session_id);
        }
        let db_path = root.join(".local/share/opencode/opencode.db");
        std::fs::create_dir_all(db_path.parent().unwrap()).unwrap();
        let db = rusqlite::Connection::open(&db_path).unwrap();
        db.execute_batch("PRAGMA foreign_keys = OFF;").unwrap();
        db.execute_batch(include_str!(
            "../../tkm-hosts/tests/fixtures/opencode/session.sql"
        ))
        .unwrap();
        drop(db);
        assert_fixture_fold(
            &registry,
            Host::Opencode,
            root.join(".config/opencode"),
            "ses_example1",
        );
    }

    fn assert_fixture_fold(
        registry: &crate::registry::AdapterRegistry,
        host: Host,
        home: PathBuf,
        session_id: &str,
    ) {
        let store = Store::open_in_memory().unwrap();
        let repo = RepoKey("fixture".into());
        let cwd = PathBuf::from("/home/user/project");
        store
            .upsert_repo(&tkm_store::Repo {
                repo_key: repo.clone(),
                display_name: "fixture".into(),
                first_seen: Timestamp(0),
                last_seen: Timestamp(0),
                delegated_runs: 0,
            })
            .unwrap();
        let key = SessionKey {
            host,
            agent_home: home.to_string_lossy().into_owned(),
            session_id: session_id.into(),
        };
        let id = store
            .upsert_session(&tkm_store::NewSession {
                key: key.clone(),
                repo_key: repo.clone(),
                cwd: cwd.clone(),
                started: Timestamp(0),
                ended: Some(Timestamp(1)),
                fold_status: "pending".into(),
                shadow: false,
                integration_level: "hooks".into(),
            })
            .unwrap();
        let job = FoldJob {
            key,
            host,
            repo,
            cwd,
            agent_home: home,
            log_path: None,
            settings: Arc::new(Settings::default()),
            injected: BTreeSet::new(),
            overhead: (0, Some(0.0)),
            ended_ms: 1,
        };
        let record = store.get_session(id).unwrap().unwrap();
        let adapter = registry.get(host);
        let report = fold_inner(&store, &record, adapter.as_ref(), &job, None).unwrap();
        assert_eq!(report.status, "folded", "{host:?}");
        assert_eq!(
            store.get_session(id).unwrap().unwrap().fold_status,
            "folded",
            "{host:?}"
        );
    }

    #[test]
    fn fold_batches_candidate_and_exercised_fact_questions() {
        let source = SessionSource {
            agent_home: "/fake/agent".into(),
            path: PathBuf::from(concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/tests/fixtures/replay-form-repair.jsonl"
            )),
            session_id: "fixture".into(),
            repo_key: RepoKey("fixture".into()),
            cwd: "/fake/repo".into(),
        };
        let session = ClaudeAdapter::new().read_session(&source).unwrap();
        let mut state = replay_fold(
            &session,
            FactsState::default(),
            &BTreeSet::new(),
            &tkm_core::facts::FactsConfig::default(),
        )
        .facts;
        for fact in state.facts.values_mut() {
            fact.life.status = Status::Active;
        }
        let (filters, contradictions) = prepare_fact_questions(&session, &state, &BTreeSet::new());
        assert!(!filters.is_empty());
        assert!(filters.keys().any(|key| key.starts_with("stable_")));
        assert!(filters.keys().any(|key| key.starts_with("safe_")));
        assert!(
            contradictions
                .keys()
                .all(|key| key.starts_with("contradicts_"))
        );
        assert!(!contradictions.is_empty());
    }

    #[test]
    fn replay_matches_persisted_fixture_fold() {
        let store = Store::open_in_memory().unwrap();
        let repo = RepoKey("fixture".into());
        store
            .upsert_repo(&tkm_store::Repo {
                repo_key: repo.clone(),
                display_name: "fixture".into(),
                first_seen: Timestamp(0),
                last_seen: Timestamp(0),
                delegated_runs: 0,
            })
            .unwrap();
        let key = SessionKey {
            host: Host::Claude,
            agent_home: "/fake/agent".into(),
            session_id: "fixture".into(),
        };
        let path = PathBuf::from(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/fixtures/replay-form-repair.jsonl"
        ));
        let settings = Arc::new(Settings::default());
        let job = FoldJob {
            key: key.clone(),
            host: Host::Claude,
            repo: repo.clone(),
            cwd: "/fake/repo".into(),
            agent_home: "/fake/agent".into(),
            log_path: Some(path.clone()),
            settings: settings.clone(),
            injected: BTreeSet::new(),
            overhead: (0, Some(0.0)),
            ended_ms: 1,
        };
        let id = store
            .upsert_session(&tkm_store::NewSession {
                key: key.clone(),
                repo_key: repo.clone(),
                cwd: job.cwd.clone(),
                started: Timestamp(0),
                ended: Some(Timestamp(1)),
                fold_status: "pending".into(),
                shadow: false,
                integration_level: "hooks".into(),
            })
            .unwrap();
        let source = SessionSource {
            agent_home: job.agent_home.clone(),
            path,
            session_id: key.session_id,
            repo_key: repo.clone(),
            cwd: job.cwd.clone(),
        };
        let session = ClaudeAdapter::new().read_session(&source).unwrap();
        let expected = replay_fold(
            &session,
            FactsState::default(),
            &BTreeSet::new(),
            &settings.facts,
        );
        assert!(
            !expected.events.is_empty(),
            "fixture must train fact lifecycle events"
        );
        assert!(!expected.facts.facts.is_empty());
        let record = store.get_session(id).unwrap().unwrap();
        let actual = fold_inner(&store, &record, &ClaudeAdapter::new(), &job, None).unwrap();
        assert_eq!(actual.events, expected.events);
        let persisted = store.list_facts(&repo, None).unwrap();
        assert_eq!(persisted.len(), expected.facts.facts.len());
        for record in persisted {
            let fact = fact_from_record(&record).unwrap();
            let replayed = &expected.facts.facts[&record.fact_id];
            assert_eq!(fact.life, replayed.life);
            assert_eq!(fact.candidate.params, replayed.candidate.params);
        }
        assert_prune_matches(&store, &repo, &expected.prune);
    }

    fn assert_prune_matches(store: &Store, repo: &RepoKey, expected: &PruneEvidence) {
        assert!(!expected.used_tools.is_empty());
        assert!(!expected.tools.is_empty());
        let rows = store.list_tool_evidence(repo, Host::Claude).unwrap();
        let names: BTreeSet<_> = expected
            .tools
            .keys()
            .chain(expected.used_tools.iter())
            .collect();
        assert_eq!(rows.len(), names.len());
        for row in rows {
            if expected.used_tools.contains(&row.tool) {
                assert_eq!(row.last_used, Some(Timestamp(1)));
                assert_eq!(row.prompts_unused, 0);
                assert_eq!(row.sessions_unused, 0);
            } else {
                let evidence = &expected.tools[&row.tool];
                assert_eq!(u64::from(row.prompts_unused), evidence.prompts);
                assert_eq!(u64::from(row.sessions_unused), evidence.sessions);
                assert_eq!(row.last_used, None);
            }
        }
    }
}

#[cfg(test)]
mod label_tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]
    use super::*;
    use tkm_core::model::{Call, Session, Turn};

    #[test]
    fn fold_labels_prior_facts_and_same_session_relevance_without_labelling_its_own_fact_prediction()
     {
        let store = Store::open_in_memory().unwrap();
        let repo = RepoKey("labels".into());
        store
            .upsert_repo(&tkm_store::Repo {
                repo_key: repo.clone(),
                display_name: "labels".into(),
                first_seen: Timestamp(0),
                last_seen: Timestamp(200),
                delegated_runs: 0,
            })
            .unwrap();
        let mut ids = Vec::new();
        for (name, start) in [("prior", 0), ("current", 200)] {
            ids.push(
                store
                    .upsert_session(&tkm_store::NewSession {
                        key: SessionKey {
                            host: Host::Claude,
                            agent_home: "/agent".into(),
                            session_id: name.into(),
                        },
                        repo_key: repo.clone(),
                        cwd: "/repo".into(),
                        started: Timestamp(start),
                        ended: Some(Timestamp(start + 99)),
                        fold_status: "pending".into(),
                        shadow: true,
                        integration_level: "hooks".into(),
                    })
                    .unwrap(),
            );
        }
        let current = store.get_session(ids[1]).unwrap().unwrap();
        let mut decisions = Vec::new();
        for (purpose, key, session, at) in [
            ("fact_filter", "stable_repair", ids[0], 100),
            ("fact_contradiction", "contradicts_repair", ids[0], 100),
            ("relevance", "rel_repair", ids[1], 201),
            ("fact_filter", "stable_repair", ids[1], 299),
        ] {
            decisions.push(
                store
                    .insert_decision_with_context(
                        &tkm_store::NewDecision {
                            model: "model".into(),
                            purpose: purpose.into(),
                            questions: serde_json::json!({key:{"type":"noul"}}),
                            answers: serde_json::json!({}),
                            cost_usd: Some(0.0),
                            latency_ms: 0,
                            shadow: true,
                        },
                        Some(Timestamp(at)),
                        Some(&repo),
                        Some(session),
                        None,
                    )
                    .unwrap(),
            );
        }
        let mut session = Session::new(Host::Claude);
        session.session_id = "current".into();
        session.turns.push(Turn {
            stopped_by: StopReason::OwnStop,
            calls: vec![Call {
                tool: "Bash".into(),
                args_digest: "python -m pytest".into(),
                output_digest: "10 passed".into(),
                exit_code: Some(0),
                ..Call::default()
            }],
            ..Turn::default()
        });
        let state = FactsState {
            facts: BTreeMap::from([(
                "repair".into(),
                Fact {
                    candidate: Candidate {
                        id: "repair".into(),
                        family: facts::Family::FormRepair,
                        params: Params {
                            intent: Some(facts::observe::Intent::RunTests),
                            bad_form: "pytest".into(),
                            forms: BTreeMap::from([("python -m pytest".into(), 3)]),
                            ..Params::default()
                        },
                        scope: facts::Scope::Neutral,
                        evidence: vec![],
                    },
                    life: Lifecycle::default(),
                },
            )]),
            ..FactsState::default()
        };
        label_decisions(&store, &current, &session, &state, &BTreeSet::new(), &[]).unwrap();
        for (index, key, expected) in [
            (0, "stable_repair", true),
            (1, "contradicts_repair", false),
            (2, "rel_repair", true),
        ] {
            let row = store.get_decision(decisions[index]).unwrap().unwrap();
            let label: serde_json::Value =
                serde_json::from_str(row.outcome_label.as_deref().unwrap()).unwrap();
            assert_eq!(label[key], expected);
        }
        assert!(
            store
                .get_decision(decisions[3])
                .unwrap()
                .unwrap()
                .outcome_label
                .is_none()
        );
        let verdict = FactEvent {
            fact_id: "repair".into(),
            session_id: "current".into(),
            kind: FactEventKind::Contradicted,
        };
        label_decisions(
            &store,
            &current,
            &session,
            &state,
            &BTreeSet::new(),
            &[verdict],
        )
        .unwrap();
        let row = store.get_decision(decisions[0]).unwrap().unwrap();
        let label: serde_json::Value =
            serde_json::from_str(row.outcome_label.as_deref().unwrap()).unwrap();
        assert_eq!(label["stable_repair"], false);
    }
}
