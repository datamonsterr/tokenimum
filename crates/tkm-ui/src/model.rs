//! In-memory monitor data and initial read-only database snapshot.
use rusqlite::Connection;
use serde::Serialize;
use std::collections::VecDeque;
use tkm_proto::monitor::{
    DecisionNotice, MonitorMessage, RedirectNotice, SessionUpdate, SpendUpdate,
};

/// Maximum retained decisions and redirects.
pub const HISTORY_LIMIT: usize = 1_000;
/// A fact, including scores that may be unavailable in live notices.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Fact {
    /// Repository identifier.
    pub repo: String,
    /// Fact identifier.
    pub fact_id: String,
    /// Lifecycle state.
    pub state: String,
    /// Last stored score.
    pub score: Option<f64>,
    /// Number of confirming sessions.
    pub confirmations: Option<u32>,
}
/// A session cost observed in the ledger.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Savings {
    /// Repository identifier.
    pub repo: String,
    /// Harness name.
    pub host: String,
    /// Session cost.
    pub cost: f64,
    /// Whether the session was shadowed (a baseline sample).
    pub shadow: bool,
}
/// The shared, deterministic monitor model.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct MonitorState {
    /// Whether the live state is awaiting a successful resynchronization.
    pub degraded: bool,
    /// Session updates.
    pub sessions: Vec<SessionUpdate>,
    /// Latest stored event ID for each session, when known.
    pub session_event_ids: Vec<Option<i64>>,
    /// Recent decisions, oldest first.
    pub decisions: VecDeque<DecisionNotice>,
    /// Stored decision IDs; live notices do not contain IDs.
    pub decision_ids: VecDeque<Option<i64>>,
    /// Latest live spend and caps, when available.
    pub spend: Option<SpendUpdate>,
    /// Daily ledger totals for the seven most recent days.
    pub spend_days: Vec<(String, f64)>,
    /// Ledger cost samples.
    pub savings: Vec<Savings>,
    /// Facts and their lifecycle states.
    pub facts: Vec<Fact>,
    #[serde(skip)]
    live_fact_keys: VecDeque<(String, String)>,
    /// Recent redirects, oldest first.
    pub redirects: VecDeque<RedirectNotice>,
    /// Database event IDs corresponding to redirect rows.
    pub redirect_event_ids: VecDeque<Option<i64>>,
}
fn push_bounded<T>(items: &mut VecDeque<T>, item: T) {
    if items.len() >= HISTORY_LIMIT {
        items.pop_front();
    }
    items.push_back(item);
}
impl MonitorState {
    /// Apply a message and refresh ledger-derived data after a folded session.
    ///
    /// # Errors
    /// Returns the loader error without applying the message.
    pub fn apply_with_loader<E>(
        &mut self,
        message: MonitorMessage,
        load: impl FnOnce() -> Result<(Vec<Savings>, Vec<(String, f64)>), E>,
    ) -> Result<(), E> {
        if matches!(&message, MonitorMessage::Session(update) if update.phase == "folded") {
            let (savings, spend_days) = load()?;
            self.savings = savings;
            self.spend_days = spend_days;
        }
        self.apply(message);
        Ok(())
    }
    /// Apply a daemon message without performing I/O or consulting a clock.
    pub fn apply(&mut self, message: MonitorMessage) {
        match message {
            MonitorMessage::Session(update) => {
                if matches!(update.phase.as_str(), "ended" | "folded") {
                    if let Some(index) = self
                        .sessions
                        .iter()
                        .position(|s| s.host == update.host && s.session_id == update.session_id)
                    {
                        self.sessions.remove(index);
                        self.session_event_ids.remove(index);
                    }
                    return;
                }
                if let Some(old) = self
                    .sessions
                    .iter_mut()
                    .find(|s| s.host == update.host && s.session_id == update.session_id)
                {
                    *old = update;
                } else {
                    self.sessions.push(update);
                    self.session_event_ids.push(None);
                }
            }
            MonitorMessage::Decision(update) => {
                push_bounded(&mut self.decisions, update);
                push_bounded(&mut self.decision_ids, None);
            }
            MonitorMessage::Spend(update) => self.spend = Some(update),
            MonitorMessage::Fact(update) => {
                let state = match update.kind.as_str() {
                    "promoted" => "active",
                    "retired" => "retired",
                    _ => "candidate",
                };
                if let Some(fact) = self
                    .facts
                    .iter_mut()
                    .find(|f| f.repo == update.repo && f.fact_id == update.fact_id)
                {
                    if matches!(update.kind.as_str(), "promoted" | "retired") {
                        fact.state = state.into();
                    }
                } else {
                    if self.live_fact_keys.len() >= 2_000
                        && let Some((repo, fact_id)) = self.live_fact_keys.pop_front()
                    {
                        self.facts
                            .retain(|fact| fact.repo != repo || fact.fact_id != fact_id);
                    }
                    self.live_fact_keys
                        .push_back((update.repo.clone(), update.fact_id.clone()));
                    self.facts.push(Fact {
                        repo: update.repo,
                        fact_id: update.fact_id,
                        state: state.into(),
                        score: None,
                        confirmations: None,
                    });
                }
            }
            MonitorMessage::Redirect(update) => {
                push_bounded(&mut self.redirects, update);
                push_bounded(&mut self.redirect_event_ids, None);
            }
            _ => {}
        }
    }
}
/// Read a snapshot from an existing connection, normally opened with `tkm_store::open_read_only`.
///
/// # Errors
/// Returns an error if SQLite or a stored JSON value cannot be read.
pub fn load_snapshot(conn: &Connection) -> Result<MonitorState, crate::Error> {
    let mut state = MonitorState::default();
    let mut query = conn.prepare("SELECT s.host,s.session_id,s.repo_key,s.fold_status,COALESCE((SELECT calls FROM turns WHERE session_id=s.id ORDER BY turn_idx DESC LIMIT 1),0),(SELECT id FROM events WHERE session_id=s.id ORDER BY id DESC LIMIT 1) FROM sessions s WHERE s.ended_at IS NULL ORDER BY s.id")?;
    for row in query.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, u32>(4)?,
            row.get::<_, Option<i64>>(5)?,
        ))
    })? {
        let (host, session_id, repo, phase, turn_calls, event_id) = row?;
        if let Ok(host) = host.parse() {
            state.sessions.push(SessionUpdate {
                host,
                session_id,
                repo: Some(repo),
                phase,
                turn_calls,
                envelope: None,
                loop_state: "unknown".into(),
            });
            state.session_event_ids.push(event_id);
        }
    }
    let mut query = conn.prepare("SELECT purpose,model,latency_ms,cost,shadow,answers,id FROM (SELECT * FROM decisions ORDER BY id DESC LIMIT 1000) ORDER BY id")?;
    for row in query.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            {
                let value: i64 = row.get(2)?;
                u64::try_from(value)
                    .map_err(|_| rusqlite::Error::IntegralValueOutOfRange(2, value))?
            },
            row.get::<_, f64>(3)?,
            row.get::<_, bool>(4)?,
            row.get::<_, String>(5)?,
            row.get::<_, i64>(6)?,
        ))
    })? {
        let (purpose, model, latency_ms, cost_usd, shadow, answers, id) = row?;
        state.decisions.push_back(DecisionNotice {
            purpose,
            model,
            latency_ms,
            cost_usd,
            shadow,
            answers: serde_json::from_str(&answers)?,
        });
        state.decision_ids.push_back(Some(id));
    }
    let mut query = conn.prepare(
        "SELECT repo_key,fact_id,state,score,confirmations FROM facts WHERE state IN ('active','candidate') OR rowid IN (SELECT rowid FROM facts WHERE state='retired' ORDER BY rowid DESC LIMIT 100) ORDER BY repo_key,fact_id",
    )?;
    state.facts = query
        .query_map([], |row| {
            Ok(Fact {
                repo: row.get(0)?,
                fact_id: row.get(1)?,
                state: row.get(2)?,
                score: Some(row.get(3)?),
                confirmations: Some(row.get(4)?),
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    let mut query = conn.prepare("SELECT repo_key,host,priced_cost,shadow FROM (SELECT s.repo_key,s.host,l.priced_cost,s.shadow,s.started_at FROM ledger l JOIN sessions s ON s.id=l.session_id ORDER BY s.started_at DESC LIMIT 1000) ORDER BY started_at")?;
    state.savings = query
        .query_map([], |row| {
            Ok(Savings {
                repo: row.get(0)?,
                host: row.get(1)?,
                cost: row.get(2)?,
                shadow: row.get(3)?,
            })
        })?
        .collect::<rusqlite::Result<_>>()?;
    let mut query = conn.prepare("SELECT day, cost FROM (SELECT date(s.started_at / 1000, 'unixepoch') AS day, sum(l.priced_cost) AS cost FROM ledger l JOIN sessions s ON s.id=l.session_id GROUP BY day ORDER BY day DESC LIMIT 7) ORDER BY day")?;
    state.spend_days = query
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?;
    load_redirects(conn, &mut state)?;
    Ok(state)
}

fn load_redirects(conn: &Connection, state: &mut MonitorState) -> Result<(), crate::Error> {
    let mut query = conn.prepare("SELECT e.id,s.host,s.session_id,e.kind,e.params,e.inputs FROM (SELECT * FROM events WHERE kind IN ('loop.redirect','loop.escalation') ORDER BY id DESC LIMIT 1000) e JOIN sessions s ON s.id=e.session_id ORDER BY e.id")?;
    for row in query.query_map([], |row| {
        Ok((
            row.get::<_, i64>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, String>(4)?,
            row.get::<_, String>(5)?,
        ))
    })? {
        let (id, host, session_id, kind, params, inputs) = row?;
        let params: serde_json::Value = serde_json::from_str(&params)?;
        let inputs: serde_json::Value = serde_json::from_str(&inputs)?;
        if let Ok(host) = host.parse() {
            state.redirects.push_back(RedirectNotice {
                host,
                session_id,
                kind,
                text: params["detail"].as_str().unwrap_or_default().to_owned(),
                shadow: inputs["shadow"].as_bool().unwrap_or(false),
            });
            state.redirect_event_ids.push_back(Some(id));
        }
    }
    Ok(())
}
