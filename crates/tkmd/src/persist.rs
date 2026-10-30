//! The outbox: state changes queued by handlers and written by the flusher (SPEC 14).
//!
//! Handlers push plain values here (memory only). The flusher swaps the whole batch out every
//! 5 s and at turn end, and applies it on a blocking task. A failed apply keeps the unapplied
//! rest in the outbox, so a locked database delays rows but never loses them.

use std::collections::{HashMap, VecDeque};
use std::sync::{
    Arc,
    atomic::{AtomicI64, Ordering},
};

use tkm_core::model::{Host, RepoKey, Timestamp};
use tkm_store::{
    CalibrationRecord, NewDecision, NewEvent, NewSession, Repo, SessionKey, Store, StoreError,
    TurnRecord,
};

/// Maximum queued items per kind; the oldest are dropped beyond this (daemon-side backstop).
const MAX_QUEUED: usize = 20_000;

// Internal outbox operation: refresh a coverage row instead of appending another event.
pub(crate) const HOOK_ACTIVITY: &str = "hook.activity";

/// Session row to insert or update.
#[derive(Debug, Clone)]
pub struct SessionUpsert {
    /// Natural key.
    pub key: SessionKey,
    /// Repository identity.
    pub repo_key: RepoKey,
    /// Working directory.
    pub cwd: std::path::PathBuf,
    /// Start time.
    pub started: Timestamp,
    /// End time, once the session has ended.
    pub ended: Option<Timestamp>,
    /// `open`, `pending`, then `folded`, `partial`, `skipped` or `error` (set by the fold).
    pub fold_status: String,
    /// True when any loop action of the session was shadowed.
    pub shadow: bool,
    /// Integration level, `hooks` for hook-driven sessions.
    pub integration_level: String,
}

/// Turn row to insert or update.
#[derive(Debug, Clone)]
pub struct TurnUpsert {
    /// Owning session.
    pub key: SessionKey,
    /// Position within the session.
    pub turn_idx: i64,
    /// Tool calls.
    pub calls: i64,
    /// Whether the turn ended well.
    pub ended_well: bool,
    /// Whether the turn escalated.
    pub escalated: bool,
}

/// Event row to append, or an internal hook-activity timestamp refresh.
#[derive(Debug, Clone)]
pub struct EventInsert {
    /// Owning session.
    pub key: SessionKey,
    /// Action kind, for example `loop.redirect`.
    pub kind: String,
    /// Inputs as JSON.
    pub inputs: serde_json::Value,
    /// Explanation template identifier.
    pub template: String,
    /// Explanation parameters as JSON.
    pub params: serde_json::Value,
    /// Daemon-clock action time, Unix milliseconds.
    pub created_at: Timestamp,
    /// Persisted decision identifiers supplied by the action producer.
    pub evidence_ids: Vec<i64>,
    /// Exact decision row ID, filled by the flusher after that decision is persisted.
    pub decision_id: Option<Arc<AtomicI64>>,
}

/// Decision plus the request context captured before sending it.
#[derive(Debug, Clone)]
pub struct DecisionInsert {
    /// Shared evidence link; zero until this row has been persisted.
    pub evidence_id: Arc<AtomicI64>,
    /// Priced model answers.
    pub decision: NewDecision,
    /// Daemon-clock completion time.
    pub created_at: Timestamp,
    /// Repository when known.
    pub repo: Option<RepoKey>,
    /// Natural session key when known.
    pub key: Option<SessionKey>,
    /// Redacted request state exactly as sent.
    pub digest: Option<String>,
}

/// Outcome waiting for its decision ID to be persisted.
#[derive(Debug, Clone)]
pub struct DecisionOutcome {
    /// Decision identifier populated by the flusher.
    pub decision_id: Arc<AtomicI64>,
    /// Outcome classification.
    pub label: &'static str,
}

/// One good-turn length for the envelope ring.
#[derive(Debug, Clone)]
pub struct EnvelopeSample {
    /// Repository.
    pub repo: RepoKey,
    /// Harness.
    pub host: Host,
    /// Tool calls in the good turn.
    pub calls: u32,
}

/// A delegated session observed by the daemon, persisted through the retrying outbox.
#[derive(Debug, Clone)]
pub struct DelegatedRun {
    /// Repository identity shared across its worktrees.
    pub repo: RepoKey,
    /// Natural session key used for deduplication.
    pub key: SessionKey,
    /// First observed start time.
    pub at: Timestamp,
}

/// Everything waiting to be written.
#[derive(Debug, Default)]
pub struct FlushBatch {
    /// Repository rows (must precede sessions: foreign key).
    pub repos: VecDeque<Repo>,
    /// Session rows.
    pub sessions: VecDeque<SessionUpsert>,
    /// Timestamped delegated sessions.
    pub delegated: VecDeque<DelegatedRun>,
    /// Turn rows.
    pub turns: VecDeque<TurnUpsert>,
    /// Event rows.
    pub events: VecDeque<EventInsert>,
    /// Decision rows.
    pub decisions: VecDeque<DecisionInsert>,
    /// Observed decision outcomes.
    pub outcomes: VecDeque<DecisionOutcome>,
    /// Envelope samples.
    pub envelope: VecDeque<EnvelopeSample>,
    /// Calibration rows.
    pub calibration: VecDeque<CalibrationRecord>,
}

impl FlushBatch {
    /// True when nothing is queued.
    pub fn is_empty(&self) -> bool {
        self.repos.is_empty()
            && self.sessions.is_empty()
            && self.delegated.is_empty()
            && self.turns.is_empty()
            && self.events.is_empty()
            && self.decisions.is_empty()
            && self.outcomes.is_empty()
            && self.envelope.is_empty()
            && self.calibration.is_empty()
    }

    /// Puts the unapplied remainder of `rest` back in front of whatever was queued meanwhile.
    pub fn requeue(&mut self, mut rest: FlushBatch) -> usize {
        let mut dropped = 0;
        macro_rules! merge {
            ($field:ident) => {
                rest.$field.append(&mut self.$field);
                while rest.$field.len() > MAX_QUEUED {
                    rest.$field.pop_front();
                    dropped += 1;
                }
                self.$field = std::mem::take(&mut rest.$field);
            };
        }
        merge!(repos);
        merge!(sessions);
        merge!(delegated);
        merge!(turns);
        merge!(events);
        merge!(decisions);
        merge!(outcomes);
        merge!(envelope);
        merge!(calibration);
        dropped
    }

    /// Queues a delegated observation once per repository and session.
    pub fn push_delegated(&mut self, run: DelegatedRun) {
        if !self
            .delegated
            .iter()
            .any(|queued| queued.repo == run.repo && queued.key == run.key)
        {
            self.delegated.push_back(run);
        }
    }

    /// Queues a session upsert, replacing an older queued upsert of the same session.
    pub fn push_session(&mut self, upsert: SessionUpsert) {
        self.sessions.retain(|queued| queued.key != upsert.key);
        self.sessions.push_back(upsert);
    }

    /// Queues a turn upsert, replacing an older queued upsert of the same turn.
    pub fn push_turn(&mut self, upsert: TurnUpsert) {
        self.turns
            .retain(|queued| !(queued.key == upsert.key && queued.turn_idx == upsert.turn_idx));
        self.turns.push_back(upsert);
    }

    /// Queues a repository upsert once per repository.
    pub fn push_repo(&mut self, repo: Repo) {
        self.repos.retain(|queued| queued.repo_key != repo.repo_key);
        self.repos.push_back(repo);
    }
}

type SessionIds = HashMap<(Host, String, String), i64>;

fn session_id(
    store: &Store,
    ids: &mut SessionIds,
    key: &SessionKey,
) -> Result<Option<i64>, StoreError> {
    let tuple = (key.host, key.agent_home.clone(), key.session_id.clone());
    if let Some(id) = ids.get(&tuple) {
        return Ok(Some(*id));
    }
    let found = store.find_session(key)?.map(|record| record.id);
    if let Some(id) = found {
        ids.insert(tuple, id);
    }
    Ok(found)
}

/// Applies `batch` in dependency order. Items are removed as they succeed, so on error the batch
/// holds exactly what is still unwritten.
pub fn apply(store: &Store, batch: &mut FlushBatch) -> Result<(), StoreError> {
    let mut ids = SessionIds::new();
    while let Some(repo) = batch.repos.front() {
        store.upsert_repo(repo)?;
        batch.repos.pop_front();
    }
    while let Some(session) = batch.sessions.front() {
        let id = store.upsert_session(&NewSession {
            key: session.key.clone(),
            repo_key: session.repo_key.clone(),
            cwd: session.cwd.clone(),
            started: session.started,
            ended: session.ended,
            fold_status: session.fold_status.clone(),
            shadow: session.shadow,
            integration_level: session.integration_level.clone(),
        })?;
        ids.insert(
            (
                session.key.host,
                session.key.agent_home.clone(),
                session.key.session_id.clone(),
            ),
            id,
        );
        batch.sessions.pop_front();
    }
    while let Some(run) = batch.delegated.front() {
        store.record_delegated_run(&run.repo, &run.key, run.at)?;
        batch.delegated.pop_front();
    }
    for _ in 0..batch.turns.len() {
        let Some(turn) = batch.turns.front() else {
            break;
        };
        if let Some(id) = session_id(store, &mut ids, &turn.key)? {
            store.upsert_turn(&TurnRecord {
                session_id: id,
                turn_idx: turn.turn_idx,
                calls: turn.calls,
                ended_well: turn.ended_well,
                escalated: turn.escalated,
                digest_hash: None,
            })?;
            batch.turns.pop_front();
        } else if let Some(turn) = batch.turns.pop_front() {
            batch.turns.push_back(turn);
        }
    }
    for _ in 0..batch.decisions.len() {
        let Some(item) = batch.decisions.front() else {
            break;
        };
        let session = match &item.key {
            Some(key) => session_id(store, &mut ids, key)?,
            None => None,
        };
        if item.key.is_some() && session.is_none() {
            if let Some(item) = batch.decisions.pop_front() {
                batch.decisions.push_back(item);
            }
            continue;
        }
        let id = store.insert_decision_with_context(
            &item.decision,
            Some(item.created_at),
            item.repo.as_ref(),
            session,
            item.digest.as_deref(),
        )?;
        item.evidence_id.store(id, Ordering::Release);
        batch.decisions.pop_front();
    }
    for _ in 0..batch.outcomes.len() {
        let Some(outcome) = batch.outcomes.front() else {
            break;
        };
        let id = outcome.decision_id.load(Ordering::Acquire);
        if id == 0 {
            if let Some(outcome) = batch.outcomes.pop_front() {
                batch.outcomes.push_back(outcome);
            }
            continue;
        }
        store.set_decision_outcome(id, outcome.label)?;
        batch.outcomes.pop_front();
    }
    for _ in 0..batch.events.len() {
        let Some(event) = batch.events.front() else {
            break;
        };
        if let Some(id) = session_id(store, &mut ids, &event.key)? {
            if event.kind == HOOK_ACTIVITY {
                store.refresh_hook_observation(id, event.created_at)?;
                batch.events.pop_front();
                continue;
            }
            let mut evidence_ids = event.evidence_ids.clone();
            if let Some(link) = &event.decision_id {
                let id = link.load(Ordering::Acquire);
                if id > 0 && !evidence_ids.contains(&id) {
                    evidence_ids.push(id);
                }
            }
            let mut inputs = event.inputs.clone();
            if !inputs.is_object() {
                inputs = serde_json::json!({"action_inputs":inputs});
            }
            inputs["decision_ids"] = serde_json::json!(evidence_ids);
            if let Some(facts) = event
                .inputs
                .get("fact_ids")
                .and_then(serde_json::Value::as_array)
                && let Some(session) = store.get_session(id)?
            {
                for fact in facts.iter().filter_map(serde_json::Value::as_str) {
                    if let Some(rowid) = store.fact_evidence_id(&session.repo_key, fact)? {
                        evidence_ids.push(rowid);
                    }
                }
            }
            store.insert_event_at(
                &NewEvent {
                    kind: event.kind.clone(),
                    session_id: id,
                    inputs,
                    evidence_ids,
                    explanation_template_id: event.template.clone(),
                    params: event.params.clone(),
                },
                event.created_at,
            )?;
            batch.events.pop_front();
        } else if let Some(event) = batch.events.pop_front() {
            batch.events.push_back(event);
        }
    }
    while let Some(sample) = batch.envelope.front() {
        store.push_envelope_sample(&sample.repo, sample.host, sample.calls)?;
        batch.envelope.pop_front();
    }
    while let Some(record) = batch.calibration.front() {
        store.upsert_calibration(record)?;
        batch.calibration.pop_front();
    }
    Ok(())
}

/// Persist the promotable/retired fact actions emitted by a complete fold.
pub(crate) fn fact_actions(
    store: &Store,
    repo: &RepoKey,
    session_id: i64,
    at: Timestamp,
    events: &[tkm_core::facts::FactEvent],
) -> Result<(), StoreError> {
    for event in events {
        let template = match event.kind {
            tkm_core::facts::FactEventKind::Promoted => "fact_promoted",
            tkm_core::facts::FactEventKind::Retired => "fact_retired",
            _ => continue,
        };
        let Some(rowid) = store.fact_evidence_id(repo, &event.fact_id)? else {
            continue;
        };
        let fact = store.get_fact(repo, &event.fact_id)?;
        let detail = fact
            .as_ref()
            .and_then(crate::fold::fact_from_record)
            .map_or_else(
                || event.fact_id.clone(),
                |f| tkm_core::facts::render::claim(&f.candidate),
            );
        store.insert_event_at(&NewEvent {kind:template.replace('_',"."),session_id,inputs:serde_json::json!({"fact_ids":[event.fact_id],"decision_ids":[]}),evidence_ids:vec![rowid],explanation_template_id:template.into(),params:serde_json::json!({"detail":detail,"cost":"$0","undo":if template=="fact_promoted" {format!("tkm facts forget {}",event.fact_id)} else {"no undo action is available".into()}})},at)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(id: &str) -> SessionKey {
        SessionKey {
            host: Host::Claude,
            agent_home: "/h".into(),
            session_id: id.into(),
        }
    }

    fn session(id: &str, status: &str) -> SessionUpsert {
        SessionUpsert {
            key: key(id),
            repo_key: RepoKey("r".into()),
            cwd: "/c".into(),
            started: Timestamp(1),
            ended: None,
            fold_status: status.into(),
            shadow: false,
            integration_level: "hooks".into(),
        }
    }

    #[test]
    fn action_event_has_timestamp_and_real_decision_evidence() {
        let store = Store::open_in_memory().unwrap();
        let mut batch = FlushBatch::default();
        batch.push_repo(Repo {
            repo_key: RepoKey("r".into()),
            display_name: "r".into(),
            first_seen: Timestamp(1),
            last_seen: Timestamp(1),
            delegated_runs: 0,
        });
        batch.push_session(session("a", "open"));
        let evidence = Arc::new(AtomicI64::new(0));
        batch.decisions.push_back(DecisionInsert {
            evidence_id: evidence.clone(),
            decision: NewDecision {
                model: "fixture".into(),
                purpose: "loop_guard".into(),
                questions: serde_json::json!({}),
                answers: serde_json::json!({}),
                cost_usd: Some(0.0),
                latency_ms: 1,
                shadow: false,
            },
            created_at: Timestamp(42),
            repo: Some(RepoKey("r".into())),
            key: Some(key("a")),
            digest: Some("redacted digest".into()),
        });
        let mut unrelated = batch.decisions.front().unwrap().clone();
        unrelated.evidence_id = Arc::new(AtomicI64::new(0));
        unrelated.decision.model = "unrelated-later-decision".into();
        apply(&store, &mut batch).unwrap(); // The action arrives in a later flush.
        batch.decisions.push_back(unrelated);
        batch.events.push_back(EventInsert {
            key: key("a"),
            kind: "loop.redirect".into(),
            inputs: serde_json::json!({}),
            template: "redirect".into(),
            params: serde_json::json!({}),
            created_at: Timestamp(43),
            evidence_ids: vec![],
            decision_id: Some(evidence),
        });
        apply(&store, &mut batch).unwrap();
        let event = store.list_recent_events(1).unwrap().remove(0);
        assert_eq!(event.created_at, Some(Timestamp(43)));
        assert_eq!(event.evidence_ids.len(), 1);
        let decision = store.get_decision(event.evidence_ids[0]).unwrap().unwrap();
        assert_eq!(decision.created_at, Some(Timestamp(42)));
        assert_eq!(decision.model, "fixture");
        assert_eq!(decision.repo_key, Some(RepoKey("r".into())));
        assert_eq!(decision.session_id, Some(event.session_id));
        assert_eq!(decision.digest.as_deref(), Some("redacted digest"));
    }

    #[test]
    fn fold_fact_actions_have_real_fact_evidence_and_time() {
        let store = Store::open_in_memory().unwrap();
        store
            .upsert_repo(&Repo {
                repo_key: RepoKey("r".into()),
                display_name: "r".into(),
                first_seen: Timestamp(1),
                last_seen: Timestamp(1),
                delegated_runs: 0,
            })
            .unwrap();
        let id = store
            .upsert_session(&NewSession {
                key: key("a"),
                repo_key: RepoKey("r".into()),
                cwd: "/c".into(),
                started: Timestamp(1),
                ended: None,
                fold_status: "open".into(),
                shadow: false,
                integration_level: "hooks".into(),
            })
            .unwrap();
        store
            .insert_candidate_fact(&tkm_store::FactRecord {
                repo_key: RepoKey("r".into()),
                fact_id: "f-17".into(),
                family: "empty_probe".into(),
                scope: "neutral".into(),
                text_params: serde_json::json!({}),
                state: tkm_store::FactState::Candidate,
                score: 1.0,
                confirmations: 3,
                contradictions: 0,
                fire_count: 4,
                fire_sessions: 5,
                state_changed_at: None,
            })
            .unwrap();
        let events = [tkm_core::facts::FactEvent {
            fact_id: "f-17".into(),
            session_id: "a".into(),
            kind: tkm_core::facts::FactEventKind::Promoted,
        }];
        fact_actions(&store, &RepoKey("r".into()), id, Timestamp(42), &events).unwrap();
        let row = store.list_recent_events(1).unwrap().remove(0);
        assert_eq!(row.created_at, Some(Timestamp(42)));
        assert_eq!(
            row.evidence_ids,
            vec![
                store
                    .fact_evidence_id(&RepoKey("r".into()), "f-17")
                    .unwrap()
                    .unwrap()
            ]
        );
        assert_eq!(row.inputs["fact_ids"], serde_json::json!(["f-17"]));
    }

    #[test]
    fn newer_session_upsert_replaces_the_queued_one() {
        let mut batch = FlushBatch::default();
        batch.push_session(session("a", "open"));
        batch.push_session(session("a", "pending"));
        assert_eq!(batch.sessions.len(), 1);
        assert_eq!(batch.sessions[0].fold_status, "pending");
    }

    #[test]
    fn apply_writes_in_dependency_order_and_drains_the_batch() {
        let store = Store::open_in_memory().unwrap();
        let mut batch = FlushBatch::default();
        batch.push_repo(Repo {
            repo_key: RepoKey("r".into()),
            display_name: "r".into(),
            first_seen: Timestamp(1),
            last_seen: Timestamp(1),
            delegated_runs: 0,
        });
        batch.push_session(session("a", "open"));
        batch.push_turn(TurnUpsert {
            key: key("a"),
            turn_idx: 0,
            calls: 3,
            ended_well: true,
            escalated: false,
        });
        batch.events.push_back(EventInsert {
            key: key("a"),
            kind: "loop.redirect".into(),
            inputs: serde_json::json!({}),
            template: "redirect".into(),
            params: serde_json::json!({}),
            created_at: Timestamp(1),
            evidence_ids: Vec::new(),
            decision_id: None,
        });
        batch.envelope.push_back(EnvelopeSample {
            repo: RepoKey("r".into()),
            host: Host::Claude,
            calls: 3,
        });
        apply(&store, &mut batch).unwrap();
        assert!(batch.is_empty());
        let record = store.find_session(&key("a")).unwrap().unwrap();
        assert_eq!(store.list_turns(record.id).unwrap().len(), 1);
        assert_eq!(store.list_events_by_session(record.id).unwrap().len(), 1);
    }

    #[test]
    fn unresolved_session_rows_are_retained_until_session_exists() {
        let store = Store::open_in_memory().unwrap();
        let mut batch = FlushBatch::default();
        batch.push_turn(TurnUpsert {
            key: key("late"),
            turn_idx: 0,
            calls: 1,
            ended_well: false,
            escalated: false,
        });
        batch.events.push_back(EventInsert {
            key: key("late"),
            kind: "loop.redirect".into(),
            inputs: serde_json::json!({}),
            template: "redirect".into(),
            params: serde_json::json!({}),
            created_at: Timestamp(1),
            evidence_ids: Vec::new(),
            decision_id: None,
        });
        apply(&store, &mut batch).unwrap();
        assert_eq!(batch.turns.len(), 1);
        assert_eq!(batch.events.len(), 1);
        batch.push_repo(Repo {
            repo_key: RepoKey("r".into()),
            display_name: "r".into(),
            first_seen: Timestamp(1),
            last_seen: Timestamp(1),
            delegated_runs: 0,
        });
        batch.push_session(session("late", "open"));
        apply(&store, &mut batch).unwrap();
        assert!(batch.is_empty());
        let id = store.find_session(&key("late")).unwrap().unwrap().id;
        assert_eq!(store.list_turns(id).unwrap().len(), 1);
        assert_eq!(store.list_events_by_session(id).unwrap().len(), 1);
    }

    #[test]
    fn requeue_keeps_unapplied_items_before_newer_ones() {
        let mut current = FlushBatch::default();
        current.envelope.push_back(EnvelopeSample {
            repo: RepoKey("r".into()),
            host: Host::Claude,
            calls: 2,
        });
        let mut rest = FlushBatch::default();
        rest.envelope.push_back(EnvelopeSample {
            repo: RepoKey("r".into()),
            host: Host::Claude,
            calls: 1,
        });
        current.requeue(rest);
        let calls: Vec<u32> = current.envelope.iter().map(|s| s.calls).collect();
        assert_eq!(calls, vec![1, 2]);
    }
}

#[cfg(test)]
mod delegated_prune_tests {
    use super::*;

    #[test]
    fn delegated_outbox_retries_without_double_counting() {
        let store = Store::open_in_memory().unwrap();
        let repo = RepoKey("delegation".into());
        let key = SessionKey {
            host: Host::Claude,
            agent_home: "test".into(),
            session_id: "run".into(),
        };
        let mut batch = FlushBatch::default();
        batch.push_delegated(DelegatedRun {
            repo: repo.clone(),
            key: key.clone(),
            at: Timestamp(500),
        });
        assert!(apply(&store, &mut batch).is_err());
        assert!(!batch.is_empty());
        let mut queued = FlushBatch::default();
        queued.requeue(batch);
        queued.push_repo(Repo {
            repo_key: repo.clone(),
            display_name: "test".into(),
            first_seen: Timestamp(0),
            last_seen: Timestamp(0),
            delegated_runs: 0,
        });
        apply(&store, &mut queued).unwrap();
        assert!(queued.is_empty());
        queued.push_delegated(DelegatedRun {
            repo: repo.clone(),
            key,
            at: Timestamp(600),
        });
        apply(&store, &mut queued).unwrap();
        assert_eq!(store.get_repo(&repo).unwrap().unwrap().delegated_runs, 1);
        assert_eq!(
            store
                .last_delegated_run_within(&repo, Timestamp(700))
                .unwrap(),
            Some(Timestamp(500))
        );
    }
}
