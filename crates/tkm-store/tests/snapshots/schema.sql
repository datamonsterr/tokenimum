-- table calibration
CREATE TABLE calibration (
    model_version TEXT NOT NULL, -- model version
    purpose TEXT NOT NULL, -- calibration purpose
    thresholds TEXT NOT NULL, -- calibrated thresholds as JSON
    sample_count INTEGER NOT NULL, -- labeled sample count
    precision REAL NOT NULL, -- measured precision
    recall REAL NOT NULL, -- measured recall
    PRIMARY KEY(model_version, purpose)
);

-- table decisions
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
, created_at INTEGER, repo_key TEXT, session_id INTEGER, digest TEXT, cost_usd REAL);

-- table denylist_applied
CREATE TABLE denylist_applied (
    checkout_path TEXT NOT NULL, -- checkout path
    host TEXT NOT NULL, -- harness identity
    tool TEXT NOT NULL, -- tool name
    written_at INTEGER NOT NULL, -- write time, Unix milliseconds
    restored INTEGER NOT NULL, -- whether the entry was restored
    PRIMARY KEY(checkout_path, host, tool)
);

-- table envelopes
CREATE TABLE envelopes (
    repo_key TEXT NOT NULL REFERENCES repos(repo_key), -- repository identity
    host TEXT NOT NULL, -- harness identity
    samples TEXT NOT NULL, -- good-turn lengths as a JSON array
    PRIMARY KEY(repo_key, host)
);

-- table events
CREATE TABLE events (
    id INTEGER PRIMARY KEY, -- local event identifier
    kind TEXT NOT NULL, -- action kind
    session_id INTEGER NOT NULL REFERENCES sessions(id), -- local session identifier
    inputs TEXT NOT NULL, -- inputs as JSON
    evidence_ids TEXT NOT NULL, -- evidence identifiers as JSON
    explanation_template_id TEXT NOT NULL, -- explanation template identifier
    params TEXT NOT NULL -- explanation parameters as JSON
, created_at INTEGER);

-- table facts
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
    fire_sessions INTEGER NOT NULL, state_changed_at INTEGER, -- sessions observed for fire rate
    PRIMARY KEY(repo_key, fact_id)
);

-- table host_health
CREATE TABLE host_health (
    host TEXT PRIMARY KEY,
    fold_only INTEGER NOT NULL CHECK (fold_only = 1)
);

-- table installs
CREATE TABLE installs (
    host TEXT NOT NULL, -- harness identity
    agent_home TEXT NOT NULL, -- agent home path
    files_written TEXT NOT NULL, -- written paths as JSON
    entry_ids TEXT NOT NULL, -- installation entry identifiers as JSON
    backup_paths TEXT NOT NULL, -- backup paths as JSON
    shim_hashes TEXT NOT NULL, -- shim hashes as JSON
    PRIMARY KEY(host, agent_home)
);

-- table ledger
CREATE TABLE ledger (
    session_id INTEGER PRIMARY KEY REFERENCES sessions(id), -- local session identifier
    input_tokens INTEGER NOT NULL, -- uncached input tokens
    output_tokens INTEGER NOT NULL, -- output tokens
    cache_read_tokens INTEGER NOT NULL, -- cache-read tokens
    cache_write_tokens INTEGER NOT NULL, -- cache-write tokens
    priced_cost REAL NOT NULL, -- priced session cost
    overhead_tokens INTEGER NOT NULL, -- tokenimum decision tokens
    overhead_cost REAL NOT NULL -- tokenimum decision cost
, overhead_cost_usd REAL, priced_cost_usd REAL);

-- table repos
CREATE TABLE repos (
    repo_key TEXT PRIMARY KEY, -- normalized repository identity
    display_name TEXT NOT NULL, -- human-readable repository name
    first_seen INTEGER NOT NULL, -- first observation, Unix milliseconds
    last_seen INTEGER NOT NULL, -- most recent observation, Unix milliseconds
    delegated_runs INTEGER NOT NULL DEFAULT 0 -- delegated-run count
);

-- table sessions
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

-- table skill_evidence
CREATE TABLE skill_evidence (
    session_id INTEGER NOT NULL REFERENCES sessions(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    loaded INTEGER NOT NULL,
    used INTEGER NOT NULL,
    PRIMARY KEY(session_id, name)
);

-- table skill_evidence_sessions
CREATE TABLE skill_evidence_sessions (session_id INTEGER PRIMARY KEY REFERENCES sessions(id) ON DELETE CASCADE);

-- table spend_attempts
CREATE TABLE spend_attempts (id INTEGER PRIMARY KEY, record TEXT NOT NULL);

-- table spend_migration
CREATE TABLE spend_migration (legacy_decision_max INTEGER NOT NULL);

-- table tool_evidence
CREATE TABLE tool_evidence (
    repo_key TEXT NOT NULL REFERENCES repos(repo_key), -- repository identity
    host TEXT NOT NULL, -- harness identity
    tool TEXT NOT NULL, -- tool name
    prompts_unused INTEGER NOT NULL, -- consecutive unused prompts
    sessions_unused INTEGER NOT NULL, -- consecutive unused sessions
    last_used INTEGER, -- last use, Unix milliseconds
    PRIMARY KEY(repo_key, host, tool)
);

-- table turns
CREATE TABLE turns (
    session_id INTEGER NOT NULL REFERENCES sessions(id), -- local session identifier
    turn_idx INTEGER NOT NULL, -- position within the session
    calls INTEGER NOT NULL, -- number of tool calls
    ended_well INTEGER NOT NULL, -- whether the turn ended well
    escalated INTEGER NOT NULL, -- whether the turn escalated
    digest_hash TEXT, -- optional digest hash
    PRIMARY KEY(session_id, turn_idx)
);
