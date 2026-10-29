//! Dashboard lifecycle and daemon-backed actions.
use crate::daemon::Daemon;
use std::{
    collections::BTreeSet,
    io::{self, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use tkm_config::{
    config::{ConfigFile, Layer},
    paths::Paths,
};
use tkm_core::model::{Host, RepoKey};
use tkm_ui::web::{Actions, WebConfig, WebState};

struct DaemonActions {
    db: PathBuf,
    daemon: Arc<Daemon>,
    deny_lock: Mutex<()>,
}
impl Actions for DaemonActions {
    fn forget_fact(&self, repo: &str, fact_id: &str) -> Result<String, String> {
        let store = tkm_store::Store::open(&self.db).map_err(|e| e.to_string())?;
        store
            .transition_fact(
                &RepoKey(repo.into()),
                fact_id,
                tkm_store::FactState::Retired,
            )
            .map_err(|e| e.to_string())?;
        self.daemon.publish(|| {
            tkm_proto::monitor::MonitorMessage::Fact(tkm_proto::monitor::FactNotice {
                repo: repo.into(),
                fact_id: fact_id.into(),
                kind: "retired".into(),
            })
        });
        Ok(format!("Forgot fact {fact_id} in {repo}"))
    }
    fn restore_tool(&self, checkout: &str, host: &str, tool: &str) -> Result<String, String> {
        let _guard = self.deny_lock.lock().map_err(|e| e.to_string())?;
        let host: Host = host.parse().map_err(|_| "invalid host".to_owned())?;
        if host != Host::Claude {
            return Err("this host does not support deny-list writes".into());
        }
        let path = Path::new(checkout);
        let store = tkm_store::Store::open(&self.db).map_err(|e| e.to_string())?;
        let entries = store.list_denylist(path).map_err(|e| e.to_string())?;
        let owned: BTreeSet<String> = entries
            .iter()
            .filter(|entry| entry.host == host && !entry.restored)
            .map(|entry| entry.tool.clone())
            .collect();
        if !owned.contains(tool) {
            return Err("tool is not an active owned deny rule".into());
        }
        let mut plan = tkm_core::prune::denylist::DenyState::default();
        plan.applied.insert(checkout.to_owned(), owned.clone());
        let requested = BTreeSet::from([tool.to_owned()]);
        let (_, delta) = tkm_core::prune::denylist::restore(&plan, checkout, &owned, &requested);
        let settings = path.join(".claude/settings.local.json");
        let current: serde_json::Value = match std::fs::read_to_string(settings) {
            Ok(contents) => {
                serde_json::from_str(&contents).map_err(|e| format!("invalid settings: {e}"))?
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => serde_json::json!({}),
            Err(e) => return Err(e.to_string()),
        };
        let present: BTreeSet<String> = current
            .pointer("/permissions/deny")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(serde_json::Value::as_str)
            .map(str::to_owned)
            .collect();
        let desired = owned
            .intersection(&present)
            .filter(|entry| !delta.remove.contains(*entry))
            .cloned()
            .collect();
        tkm_hosts::writer::deny::update_deny(path, &owned, &desired, false)
            .map_err(|e| e.to_string())?;
        store
            .mark_denylist_restored(path, host, tool)
            .map_err(|e| e.to_string())?;
        Ok(format!("Restored {tool} in {checkout}"))
    }
    fn switch_preset(&self, repo: &str, preset: &str) -> Result<String, String> {
        let conn = tkm_store::open_read_only(&self.db).map_err(|e| e.to_string())?;
        let cwd: String = conn
            .query_row(
                "SELECT cwd FROM sessions WHERE repo_key=?1 ORDER BY started_at DESC LIMIT 1",
                [repo],
                |r| r.get(0),
            )
            .map_err(|e| e.to_string())?;
        let identity = tkm_config::repo::repo_identity(Path::new(&cwd), "", None);
        let root = identity.root;
        if identity.key.0 != repo {
            return Err("checkout no longer matches repository".into());
        }
        let mut config = ConfigFile::load(root.join(".tokenimum.toml"), Layer::Repo)
            .map_err(|e| e.to_string())?;
        config.set("preset", preset).map_err(|e| e.to_string())?;
        config.save().map_err(|e| e.to_string())?;
        Ok(format!("Changed {repo} to {preset}"))
    }
}
struct DiscoveryFile(PathBuf);
impl Drop for DiscoveryFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}
/// Running dashboard and its private discovery file.
pub struct WebServer {
    stop: tokio::sync::oneshot::Sender<()>,
    task: tokio::task::JoinHandle<()>,
    feed: tokio::task::JoinHandle<()>,
    file: DiscoveryFile,
}
impl WebServer {
    /// Start a dashboard; failures leave the daemon unaffected.
    pub async fn start(daemon: Arc<Daemon>, paths: &Paths, bind: &str) -> Result<Self, String> {
        let address = tkm_ui::web::validate_bind(bind).map_err(|e| e.to_string())?;
        let token = tkm_ui::web::generate_token().map_err(|e| e.to_string())?;
        let listener = tokio::net::TcpListener::bind(address)
            .await
            .map_err(|e| e.to_string())?;
        let address = listener.local_addr().map_err(|e| e.to_string())?;
        // Ensure a fresh installation has a database before opening its read-only snapshot.
        tkm_store::Store::open(&paths.db).map_err(|e| e.to_string())?;
        let conn = tkm_store::open_read_only(&paths.db).map_err(|e| e.to_string())?;
        let mut receiver = daemon.subscribe();
        let snapshot = tkm_ui::model::load_snapshot(&conn).map_err(|e| e.to_string())?;
        let state = WebState::new(
            WebConfig {
                bind: address,
                token: token.clone(),
            },
            snapshot,
            Arc::new(DaemonActions {
                db: paths.db.clone(),
                daemon: Arc::clone(&daemon),
                deny_lock: Mutex::new(()),
            }),
            paths.db.clone(),
        );
        let feed_state = state.clone();
        let file = paths.socket.with_file_name("tkmd-web.json");
        let temp = paths
            .socket
            .with_file_name(format!("tkmd-web-{}.tmp", std::process::id()));
        let contents = serde_json::to_vec(&serde_json::json!({"url":format!("http://{address}/"),"token":token,"pid":std::process::id()})).map_err(|e| e.to_string())?;
        let mut output = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        let mut write = || -> io::Result<()> {
            output.set_permissions(std::fs::Permissions::from_mode(0o600))?;
            output.write_all(&contents)?;
            output.sync_all()?;
            std::fs::rename(&temp, &file)?;
            Ok(())
        };
        if let Err(error) = write() {
            let _ = std::fs::remove_file(&temp);
            return Err(error.to_string());
        }
        let feed = tokio::spawn(async move {
            loop {
                match receiver.recv().await {
                    Ok(message) => {
                        if matches!(&message, tkm_proto::monitor::MonitorMessage::Session(update) if update.phase == "folded")
                        {
                            if let Ok(snapshot) = feed_state.reload_snapshot().await {
                                if let Ok(mut state) = feed_state.monitor.write() {
                                    state.savings = snapshot.savings;
                                }
                            }
                        }
                        if let Ok(mut state) = feed_state.monitor.write() {
                            state.apply(message);
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        if let Ok(mut state) = feed_state.monitor.write() {
                            state.degraded = true;
                        }
                        if let Ok(snapshot) = feed_state.reload_snapshot().await {
                            if let Ok(mut state) = feed_state.monitor.write() {
                                *state = snapshot;
                            }
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
        let (stop, shutdown) = tokio::sync::oneshot::channel();
        let app = tkm_ui::web::router(state);
        let task = tokio::spawn(async move {
            if let Err(error) = tkm_ui::web::serve(listener, app, async {
                let _ = shutdown.await;
            })
            .await
            {
                daemon.log_error(&format!("web server: {error}"));
            }
        });
        Ok(Self {
            stop,
            task,
            feed,
            file: DiscoveryFile(file),
        })
    }
    /// Stop the server and remove the discovery file.
    pub async fn shutdown(self) {
        let _ = self.stop.send(());
        let mut task = self.task;
        if tokio::time::timeout(std::time::Duration::from_secs(2), &mut task)
            .await
            .is_err()
        {
            task.abort();
            let _ = task.await;
        }
        self.feed.abort();
        let _ = self.feed.await;
        drop(self.file);
    }
}
