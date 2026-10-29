//! Local, token-authenticated dashboard server.
#![allow(clippy::unused_async)]
use crate::model::MonitorState;
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, Path, Request, State},
    http::{HeaderValue, StatusCode, header},
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    future::Future,
    io::{self, Read},
    net::{IpAddr, Ipv4Addr, SocketAddr},
    path::PathBuf,
    sync::{
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};

/// Listener address and per-start bearer secret.
#[derive(Clone)]
pub struct WebConfig {
    /// Loopback address and port.
    pub bind: SocketAddr,
    /// Random bearer token.
    pub token: String,
}
/// Invalid dashboard configuration.
#[derive(Debug, thiserror::Error)]
pub enum WebError {
    /// Address is malformed or is not IPv4 loopback.
    #[error("web bind must be 127.0.0.1:<port>")]
    InvalidBind,
}
/// Parse a loopback-only bind address.
///
/// # Errors
/// Rejects malformed and non-loopback addresses.
pub fn validate_bind(input: &str) -> Result<SocketAddr, WebError> {
    let addr: SocketAddr = input.parse().map_err(|_| WebError::InvalidBind)?;
    if addr.ip() != IpAddr::V4(Ipv4Addr::LOCALHOST) {
        return Err(WebError::InvalidBind);
    }
    Ok(addr)
}
/// Generate 32 random bytes encoded in lowercase hexadecimal.
///
/// # Errors
/// Returns an error if the operating system random source cannot be read.
pub fn generate_token() -> io::Result<String> {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut bytes = [0_u8; 32];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut bytes)?;
    let mut result = String::with_capacity(64);
    for byte in bytes {
        result.push(char::from(HEX[usize::from(byte >> 4)]));
        result.push(char::from(HEX[usize::from(byte & 15)]));
    }
    Ok(result)
}
/// Mutating operations supplied by the daemon.
pub trait Actions: Send + Sync {
    /// Retire a fact.
    ///
    /// # Errors
    /// Returns an explanation when the operation fails.
    fn forget_fact(&self, repo: &str, fact_id: &str) -> Result<String, String>;
    /// Restore a tool in a checkout.
    ///
    /// # Errors
    /// Returns an explanation when the operation fails.
    fn restore_tool(&self, checkout: &str, host: &str, tool: &str) -> Result<String, String>;
    /// Change the repository configuration preset.
    ///
    /// # Errors
    /// Returns an explanation when the operation fails.
    fn switch_preset(&self, repo: &str, preset: &str) -> Result<String, String>;
}
#[derive(Clone, Deserialize)]
struct ActionInput {
    repo: Option<String>,
    fact_id: Option<String>,
    checkout: Option<String>,
    host: Option<String>,
    tool: Option<String>,
    preset: Option<String>,
}
struct Pending {
    kind: String,
    input: ActionInput,
    created: u64,
}
/// Shared state for a dashboard instance.
#[derive(Clone)]
pub struct WebState {
    config: WebConfig,
    /// Monitor snapshot updated by the daemon feed.
    pub monitor: Arc<RwLock<MonitorState>>,
    actions: Arc<dyn Actions>,
    closing: Arc<AtomicBool>,
    action_gate: Arc<Mutex<()>>,
    db: PathBuf,
    pending: Arc<Mutex<HashMap<String, Pending>>>,
    blocking: Arc<tokio::sync::Semaphore>,
    clock: Arc<dyn Fn() -> u64 + Send + Sync>,
}
impl WebState {
    /// Create state from an initial monitor snapshot and database path.
    #[must_use]
    pub fn new(
        config: WebConfig,
        monitor: MonitorState,
        actions: Arc<dyn Actions>,
        db: PathBuf,
    ) -> Self {
        Self {
            config,
            monitor: Arc::new(RwLock::new(monitor)),
            actions,
            closing: Arc::new(AtomicBool::new(false)),
            action_gate: Arc::new(Mutex::new(())),
            db,
            pending: Arc::new(Mutex::new(HashMap::new())),
            blocking: Arc::new(tokio::sync::Semaphore::new(4)),
            clock: Arc::new(|| {
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs()
            }),
        }
    }
    /// Reload the database snapshot without blocking an async executor thread.
    ///
    /// # Errors
    /// Returns an error when the database or blocking task fails.
    pub async fn reload_snapshot(&self) -> Result<MonitorState, StatusCode> {
        let db = self.db.clone();
        blocking(self, move || {
            let conn = tkm_store::open_read_only(&db).map_err(|e| e.to_string())?;
            crate::model::load_snapshot(&conn).map_err(|e| e.to_string())
        })
        .await
    }
    /// Prevent new actions and wait for any action already executing to finish.
    pub async fn close(&self) {
        self.closing.store(true, Ordering::Release);
        let gate = Arc::clone(&self.action_gate);
        let _ = tokio::task::spawn_blocking(move || drop(gate.lock())).await;
    }
    /// Replace the clock, for deterministic expiry tests.
    pub fn set_clock(&mut self, clock: Arc<dyn Fn() -> u64 + Send + Sync>) {
        self.clock = clock;
    }
}
fn equal_token(a: &[u8], b: &[u8]) -> bool {
    let mut diff = a.len() ^ b.len();
    for i in 0..a.len().max(b.len()) {
        diff |= usize::from(a.get(i).copied().unwrap_or(0) ^ b.get(i).copied().unwrap_or(0));
    }
    diff == 0
}
fn s_is_closing(state: &WebState) -> bool {
    state.closing.load(Ordering::Acquire)
}
async fn guard(State(state): State<WebState>, request: Request, next: Next) -> Response {
    let expected = state.config.bind.port();
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok());
    let valid_host = host.is_some_and(|h| {
        h == format!("127.0.0.1:{expected}") || h == format!("localhost:{expected}")
    });
    let authorized = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
        .is_some_and(|token| equal_token(token.as_bytes(), state.config.token.as_bytes()));
    let mut response = if s_is_closing(&state) {
        StatusCode::SERVICE_UNAVAILABLE.into_response()
    } else if !valid_host {
        StatusCode::MISDIRECTED_REQUEST.into_response()
    } else if request.uri().path().starts_with("/api/") && !authorized {
        StatusCode::UNAUTHORIZED.into_response()
    } else {
        next.run(request).await
    };
    response.headers_mut().insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'self'; frame-ancestors 'none'"),
    );
    response
}
async fn index() -> Html<&'static str> {
    Html(include_str!("../assets/index.html"))
}
async fn js() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/javascript; charset=utf-8")],
        include_str!("../assets/app.js"),
    )
}
async fn css() -> impl IntoResponse {
    (
        [(header::CONTENT_TYPE, "text/css; charset=utf-8")],
        include_str!("../assets/app.css"),
    )
}
async fn state(State(s): State<WebState>) -> Result<Json<MonitorState>, StatusCode> {
    let now = (s.clock)();
    s.pending
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
        .retain(|_, entry| now.saturating_sub(entry.created) < 60);
    s.monitor
        .read()
        .map(|m| Json(m.clone()))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}
fn query(
    db: &std::path::Path,
    sql: &str,
    params: &[&dyn rusqlite::ToSql],
) -> Result<Value, String> {
    let conn = tkm_store::open_read_only(db).map_err(|e| e.to_string())?;
    let mut stmt = conn.prepare(sql).map_err(|e| e.to_string())?;
    let names: Vec<String> = stmt.column_names().map(ToString::to_string).collect();
    let rows = stmt
        .query_map(params, |row| {
            let mut obj = serde_json::Map::new();
            for (i, name) in names.iter().enumerate() {
                let value = match row.get_ref(i)? {
                    rusqlite::types::ValueRef::Null | rusqlite::types::ValueRef::Blob(_) => {
                        Value::Null
                    }
                    rusqlite::types::ValueRef::Integer(n) => json!(n),
                    rusqlite::types::ValueRef::Real(n) => json!(n),
                    rusqlite::types::ValueRef::Text(s) => json!(String::from_utf8_lossy(s)),
                };
                obj.insert(name.clone(), value);
            }
            Ok(Value::Object(obj))
        })
        .map_err(|e| e.to_string())?;
    Ok(json!(
        rows.collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?
    ))
}
async fn blocking<T: Send + 'static>(
    s: &WebState,
    f: impl FnOnce() -> Result<T, String> + Send + 'static,
) -> Result<T, StatusCode> {
    let permit = Arc::clone(&s.blocking)
        .acquire_owned()
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        f()
    })
    .await
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?
    .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)
}
async fn history(State(s): State<WebState>) -> Result<Json<Value>, StatusCode> {
    let db = s.db.clone();
    blocking(&s, move || {
        let rows = query(&db, "SELECT sessions.session_id, sessions.repo_key, sessions.host, sessions.started_at, ledger.priced_cost_usd AS priced_cost FROM ledger JOIN sessions ON sessions.id=ledger.session_id ORDER BY sessions.started_at", &[])?;
        let daily = query(&db, "SELECT date(sessions.started_at / 1000, 'unixepoch') AS day, sum(ledger.priced_cost_usd) AS cost FROM ledger JOIN sessions ON sessions.id=ledger.session_id GROUP BY day ORDER BY day", &[])?;
        Ok(Json(json!({"sessions":rows,"days":daily})))
    }).await
}
async fn facts(
    State(s): State<WebState>,
    Path((repo, id)): Path<(String, String)>,
) -> Result<Json<Value>, StatusCode> {
    let db = s.db.clone();
    blocking(&s, move || {
        query(
            &db,
            "SELECT * FROM facts WHERE repo_key=?1 AND fact_id=?2",
            &[&repo, &id],
        )
        .map(Json)
    })
    .await
}
async fn prune(State(s): State<WebState>) -> Result<Json<Value>, StatusCode> {
    let db = s.db.clone();
    blocking(&s, move || {
        let evidence = query(&db, "SELECT * FROM tool_evidence", &[])?;
        let applied = query(&db, "SELECT * FROM denylist_applied", &[])?;
        Ok(Json(json!({"evidence":evidence,"applied":applied})))
    })
    .await
}
fn nonempty(s: Option<&str>) -> Option<&str> {
    s.filter(|s| !s.trim().is_empty())
}
async fn prepare(
    State(s): State<WebState>,
    Path(kind): Path<String>,
    Json(input): Json<ActionInput>,
) -> Result<Json<Value>, StatusCode> {
    if s_is_closing(&s) {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    if [
        input.repo.as_deref(),
        input.fact_id.as_deref(),
        input.checkout.as_deref(),
        input.host.as_deref(),
        input.tool.as_deref(),
        input.preset.as_deref(),
    ]
    .into_iter()
    .flatten()
    .any(|value| value.len() > 4096)
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let now = (s.clock)();
    {
        let mut pending = s
            .pending
            .lock()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        pending.retain(|_, entry| now.saturating_sub(entry.created) < 60);
    }
    let summary = match kind.as_str() {
        "forget_fact" => format!(
            "Forget fact {} in {}",
            nonempty(input.fact_id.as_deref()).ok_or(StatusCode::BAD_REQUEST)?,
            nonempty(input.repo.as_deref()).ok_or(StatusCode::BAD_REQUEST)?
        ),
        "restore_tool" => {
            let host = nonempty(input.host.as_deref()).ok_or(StatusCode::BAD_REQUEST)?;
            host.parse::<tkm_core::model::Host>()
                .map_err(|_| StatusCode::BAD_REQUEST)?;
            let checkout = nonempty(input.checkout.as_deref()).ok_or(StatusCode::BAD_REQUEST)?;
            if !std::path::Path::new(checkout).is_absolute() {
                return Err(StatusCode::BAD_REQUEST);
            }
            let db = s.db.clone();
            let check = checkout.to_owned();
            let recorded = blocking(&s, move || {
                let conn = tkm_store::open_read_only(&db).map_err(|e| e.to_string())?;
                conn.query_row(
                    "SELECT EXISTS(SELECT 1 FROM denylist_applied WHERE checkout_path=?1)",
                    [&check],
                    |row| row.get::<_, bool>(0),
                )
                .map_err(|e| e.to_string())
            })
            .await
            .map_err(|_| StatusCode::BAD_REQUEST)?;
            if !recorded {
                return Err(StatusCode::BAD_REQUEST);
            }
            format!(
                "Restore tool {} for {host} in {checkout}",
                nonempty(input.tool.as_deref()).ok_or(StatusCode::BAD_REQUEST)?
            )
        }
        "switch_preset" => {
            let preset = nonempty(input.preset.as_deref()).ok_or(StatusCode::BAD_REQUEST)?;
            if !["observe", "balanced", "aggressive", "offline"].contains(&preset) {
                return Err(StatusCode::BAD_REQUEST);
            }
            format!(
                "Switch {} to preset {preset}",
                nonempty(input.repo.as_deref()).ok_or(StatusCode::BAD_REQUEST)?
            )
        }
        _ => return Err(StatusCode::NOT_FOUND),
    };
    let id = blocking(&s, || generate_token().map_err(|e| e.to_string())).await?;
    if s_is_closing(&s) {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    let now = (s.clock)();
    let mut pending = s
        .pending
        .lock()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    pending.retain(|_, entry| now.saturating_sub(entry.created) < 60);
    if pending.len() >= 32 {
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }
    pending.insert(
        id.clone(),
        Pending {
            kind,
            input,
            created: now,
        },
    );
    Ok(Json(json!({"confirm_id": id, "summary": summary})))
}
async fn confirm(
    State(s): State<WebState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let entry = {
        let mut pending = s
            .pending
            .lock()
            .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "lock failed".into()))?;
        let now = (s.clock)();
        pending.retain(|_, entry| now.saturating_sub(entry.created) < 60);
        pending
            .remove(&id)
            .ok_or((StatusCode::NOT_FOUND, "confirmation not found".into()))?
    };
    let actions = Arc::clone(&s.actions);
    let closing = Arc::clone(&s.closing);
    let gate = Arc::clone(&s.action_gate);
    let permit = Arc::clone(&s.blocking)
        .acquire_owned()
        .await
        .map_err(|_| (StatusCode::INTERNAL_SERVER_ERROR, "semaphore closed".into()))?;
    let result = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let _guard = gate.lock().map_err(|_| "action lock failed".to_owned())?;
        if closing.load(Ordering::Acquire) {
            return Err("server is shutting down".to_owned());
        }
        let i = entry.input;
        match entry.kind.as_str() {
            "forget_fact" => actions.forget_fact(
                i.repo.as_deref().unwrap_or_default(),
                i.fact_id.as_deref().unwrap_or_default(),
            ),
            "restore_tool" => actions.restore_tool(
                i.checkout.as_deref().unwrap_or_default(),
                i.host.as_deref().unwrap_or_default(),
                i.tool.as_deref().unwrap_or_default(),
            ),
            "switch_preset" => actions.switch_preset(
                i.repo.as_deref().unwrap_or_default(),
                i.preset.as_deref().unwrap_or_default(),
            ),
            _ => Err("unknown action".into()),
        }
    })
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "action task failed".into(),
        )
    })?
    .map_err(|e| (StatusCode::BAD_REQUEST, e))?;
    Ok(Json(json!({"summary":result})))
}
/// Construct the dashboard router.
pub fn router(state: WebState) -> Router {
    Router::new()
        .route("/", get(index))
        .route("/assets/app.js", get(js))
        .route("/assets/app.css", get(css))
        .route("/api/state", get(self::state))
        .route("/api/history", get(history))
        .route("/api/facts/{repo}/{id}", get(facts))
        .route("/api/prune", get(prune))
        .route(
            "/api/actions/{kind}",
            post(prepare).layer(DefaultBodyLimit::max(16 * 1024)),
        )
        .route("/api/confirm/{id}", post(confirm))
        .layer(middleware::from_fn_with_state(state.clone(), guard))
        .with_state(state)
}
/// Serve until shutdown completes.
///
/// # Errors
/// Returns an I/O error if the server fails.
pub async fn serve(
    listener: tokio::net::TcpListener,
    router: Router,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> io::Result<()> {
    axum::serve(listener, router)
        .with_graceful_shutdown(shutdown)
        .await
}
