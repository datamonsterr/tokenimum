//! Embedded database schema migrations.

/// Initial database schema migration.
pub(crate) const V001: &str = r"CREATE TABLE repos (
    repo_key TEXT PRIMARY KEY, -- normalized repository identity
    display_name TEXT NOT NULL, -- human-readable repository name
    first_seen INTEGER NOT NULL, -- first observation, Unix milliseconds
    last_seen INTEGER NOT NULL, -- most recent observation, Unix milliseconds
    delegated_runs INTEGER NOT NULL DEFAULT 0 -- delegated-run count
);
CREATE TABLE sessions (
    id INTEGER PRIMARY KEY, -- local session identifier
    host TEXT NOT NULL, -- harness identity
    agent_home TEXT NOT NULL, -- agent home path
    session_id TEXT NOT NULL, -- host session identifier
    repo_key TEXT NOT NULL REFERENCES repos(repo_key), -- repository identity
    cwd TEXT NOT NULL, -- checkout working directory
    started_at INTEGER NOT NULL, -- start time, Unix milliseconds
    ended_at INTEGER, -- end time, Unix milliseconds
    fold_status TEXT NOT NULL, -- fold processing state
    shadow INTEGER NOT NULL, -- whether actions were shadowed
    integration_level TEXT NOT NULL, -- integration level
    UNIQUE(host, agent_home, session_id)
);
CREATE TABLE turns (
    session_id INTEGER NOT NULL REFERENCES sessions(id), -- local session identifier
    turn_idx INTEGER NOT NULL, -- position within the session
    calls INTEGER NOT NULL, -- number of tool calls
    ended_well INTEGER NOT NULL, -- whether the turn ended well
    escalated INTEGER NOT NULL, -- whether the turn escalated
    digest_hash TEXT, -- optional digest hash
    PRIMARY KEY(session_id, turn_idx)
);
CREATE TABLE facts (
    repo_key TEXT NOT NULL REFERENCES repos(repo_key), -- repository identity
    fact_id TEXT NOT NULL, -- repository-local fact identifier
    family TEXT NOT NULL, -- extraction family
    scope TEXT NOT NULL, -- neutral or host-specific scope
    text_params TEXT NOT NULL, -- template parameters as JSON
    state TEXT NOT NULL, -- candidate, active, or retired
    score REAL NOT NULL, -- current lifecycle score
    confirmations INTEGER NOT NULL, -- confirming session count
    contradictions INTEGER NOT NULL, -- contradicting session count
    fire_count INTEGER NOT NULL, -- sessions where the fact fired
    fire_sessions INTEGER NOT NULL, -- sessions observed for fire rate
    PRIMARY KEY(repo_key, fact_id)
);
CREATE TABLE tool_evidence (
    repo_key TEXT NOT NULL REFERENCES repos(repo_key), -- repository identity
    host TEXT NOT NULL, -- harness identity
    tool TEXT NOT NULL, -- tool name
    prompts_unused INTEGER NOT NULL, -- consecutive unused prompts
    sessions_unused INTEGER NOT NULL, -- consecutive unused sessions
    last_used INTEGER, -- last use, Unix milliseconds
    PRIMARY KEY(repo_key, host, tool)
);
CREATE TABLE denylist_applied (
    checkout_path TEXT NOT NULL, -- checkout path
    host TEXT NOT NULL, -- harness identity
    tool TEXT NOT NULL, -- tool name
    written_at INTEGER NOT NULL, -- write time, Unix milliseconds
    restored INTEGER NOT NULL, -- whether the entry was restored
    PRIMARY KEY(checkout_path, host, tool)
);
CREATE TABLE envelopes (
    repo_key TEXT NOT NULL REFERENCES repos(repo_key), -- repository identity
    host TEXT NOT NULL, -- harness identity
    samples TEXT NOT NULL, -- good-turn lengths as a JSON array
    PRIMARY KEY(repo_key, host)
);
CREATE TABLE decisions (
    id INTEGER PRIMARY KEY, -- local decision identifier
    model TEXT NOT NULL, -- model version
    purpose TEXT NOT NULL, -- decision purpose
    questions TEXT NOT NULL, -- questions as JSON
    answers TEXT NOT NULL, -- answers as JSON
    cost REAL NOT NULL, -- priced decision cost
    latency_ms INTEGER NOT NULL, -- decision latency in milliseconds
    shadow INTEGER NOT NULL, -- whether the decision was shadowed
    outcome_label TEXT -- optional observed outcome
);
CREATE TABLE calibration (
    model_version TEXT NOT NULL, -- model version
    purpose TEXT NOT NULL, -- calibration purpose
    thresholds TEXT NOT NULL, -- calibrated thresholds as JSON
    sample_count INTEGER NOT NULL, -- labeled sample count
    precision REAL NOT NULL, -- measured precision
    recall REAL NOT NULL, -- measured recall
    PRIMARY KEY(model_version, purpose)
);
CREATE TABLE events (
    id INTEGER PRIMARY KEY, -- local event identifier
    kind TEXT NOT NULL, -- action kind
    session_id INTEGER NOT NULL REFERENCES sessions(id), -- local session identifier
    inputs TEXT NOT NULL, -- inputs as JSON
    evidence_ids TEXT NOT NULL, -- evidence identifiers as JSON
    explanation_template_id TEXT NOT NULL, -- explanation template identifier
    params TEXT NOT NULL -- explanation parameters as JSON
);
CREATE TABLE ledger (
    session_id INTEGER PRIMARY KEY REFERENCES sessions(id), -- local session identifier
    input_tokens INTEGER NOT NULL, -- uncached input tokens
    output_tokens INTEGER NOT NULL, -- output tokens
    cache_read_tokens INTEGER NOT NULL, -- cache-read tokens
    cache_write_tokens INTEGER NOT NULL, -- cache-write tokens
    priced_cost REAL NOT NULL, -- priced session cost
    overhead_tokens INTEGER NOT NULL, -- tokenimum decision tokens
    overhead_cost REAL NOT NULL -- tokenimum decision cost
);
CREATE TABLE installs (
    host TEXT NOT NULL, -- harness identity
    agent_home TEXT NOT NULL, -- agent home path
    files_written TEXT NOT NULL, -- written paths as JSON
    entry_ids TEXT NOT NULL, -- installation entry identifiers as JSON
    backup_paths TEXT NOT NULL, -- backup paths as JSON
    shim_hashes TEXT NOT NULL, -- shim hashes as JSON
    PRIMARY KEY(host, agent_home)
);
";

/// Migrations in schema version order.
pub(crate) const MIGRATIONS: &[&str] = &[V001, V002, V003];

/// Nullable reporting context; historical rows deliberately retain unknown times.
pub(crate) const V003: &str = r"
CREATE TABLE skill_evidence_sessions (session_id INTEGER PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE);
CREATE TABLE skill_evidence (
    session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    loaded INTEGER NOT NULL,
    used INTEGER NOT NULL,
    PRIMARY KEY(session_id, name)
);
";

pub(crate) const V002: &str = r"
ALTER TABLE events ADD COLUMN created_at INTEGER;
ALTER TABLE decisions ADD COLUMN created_at INTEGER;
ALTER TABLE decisions ADD COLUMN repo_key TEXT;
ALTER TABLE decisions ADD COLUMN session_id INTEGER;
ALTER TABLE decisions ADD COLUMN digest TEXT;
ALTER TABLE facts ADD COLUMN state_changed_at INTEGER;
";

#[cfg(test)]
mod report_schema_tests {
    use super::V001;
    use crate::Store;
    use rusqlite::Connection;

    #[test]
    fn read_only_v001_reports_unknown_context_without_migration() {
        let path = std::env::temp_dir().join(format!(
            "tkm-read-only-v001-{}-{}.sqlite",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch(V001).unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
        conn.execute_batch("INSERT INTO repos VALUES ('r','repo',1,1,0); INSERT INTO facts VALUES ('r','f','empty_probe','neutral','{}','retired',0,0,0,0,0);").unwrap();
        assert!(Store::from_read_only_connection(conn).is_err());
        let conn = crate::open_read_only(&path).unwrap();
        let store = Store::from_read_only_connection(conn).unwrap();
        let repo = tkm_core::model::RepoKey("r".into());
        assert_eq!(
            store
                .get_fact(&repo, "f")
                .unwrap()
                .unwrap()
                .state_changed_at,
            None
        );
        assert_eq!(store.list_facts(&repo, None).unwrap().len(), 1);
        assert_eq!(store.list_recent_events(-1).unwrap().len(), 0);
        assert_eq!(store.list_events_by_session(1).unwrap().len(), 0);
        assert!(store.get_event(1).unwrap().is_none());
        assert_eq!(store.list_recent_decisions(-1).unwrap().len(), 0);
        assert!(store.get_decision(1).unwrap().is_none());
        assert_eq!(
            store
                .list_decisions_by_model_purpose("m", "p", -1)
                .unwrap()
                .len(),
            0
        );
        let version: i64 = store
            .conn
            .pragma_query_value(None, "user_version", |r| r.get(0))
            .unwrap();
        assert_eq!(version, 1);
        drop(store);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn v001_database_upgrades_with_nullable_report_columns() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute_batch(V001).unwrap();
        conn.pragma_update(None, "user_version", 1).unwrap();
        conn.execute_batch("INSERT INTO repos VALUES ('legacy','legacy',1,1,0); INSERT INTO facts VALUES ('legacy','f','empty_probe','neutral','{}','retired',0,0,0,0,0);").unwrap();
        let store = Store::initialize(conn, std::time::Duration::from_millis(100)).unwrap();
        let conn = &store.conn;
        let version: i64 = conn
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 3);
        let legacy = store
            .get_fact(&tkm_core::model::RepoKey("legacy".into()), "f")
            .unwrap()
            .unwrap();
        assert_eq!(legacy.state, crate::FactState::Retired);
        assert_eq!(legacy.state_changed_at, None);
        for (table, column) in [
            ("events", "created_at"),
            ("decisions", "created_at"),
            ("decisions", "repo_key"),
            ("decisions", "session_id"),
            ("decisions", "digest"),
            ("facts", "state_changed_at"),
        ] {
            let mut stmt = conn
                .prepare(&format!("PRAGMA table_info({table})"))
                .unwrap();
            let columns = stmt
                .query_map([], |row| {
                    Ok((row.get::<_, String>(1)?, row.get::<_, bool>(3)?))
                })
                .unwrap()
                .collect::<rusqlite::Result<Vec<_>>>()
                .unwrap();
            assert!(
                columns
                    .iter()
                    .any(|(name, not_null)| name == column && !not_null),
                "{table}.{column}"
            );
        }
    }
}
