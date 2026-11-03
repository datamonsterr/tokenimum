//! In-memory monitor data and initial read-only database snapshot.
use rusqlite::Connection;
use serde::Serialize;
use std::collections::VecDeque;
use tkm_core::loopguard::{EnvelopeTracker, LoopConfig};
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
    /// Daily decision spend (UTC) for the seven most recent days, oldest first.
    pub spend_days: Vec<(String, f64)>,
    /// Why no decisions can be made right now, when known (disabled or no key).
    pub decisions_unavailable: Option<String>,
    /// Count of durable decisions with unknown cost.
    pub unknown_decision_costs: u64,
    /// Count of sessions whose cost is unknown.
    pub unknown_session_costs: u64,
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
                if update.cost_usd.is_finite()
                    && update.cost_usd > 0.0
                    && let Some((_, today)) = self.spend_days.last_mut()
                {
                    *today += update.cost_usd;
                }
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
/// Settings the database does not hold, used to complete a snapshot.
#[derive(Debug, Clone, Default)]
pub struct SnapshotConfig {
    /// Daily and session decision caps in US dollars, when known.
    pub caps: Option<(f64, f64)>,
    /// Configured `loop.mode`, when known.
    pub loop_mode: Option<String>,
    /// Loop settings that turn envelope samples into an envelope.
    pub loop_cfg: LoopConfig,
    /// Current time in Unix milliseconds, used for today's spend.
    pub now_ms: Option<i64>,
    /// Why decisions cannot run (disabled or no API key), when known.
    pub decisions_unavailable: Option<String>,
}
/// Read a snapshot from an existing connection, normally opened with `tkm_store::open_read_only`.
///
/// # Errors
/// Returns an error if SQLite or a stored JSON value cannot be read.
pub fn load_snapshot(conn: &Connection) -> Result<MonitorState, crate::Error> {
    load_snapshot_with(conn, &SnapshotConfig::default())
}
/// Read a snapshot and complete spend and loop state from the supplied settings.
///
/// # Errors
/// Returns an error if SQLite or a stored JSON value cannot be read.
pub fn load_snapshot_with(
    conn: &Connection,
    config: &SnapshotConfig,
) -> Result<MonitorState, crate::Error> {
    let mut state = MonitorState::default();
    load_sessions(conn, config, &mut state)?;
    let version: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    let decisions_sql = if version >= 4 {
        "SELECT purpose,model,latency_ms,cost_usd,shadow,answers,id FROM (SELECT * FROM decisions WHERE cost_usd IS NOT NULL ORDER BY id DESC LIMIT 1000) ORDER BY id"
    } else {
        "SELECT purpose,model,latency_ms,cost,shadow,answers,id FROM (SELECT * FROM decisions ORDER BY id DESC LIMIT 1000) ORDER BY id"
    };
    let mut query = conn.prepare(decisions_sql)?;
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
    state.facts = load_facts(conn)?;
    if version >= 4 {
        state.unknown_decision_costs = unknown_decision_costs(conn)?;
    }
    let price = if version >= 4 {
        "l.priced_cost_usd"
    } else {
        "NULLIF(l.priced_cost, 0)"
    };
    state.unknown_session_costs = count(conn.query_row(
        &format!("SELECT count(*) FROM ledger l WHERE {price} IS NULL"),
        [],
        |row| row.get::<_, i64>(0),
    )?)?;
    let sql = format!(
        "SELECT repo_key,host,priced_cost,shadow FROM (SELECT s.repo_key,s.host,{price} AS priced_cost,s.shadow,s.started_at FROM ledger l JOIN sessions s ON s.id=l.session_id WHERE {price} IS NOT NULL ORDER BY s.started_at DESC LIMIT 1000) ORDER BY started_at"
    );
    let mut query = conn.prepare(&sql)?;
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
    state
        .decisions_unavailable
        .clone_from(&config.decisions_unavailable);
    if version >= 4 {
        state.spend_days = decision_spend_days(conn, config.now_ms.unwrap_or_else(system_now_ms))?;
    }
    load_redirects(conn, &mut state)?;
    if let (Some((daily_cap_usd, session_cap_usd)), Some(now_ms), true) =
        (config.caps, config.now_ms, version >= 4)
    {
        state.spend = Some(SpendUpdate {
            today_usd: spent_today(conn, now_ms)?,
            daily_cap_usd,
            session_cap_usd,
        });
    }
    Ok(state)
}

const DAY_MS: i64 = 86_400_000;

fn system_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .and_then(|d| i64::try_from(d.as_millis()).ok())
        .unwrap_or(0)
}

/// Decision spend since UTC midnight, counted as `tkm status` counts it.
fn spent_today(conn: &Connection, now_ms: i64) -> Result<f64, crate::Error> {
    spent_between(conn, now_ms - now_ms.rem_euclid(DAY_MS), now_ms)
}

/// Decision spend per UTC day for the seven days ending at `now_ms`, oldest first.
///
/// This is decision spend, not session ledger cost: SPEC 11.3 plots spend against the caps.
fn decision_spend_days(conn: &Connection, now_ms: i64) -> Result<Vec<(String, f64)>, crate::Error> {
    let today = now_ms - now_ms.rem_euclid(DAY_MS);
    (0..7)
        .rev()
        .map(|back| {
            let start = today - back * DAY_MS;
            let end = if back == 0 {
                now_ms
            } else {
                start + DAY_MS - 1
            };
            let day = conn.query_row("SELECT date(?1 / 1000, 'unixepoch')", [start], |row| {
                row.get::<_, String>(0)
            })?;
            Ok((day, spent_between(conn, start, end)?))
        })
        .collect()
}

fn spent_between(conn: &Connection, from_ms: i64, to_ms: i64) -> Result<f64, crate::Error> {
    let spent = conn.query_row(
        "SELECT COALESCE((SELECT sum(json_extract(record, '$.cost_usd')) FROM spend_attempts WHERE json_extract(record, '$.ts_ms') BETWEEN ?1 AND ?2 AND json_extract(record, '$.cost_usd') >= 0), 0) + COALESCE((SELECT sum(cost_usd) FROM decisions WHERE created_at BETWEEN ?1 AND ?2 AND cost_usd >= 0 AND (NOT EXISTS (SELECT 1 FROM spend_attempts) OR id <= (SELECT legacy_decision_max FROM spend_migration) OR purpose IN ('models_probe','doctor_probe'))), 0)",
        [from_ms, to_ms],
        |row| row.get::<_, f64>(0),
    )?;
    Ok(if spent.is_finite() { spent } else { 0.0 })
}

fn load_sessions(
    conn: &Connection,
    config: &SnapshotConfig,
    state: &mut MonitorState,
) -> Result<(), crate::Error> {
    // The daemon starts each session in the state its loop mode implies (SPEC 11.3).
    let loop_state = match config.loop_mode.as_deref() {
        Some("off") => "off",
        Some("shadow") => "shadow",
        Some(_) => "watching",
        None => "unknown",
    };
    let min_samples = u32::try_from(config.loop_cfg.envelope_min_samples).unwrap_or(u32::MAX);
    let mut query = conn.prepare("SELECT s.host,s.session_id,s.repo_key,s.fold_status,COALESCE((SELECT calls FROM turns WHERE session_id=s.id ORDER BY turn_idx DESC LIMIT 1),0),(SELECT id FROM events WHERE session_id=s.id ORDER BY id DESC LIMIT 1),(SELECT turn_idx FROM turns WHERE session_id=s.id ORDER BY turn_idx DESC LIMIT 1),(SELECT samples FROM envelopes WHERE repo_key=s.repo_key AND host=s.host) FROM sessions s WHERE s.ended_at IS NULL ORDER BY s.id")?;
    for row in query.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, u32>(4)?,
            row.get::<_, Option<i64>>(5)?,
            row.get::<_, Option<i64>>(6)?,
            row.get::<_, Option<String>>(7)?,
        ))
    })? {
        let (host, session_id, repo, phase, turn_calls, event_id, turn_id, samples) = row?;
        let samples: Vec<u32> = samples
            .map(|json| serde_json::from_str(&json))
            .transpose()?
            .unwrap_or_default();
        let envelope = EnvelopeTracker::from_lengths(&samples).value(&config.loop_cfg);
        if let Ok(host) = host.parse() {
            state.sessions.push(SessionUpdate {
                host,
                session_id,
                turn_id: turn_id.and_then(|turn| u64::try_from(turn).ok()),
                repo: Some(repo),
                phase,
                turn_calls,
                envelope,
                envelope_samples: Some(u32::try_from(samples.len()).unwrap_or(u32::MAX)),
                envelope_min_samples: Some(min_samples),
                loop_state: loop_state.into(),
            });
            state.session_event_ids.push(event_id);
        }
    }
    Ok(())
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
                triggering_calls: inputs["triggering_calls"]
                    .as_array()
                    .map(|calls| {
                        calls
                            .iter()
                            .filter_map(serde_json::Value::as_str)
                            .map(str::to_owned)
                            .collect()
                    })
                    .unwrap_or_default(),
                kind,
                text: params["detail"].as_str().unwrap_or_default().to_owned(),
                shadow: inputs["shadow"].as_bool().unwrap_or(false),
            });
            state.redirect_event_ids.push_back(Some(id));
        }
    }
    Ok(())
}

fn count(value: i64) -> rusqlite::Result<u64> {
    u64::try_from(value).map_err(|_| rusqlite::Error::IntegralValueOutOfRange(0, value))
}

fn load_facts(conn: &Connection) -> Result<Vec<Fact>, crate::Error> {
    let mut query = conn.prepare(
        "SELECT repo_key,fact_id,state,score,confirmations FROM facts WHERE state IN ('active','candidate') OR rowid IN (SELECT rowid FROM facts WHERE state='retired' ORDER BY rowid DESC LIMIT 100) ORDER BY repo_key,fact_id",
    )?;
    let facts = query
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
    Ok(facts)
}

fn unknown_decision_costs(conn: &Connection) -> Result<u64, crate::Error> {
    let unknown = conn.query_row(
        "SELECT (SELECT count(*) FROM spend_attempts WHERE json_extract(record, '$.cost_usd') IS NULL) + (SELECT count(*) FROM decisions WHERE cost_usd IS NULL AND (id <= (SELECT legacy_decision_max FROM spend_migration) OR purpose IN ('models_probe','doctor_probe')))",
        [],
        |row| row.get::<_, i64>(0),
    )?;
    Ok(count(unknown)?)
}
