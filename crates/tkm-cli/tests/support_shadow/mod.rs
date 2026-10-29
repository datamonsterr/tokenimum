use std::collections::BTreeMap;
use std::fs;
use std::io::{self, Write};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;
use tkm_config::config::{LoadInputs, load};
use tkm_proto::{EventKind, Host};
use tkmd::daemon::{Daemon, DaemonOptions};
use tkmd::monitor::Monitor;
use tkmd::server::{Bind, Server};
use tkmd::settings::{FileSettings, LoopMode, Settings};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::runtime::Runtime;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

use super::support_init::Fixture;

pub const SESSION: &str = "shadow-fixture-session";
pub const MODEL: &str = "typesafe/jev-1.14-20261014";
pub const EVENTS: [(&str, EventKind); 6] = [
    ("SessionStart", EventKind::SessionStart),
    ("UserPromptSubmit", EventKind::Prompt),
    ("PostToolUse", EventKind::ToolDone),
    ("PostToolUseFailure", EventKind::ToolDone),
    ("Stop", EventKind::TurnEnd),
    ("SessionEnd", EventKind::SessionEnd),
];

pub struct ShadowFixture {
    pub init: Fixture,
    key: PathBuf,
}

impl ShadowFixture {
    pub fn new() -> Self {
        let init = Fixture::new();
        let key = init.temp.path().join("openrouter.env");
        fs::write(&key, "OPENROUTER_API_KEY=fixture-shadow-key\n").unwrap();
        fs::set_permissions(&key, fs::Permissions::from_mode(0o600)).unwrap();
        fs::create_dir_all(&init.paths.config_dir).unwrap();
        fs::write(
            &init.paths.global_config,
            format!(
                "preset = \"observe\"\n[decide]\napi_key_path = {}\nchain = [\"{MODEL}\"]\n",
                serde_json::to_string(key.to_str().unwrap()).unwrap()
            ),
        )
        .unwrap();
        // Respawning from the fault test must never reach the developer's daemon or HOME.
        init.script("tkmd", "exit 0\n");
        Self { init, key }
    }

    pub fn install(&self) -> BTreeMap<String, Vec<String>> {
        let original_key = fs::read(&self.key).unwrap();
        let output = self.init.run(&["init", "--yes", "--only", "claude"]);
        assert!(output.status.success(), "{output:?}");
        assert_eq!(fs::read(&self.key).unwrap(), original_key);
        let config = load(&self.config_inputs()).unwrap();
        assert!(config.errors.is_empty(), "{:?}", config.errors);
        assert_eq!(config.get_bool("decide.enabled"), Some(true));
        assert_eq!(config.get_str("decide.api_key_path"), self.key.to_str());
        assert_eq!(config.get_str("loop.mode"), Some("shadow"));
        let records = self.init.installs();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].host, Host::Claude);
        let settings = fs::read(self.init.home.join(".claude/settings.json")).unwrap();
        let document: Value = serde_json::from_slice(&settings).unwrap();
        assert_eq!(
            document["hooks"]["SessionStart"][0]["hooks"][0]["command"],
            "foreign-hook"
        );
        installed_hooks(&settings)
    }

    fn config_inputs(&self) -> LoadInputs {
        LoadInputs {
            global_path: self.init.paths.global_config.clone(),
            repo_root: None,
            env: Vec::new(),
            flags: Vec::new(),
        }
    }

    pub fn error_lines(&self) -> Vec<String> {
        match fs::read_to_string(&self.init.paths.errors_log) {
            Ok(text) => text.lines().map(str::to_owned).collect(),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
            Err(error) => panic!("cannot read fixture errors.log: {error}"),
        }
    }

    pub fn install_hook_binary(&self) {
        // As in tkmd/tests/e2e_claude.rs, workspace CI builds the sibling hook executable.
        let binary = Path::new(env!("CARGO_BIN_EXE_tkm")).with_file_name("tkm-hook");
        assert!(
            binary.is_file(),
            "workspace CI must build {}",
            binary.display()
        );
        symlink(binary, self.init.bin.join("tkm-hook")).unwrap();
    }

    pub fn hook_process(&self, args: &[String], input: &Value) -> Output {
        let base = self.init.command(&[]);
        let mut child = Command::new(self.init.bin.join("tkm-hook"))
            .args(args)
            .env_clear()
            .envs(base.get_envs().map(|(key, value)| (key, value.unwrap())))
            .env("TKM_TKMD_BIN", self.init.bin.join("tkmd"))
            .current_dir(&self.init.home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(&serde_json::to_vec(input).unwrap())
            .unwrap();
        child.wait_with_output().unwrap()
    }
}

pub fn installed_hooks(bytes: &[u8]) -> BTreeMap<String, Vec<String>> {
    let settings: Value = serde_json::from_slice(bytes).unwrap();
    let mut result = BTreeMap::new();
    for (native, expected) in EVENTS {
        let handlers: Vec<_> = settings["hooks"][native]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|group| group["hooks"].as_array().unwrap())
            .filter(|handler| handler["command"] == "tkm-hook")
            .collect();
        assert_eq!(
            handlers.len(),
            1,
            "missing or duplicate installed hook: {native}"
        );
        let handler = handlers[0];
        assert_eq!(handler["type"], "command");
        let args: Vec<String> = handler["args"]
            .as_array()
            .unwrap()
            .iter()
            .map(|arg| arg.as_str().unwrap().to_owned())
            .collect();
        let parsed = tkm_hook::parse_args(&args).expect("installed argv must parse");
        assert_eq!(parsed.host, Host::Claude);
        assert_eq!(parsed.event, expected, "{native}");
        assert!(parsed.payload.is_none());
        result.insert(native.to_owned(), args);
    }
    result
}

pub fn payload(fixture: &ShadowFixture, native: &str) -> Value {
    let file = match native {
        "PostToolUse" => "PostToolUse-bash",
        "PostToolUseFailure" => "PostToolUseFailure-bash",
        other => other,
    };
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/claude/hooks")
        .join(format!("{file}.json"));
    let mut payload: Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    payload["session_id"] = SESSION.into();
    payload["cwd"] = fixture.init.home.to_str().unwrap().into();
    payload["scratchpad_dir"] = fixture.init.temp.path().to_str().unwrap().into();
    // This session is driven by hooks only; it has no harness transcript to fold.
    payload["transcript_path"] = Value::Null;
    payload
}

pub struct ShadowEvidence {
    pub event_id: i64,
    pub repo: String,
    pub session: String,
}

pub struct LiveDaemon {
    runtime: Runtime,
    daemon: Arc<Daemon>,
    stop: oneshot::Sender<()>,
    server: JoinHandle<io::Result<()>>,
    monitor: JoinHandle<()>,
    backend: Option<JoinHandle<Value>>,
}

impl LiveDaemon {
    pub fn start(fixture: &ShadowFixture) -> Self {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let (daemon, stop, server, monitor, backend) = runtime.block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let endpoint = format!(
                "http://{}/api/alpha/decisions",
                listener.local_addr().unwrap()
            );
            let backend = tokio::spawn(async move { serve_decision(&listener).await });
            let config = load(&fixture.config_inputs()).unwrap();
            let settings = Settings::from_config(&config);
            assert_eq!(settings.loop_mode, LoopMode::Shadow);
            let key = tkm_decide::key::load_key(
                None,
                Path::new(config.get_str("decide.api_key_path").unwrap()),
                None,
            )
            .unwrap();
            let mut options = DaemonOptions::new(fixture.init.paths.clone());
            options.env = vec![
                (
                    "HOME".into(),
                    fixture.init.home.to_str().unwrap().to_owned(),
                ),
                (
                    "GIT_CEILING_DIRECTORIES".into(),
                    fixture.init.temp.path().to_str().unwrap().to_owned(),
                ),
            ];
            options.settings = Arc::new(FileSettings::new(fixture.init.paths.clone(), Vec::new()));
            options.decider = tkmd::decider::build_decider(
                settings.decide,
                Some(key),
                Some(&endpoint),
                options.ledger.clone(),
            );
            let daemon = Daemon::new(options);
            let Bind::Bound(server) = Server::bind(daemon.clone()).await.unwrap() else {
                panic!("a fresh fake HOME cannot already have a daemon");
            };
            let monitor = Monitor::bind(&daemon).await.unwrap();
            let monitor = tokio::spawn(monitor.run(daemon.clone()));
            let (stop, stopped) = oneshot::channel();
            let server = tokio::spawn(server.run(async move {
                let _ = stopped.await;
            }));
            daemon.start_workers();
            (daemon, stop, server, monitor, backend)
        });
        Self {
            runtime,
            daemon,
            stop,
            server,
            monitor,
            backend: Some(backend),
        }
    }

    pub fn hook(&self, fixture: &ShadowFixture, args: &[String], payload: &Value) {
        let mut out = Vec::new();
        // A socket regression must not spawn a real daemon from this in-process client.
        fs::write(self.daemon.paths().socket.with_file_name("tkmd.spawn"), b"").unwrap();
        tkm_hook::run_hook_with_stdin(
            args,
            [(
                "CLAUDE_CONFIG_DIR".into(),
                fixture
                    .init
                    .home
                    .join(".claude")
                    .to_str()
                    .unwrap()
                    .to_owned(),
            )],
            || serde_json::to_vec(payload).unwrap(),
            Some(&self.daemon.paths().socket),
            Some(&self.daemon.paths().errors_log),
            &mut out,
        );
        // SPEC 6.3: Claude's no-op is empty stdout, not an empty JSON object.
        assert_eq!(
            out,
            Host::Claude.noop_reply().as_bytes(),
            "shadow hooks must never inject or block"
        );
    }

    pub fn event(
        &self,
        fixture: &ShadowFixture,
        hooks: &BTreeMap<String, Vec<String>>,
        native: &str,
    ) {
        self.hook(fixture, &hooks[native], &payload(fixture, native));
    }

    pub fn wait_for_envelope(&self, fixture: &ShadowFixture) {
        self.wait_for(fixture, |db| {
            let samples: String = db.query_row(
                "SELECT COALESCE((SELECT samples FROM envelopes WHERE host = 'claude' LIMIT 1), '[]')",
                [], |row| row.get(0)).unwrap();
            serde_json::from_str::<Vec<u32>>(&samples).unwrap() == [5; 5]
        });
        let db = tkm_store::open_read_only(&fixture.init.paths.db).unwrap();
        let decisions: i64 = db
            .query_row("SELECT COUNT(*) FROM decisions", [], |row| row.get(0))
            .unwrap();
        assert_eq!(decisions, 0, "successful warm-up turns never ask the judge");
    }

    pub fn wait_for_shadow(&self, fixture: &ShadowFixture) -> ShadowEvidence {
        self.wait_for(fixture, |db| {
            let count: i64 = db.query_row(
                "SELECT COUNT(*) FROM decisions WHERE purpose = 'loop_guard' AND shadow = 1",
                [], |row| row.get(0)).unwrap();
            let events: i64 = db.query_row(
                "SELECT COUNT(*) FROM events e JOIN sessions s ON s.id = e.session_id WHERE e.kind = 'loop.shadow' AND s.session_id = ?1",
                [SESSION], |row| row.get(0)).unwrap();
            count > 0 && events > 0
        });
        let db = tkm_store::open_read_only(&fixture.init.paths.db).unwrap();
        let event_id: i64 = db.query_row(
            "SELECT e.id FROM events e JOIN sessions s ON s.id = e.session_id WHERE e.kind = 'loop.shadow' AND s.session_id = ?1 ORDER BY e.id DESC LIMIT 1",
            [SESSION], |row| row.get(0)).unwrap();
        let store = tkm_store::Store::from_read_only_connection(db).unwrap();
        let event = store.get_event(event_id).unwrap().unwrap();
        assert_eq!(event.inputs["shadow"], true);
        assert_eq!(event.inputs["redirect"], "reread_error");
        assert!(event.created_at.is_some());
        let ids = event.inputs["decision_ids"].as_array().unwrap();
        assert_eq!(ids.len(), 1, "shadow action needs its exact judge evidence");
        let decision_id = ids[0].as_i64().unwrap();
        assert_eq!(event.evidence_ids, [decision_id]);
        let decision = store.get_decision(decision_id).unwrap().unwrap();
        let session = store.get_session(event.session_id).unwrap().unwrap();
        assert_eq!(session.key.host, Host::Claude);
        assert_eq!(session.key.session_id, SESSION);
        assert_eq!(session.cwd, fixture.init.home);
        assert_eq!(decision.session_id, Some(session.id));
        assert_eq!(decision.repo_key.as_ref(), Some(&session.repo_key));
        assert_eq!(decision.model, MODEL);
        assert_eq!(decision.purpose, "loop_guard");
        assert!(decision.shadow);
        assert!(decision.created_at.is_some());
        assert!(decision.cost_usd > 0.0);
        assert!(
            decision
                .digest
                .as_deref()
                .unwrap()
                .contains("ValueError: boom")
        );
        assert_eq!(decision.answers["stuck"]["noul"], 0.95);
        assert_eq!(decision.answers["progress"]["noul"], 0.1);
        ShadowEvidence {
            event_id,
            repo: session.repo_key.0,
            session: session.key.session_id,
        }
    }

    pub fn wait_for_end(&self, fixture: &ShadowFixture) {
        self.wait_for(fixture, |db| db.query_row(
            "SELECT COUNT(*) FROM sessions WHERE session_id = ?1 AND ended_at IS NOT NULL AND shadow = 1",
            [SESSION], |row| row.get::<_, i64>(0)).unwrap() == 1);
        self.runtime.block_on(self.daemon.settle());
        self.runtime.block_on(self.daemon.flush_now());
    }

    fn wait_for(&self, fixture: &ShadowFixture, condition: impl Fn(&rusqlite::Connection) -> bool) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            self.runtime.block_on(self.daemon.flush_now());
            let db = tkm_store::open_read_only(&fixture.init.paths.db).unwrap();
            if condition(&db) {
                return;
            }
            assert!(
                Instant::now() < deadline,
                "live session did not settle; errors: {:?}",
                fixture.error_lines()
            );
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    pub fn backend_request(&mut self) -> Value {
        let backend = self.backend.take().unwrap();
        self.runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(5), backend)
                .await
                .unwrap()
                .unwrap()
        })
    }

    pub fn shutdown(self) {
        let _ = self.stop.send(());
        self.runtime.block_on(async {
            tokio::time::timeout(Duration::from_secs(5), self.server)
                .await
                .unwrap()
                .unwrap()
                .unwrap();
            self.daemon.shutdown().await;
        });
        self.monitor.abort();
    }
}

async fn serve_decision(listener: &tokio::net::TcpListener) -> Value {
    // One pinned model and one gate crossing: an unexpected extra request cannot escape locally.
    let (mut stream, _) = listener.accept().await.unwrap();
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4096];
    let body = loop {
        let read = stream.read(&mut buffer).await.unwrap();
        assert!(read > 0, "decision client closed an incomplete request");
        request.extend_from_slice(&buffer[..read]);
        assert!(
            request.len() <= 65_536,
            "decision request exceeded fixture limit"
        );
        if let Some(split) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
            let headers = std::str::from_utf8(&request[..split]).unwrap();
            assert!(headers.starts_with("POST /api/alpha/decisions HTTP/1.1"));
            let length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap();
            if request.len() >= split + 4 + length {
                break serde_json::from_slice::<Value>(&request[split + 4..split + 4 + length])
                    .unwrap();
            }
        }
    };
    // Same recorded response shape used by tkmd's successful real-client fault test.
    let response = include_str!("stuck-decision.json");
    stream.write_all(format!(
        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{response}", response.len()
    ).as_bytes()).await.unwrap();
    body
}
