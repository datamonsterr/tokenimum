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
        self.restore_tool_with_hook(checkout, host, tool, || {})
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
impl DaemonActions {
    fn restore_tool_with_hook(
        &self,
        checkout: &str,
        host: &str,
        tool: &str,
        after_read: impl FnOnce(),
    ) -> Result<String, String> {
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
        after_read();
        let requested = BTreeSet::from([tool.to_owned()]);
        tkm_hosts::writer::deny::update_deny(path, &requested, &BTreeSet::new(), false)
            .map_err(|e| e.to_string())?;
        store
            .mark_denylist_restored(path, host, tool)
            .map_err(|e| e.to_string())?;
        Ok(format!("Restored {tool} in {checkout}"))
    }
}
async fn resync_after_lag<E, F>(
    monitor: &std::sync::RwLock<tkm_ui::model::MonitorState>,
    mut load: impl FnMut() -> F,
) where
    F: std::future::Future<Output = Result<tkm_ui::model::MonitorState, E>>,
{
    if let Ok(mut state) = monitor.write() {
        state.degraded = true;
    }
    let mut delay = std::time::Duration::from_millis(250);
    loop {
        if let Ok(snapshot) = load().await {
            if let Ok(mut state) = monitor.write() {
                *state = snapshot;
            }
            break;
        }
        tokio::time::sleep(delay).await;
        delay = (delay * 2).min(std::time::Duration::from_secs(5));
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
    state: WebState,
    file: DiscoveryFile,
}
impl WebServer {
    /// Start a dashboard; failures leave the daemon unaffected.
    pub async fn start(daemon: Arc<Daemon>, paths: &Paths, bind: &str) -> Result<Self, String> {
        Self::start_with_actions(daemon, paths, bind, None).await
    }
    async fn start_with_actions(
        daemon: Arc<Daemon>,
        paths: &Paths,
        bind: &str,
        actions: Option<Arc<dyn Actions>>,
    ) -> Result<Self, String> {
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
            actions.unwrap_or_else(|| {
                Arc::new(DaemonActions {
                    db: paths.db.clone(),
                    daemon: Arc::clone(&daemon),
                    deny_lock: Mutex::new(()),
                })
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
                            && let Ok(snapshot) = feed_state.reload_snapshot().await
                            && let Ok(mut state) = feed_state.monitor.write()
                        {
                            state.unknown_session_costs = snapshot.unknown_session_costs;
                            state.unknown_decision_costs = snapshot.unknown_decision_costs;
                            state.savings = snapshot.savings;
                            state.spend_days = snapshot.spend_days;
                        }
                        if let Ok(mut state) = feed_state.monitor.write() {
                            state.apply(message);
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {
                        resync_after_lag(&feed_state.monitor, || feed_state.reload_snapshot())
                            .await;
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => break,
                }
            }
        });
        let (stop, shutdown) = tokio::sync::oneshot::channel();
        let app = tkm_ui::web::router(state.clone());
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
            state,
            file: DiscoveryFile(file),
        })
    }
    /// Stop the server and remove the discovery file.
    pub async fn shutdown(self) {
        self.state.close().await;
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

#[cfg(test)]
mod tests {
    use super::*;
    use tkm_core::model::Timestamp;
    use tkm_store::DenylistEntry;

    #[test]
    fn restore_does_not_readd_manually_removed_owned_rule() -> Result<(), Box<dyn std::error::Error>>
    {
        let dir = tempfile::tempdir()?;
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q"])
                .current_dir(dir.path())
                .status()?
                .success()
        );
        let paths = Paths::from_dirs(
            dir.path(),
            Some(dir.path()),
            Some(dir.path()),
            Some(dir.path()),
        );
        paths.ensure_runtime_dir()?;
        let store = tkm_store::Store::open(&paths.db)?;
        for tool in ["A", "B"] {
            store.record_denylist_write(&DenylistEntry {
                checkout_path: dir.path().into(),
                host: Host::Claude,
                tool: tool.into(),
                written_at: Timestamp(1),
                restored: false,
            })?;
        }
        std::fs::create_dir_all(dir.path().join(".claude"))?;
        let file = dir.path().join(".claude/settings.local.json");
        std::fs::write(&file, r#"{"permissions":{"deny":["A"]}}"#)?;
        let actions = DaemonActions {
            db: paths.db.clone(),
            daemon: Daemon::new(crate::daemon::DaemonOptions::new(paths)),
            deny_lock: Mutex::new(()),
        };
        actions
            .restore_tool(&dir.path().to_string_lossy(), "claude", "A")
            .map_err(std::io::Error::other)?;
        let value: serde_json::Value = serde_json::from_slice(&std::fs::read(file)?)?;
        assert_eq!(value["permissions"]["deny"], serde_json::json!([]));
        Ok(())
    }
    #[test]
    fn restore_preserves_manual_removal_after_settings_read()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        assert!(
            std::process::Command::new("git")
                .args(["init", "-q"])
                .current_dir(dir.path())
                .status()?
                .success()
        );
        let paths = Paths::from_dirs(
            dir.path(),
            Some(dir.path()),
            Some(dir.path()),
            Some(dir.path()),
        );
        paths.ensure_runtime_dir()?;
        let store = tkm_store::Store::open(&paths.db)?;
        for tool in ["A", "B"] {
            store.record_denylist_write(&DenylistEntry {
                checkout_path: dir.path().into(),
                host: Host::Claude,
                tool: tool.into(),
                written_at: Timestamp(1),
                restored: false,
            })?;
        }
        std::fs::create_dir_all(dir.path().join(".claude"))?;
        let file = dir.path().join(".claude/settings.local.json");
        std::fs::write(&file, r#"{"permissions":{"deny":["A","B"]}}"#)?;
        let actions = DaemonActions {
            db: paths.db.clone(),
            daemon: Daemon::new(crate::daemon::DaemonOptions::new(paths)),
            deny_lock: Mutex::new(()),
        };
        let mut edit_result = Ok(());
        actions
            .restore_tool_with_hook(&dir.path().to_string_lossy(), "claude", "A", || {
                edit_result = std::fs::write(&file, r#"{"permissions":{"deny":["A"]}}"#);
            })
            .map_err(std::io::Error::other)?;
        edit_result?;
        let value: serde_json::Value = serde_json::from_slice(&std::fs::read(file)?)?;
        assert_eq!(value["permissions"]["deny"], serde_json::json!([]));
        Ok(())
    }
    #[test]
    fn forgetting_fact_publishes_retirement() -> Result<(), Box<dyn std::error::Error>> {
        use tkm_store::{FactRecord, FactState, Repo};
        let dir = tempfile::tempdir()?;
        let paths = Paths::from_dirs(
            dir.path(),
            Some(dir.path()),
            Some(dir.path()),
            Some(dir.path()),
        );
        paths.ensure_runtime_dir()?;
        let store = tkm_store::Store::open(&paths.db)?;
        let repo = RepoKey("repo".into());
        store.upsert_repo(&Repo {
            repo_key: repo.clone(),
            display_name: "repo".into(),
            first_seen: Timestamp(1),
            last_seen: Timestamp(1),
            delegated_runs: 0,
        })?;
        store.insert_candidate_fact(&FactRecord {
            repo_key: repo,
            fact_id: "fact".into(),
            family: "boundary".into(),
            scope: "neutral".into(),
            text_params: serde_json::json!({}),
            state: FactState::Candidate,
            score: 1.0,
            confirmations: 1,
            contradictions: 0,
            fire_count: 0,
            fire_sessions: 0,
            state_changed_at: None,
        })?;
        let daemon = Daemon::new(crate::daemon::DaemonOptions::new(paths.clone()));
        let mut receiver = daemon.subscribe();
        let actions = DaemonActions {
            db: paths.db,
            daemon,
            deny_lock: Mutex::new(()),
        };
        actions
            .forget_fact("repo", "fact")
            .map_err(std::io::Error::other)?;
        assert_eq!(
            receiver.try_recv()?,
            tkm_proto::monitor::MonitorMessage::Fact(tkm_proto::monitor::FactNotice {
                repo: "repo".into(),
                fact_id: "fact".into(),
                kind: "retired".into()
            })
        );
        Ok(())
    }
    #[tokio::test]
    async fn lag_marks_degraded_until_fake_snapshot_replaces_state()
    -> Result<(), Box<dyn std::error::Error>> {
        let (sender, mut receiver) = tokio::sync::broadcast::channel(1);
        sender.send(tkm_proto::monitor::MonitorMessage::Hello {
            version: 1,
            daemon_version: None,
            started_at_ms: None,
        })?;
        sender.send(tkm_proto::monitor::MonitorMessage::Hello {
            version: 1,
            daemon_version: None,
            started_at_ms: None,
        })?;
        assert!(matches!(
            receiver.recv().await,
            Err(tokio::sync::broadcast::error::RecvError::Lagged(_))
        ));
        let monitor = Arc::new(std::sync::RwLock::new(
            tkm_ui::model::MonitorState::default(),
        ));
        let (tx, rx) = tokio::sync::oneshot::channel();
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let shared = Arc::clone(&monitor);
        let task = tokio::spawn(async move {
            let mut channels = Some((ready_tx, rx));
            resync_after_lag(&shared, || {
                let channels = channels.take();
                async move {
                    if let Some((ready_tx, rx)) = channels {
                        let _ = ready_tx.send(());
                        rx.await.map_err(|_| ())
                    } else {
                        Err(())
                    }
                }
            })
            .await;
        });
        ready_rx.await?;
        assert!(
            monitor
                .read()
                .map_err(|_| std::io::Error::other("lock"))?
                .degraded
        );
        let mut snapshot = tkm_ui::model::MonitorState::default();
        snapshot.savings.push(tkm_ui::model::Savings {
            repo: "repo".into(),
            host: "claude".into(),
            cost: 2.0,
            shadow: false,
        });
        tx.send(snapshot)
            .map_err(|_| std::io::Error::other("send"))?;
        task.await?;
        let state = monitor.read().map_err(|_| std::io::Error::other("lock"))?;
        assert!(!state.degraded);
        assert_eq!(state.savings[0].cost, 2.0);
        Ok(())
    }
    #[tokio::test]
    async fn shutdown_rejects_stalled_authenticated_body() -> Result<(), Box<dyn std::error::Error>>
    {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        struct Fake(Arc<AtomicUsize>);
        impl Actions for Fake {
            fn forget_fact(&self, _: &str, _: &str) -> Result<String, String> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(String::new())
            }
            fn restore_tool(&self, _: &str, _: &str, _: &str) -> Result<String, String> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(String::new())
            }
            fn switch_preset(&self, _: &str, _: &str) -> Result<String, String> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Ok(String::new())
            }
        }
        let dir = tempfile::tempdir()?;
        let paths = Paths::from_dirs(
            dir.path(),
            Some(dir.path()),
            Some(dir.path()),
            Some(dir.path()),
        );
        paths.ensure_runtime_dir()?;
        let daemon = Daemon::new(crate::daemon::DaemonOptions::new(paths.clone()));
        let calls = Arc::new(AtomicUsize::new(0));
        let web = WebServer::start_with_actions(
            daemon,
            &paths,
            "127.0.0.1:0",
            Some(Arc::new(Fake(Arc::clone(&calls)))),
        )
        .await
        .map_err(io::Error::other)?;
        let file = paths.socket.with_file_name("tkmd-web.json");
        let info: serde_json::Value = serde_json::from_slice(&std::fs::read(&file)?)?;
        let host = info["url"]
            .as_str()
            .ok_or("url")?
            .trim_start_matches("http://")
            .trim_end_matches('/');
        let token = info["token"].as_str().ok_or("token")?;
        let body = r#"{"repo":"repo","fact_id":"fact"}"#;
        let mut client = tokio::net::TcpStream::connect(host).await?;
        client.write_all(format!("POST /api/actions/forget_fact HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {token}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n", body.len()).as_bytes()).await?;
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        web.shutdown().await;
        assert!(!file.exists());
        let _ = client.write_all(body.as_bytes()).await;
        let mut response = Vec::new();
        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(1),
            client.read_to_end(&mut response),
        )
        .await;
        assert!(!String::from_utf8_lossy(&response).starts_with("HTTP/1.1 200"));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        Ok(())
    }
    #[tokio::test]
    async fn lag_recovery_retries_failed_snapshot() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let monitor = std::sync::RwLock::new(tkm_ui::model::MonitorState::default());
        let attempts = AtomicUsize::new(0);
        resync_after_lag(&monitor, || {
            let attempt = attempts.fetch_add(1, Ordering::SeqCst);
            async move {
                if attempt == 0 {
                    Err(())
                } else {
                    Ok(tkm_ui::model::MonitorState::default())
                }
            }
        })
        .await;
        assert_eq!(attempts.load(Ordering::SeqCst), 2);
        assert!(monitor.read().is_ok_and(|state| !state.degraded));
    }
}
