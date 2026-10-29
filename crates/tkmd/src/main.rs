#![forbid(unsafe_code)]
//! `tkmd` entry point. All logic lives in the library.

use std::process::ExitCode;
use std::sync::Arc;

use tkm_config::config::is_disabled;
use tkm_config::paths::Paths;
use tkm_decide::key::{default_key_file, load_key};
use tkmd::daemon::{Daemon, DaemonOptions};
use tkmd::decider::build_decider;
use tkmd::monitor::Monitor;
use tkmd::server::{Bind, Server};
use tkmd::settings::{FileSettings, Settings};

async fn shutdown_signal() {
    use tokio::signal::unix::{SignalKind, signal};
    let (Ok(mut term), Ok(mut int)) = (
        signal(SignalKind::terminate()),
        signal(SignalKind::interrupt()),
    ) else {
        std::future::pending::<()>().await;
        return;
    };
    tokio::select! {
        _ = term.recv() => {}
        _ = int.recv() => {}
    }
}

#[tokio::main]
async fn main() -> ExitCode {
    let Some(paths) = Paths::from_env() else {
        eprintln!("tkmd: cannot resolve the data directory (no home directory)");
        return ExitCode::FAILURE;
    };
    tkmd::logging::init(&paths);
    let env: Vec<(String, String)> = std::env::vars().collect();
    // Daemon-only variables are not configuration keys; keep them out of the config loader.
    let config_env: Vec<(String, String)> = env
        .iter()
        .filter(|(name, _)| name != "TKM_DECIDE_ENDPOINT" && name != "TKM_DELEGATED")
        .cloned()
        .collect();
    if is_disabled(&env) {
        return ExitCode::SUCCESS;
    }
    let var = |name: &str| {
        env.iter()
            .rev()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.clone())
    };

    let mut options = DaemonOptions::new(paths.clone());
    options.settings = Arc::new(FileSettings::new(paths.clone(), config_env.clone()));
    options.env = env.clone();
    let decide = Settings::load(&paths, None, config_env).decide;
    if decide.enabled {
        let home = var("HOME")
            .map(std::path::PathBuf::from)
            .unwrap_or_default();
        let key = load_key(var("OPENROUTER_API_KEY"), &default_key_file(&home), None).ok();
        let endpoint = var("TKM_DECIDE_ENDPOINT");
        options.decider = build_decider(decide, key, endpoint.as_deref(), options.ledger.clone());
    }
    let daemon = Daemon::new(options);

    let server = match Server::bind(daemon.clone()).await {
        Ok(Bind::Bound(server)) => server,
        Ok(Bind::AlreadyRunning) => return ExitCode::SUCCESS,
        Err(error) => {
            daemon.log_error(&format!("cannot bind {}: {error}", paths.socket.display()));
            return ExitCode::FAILURE;
        }
    };
    daemon.start_workers();
    match Monitor::bind(&daemon).await {
        Ok(monitor) => {
            tokio::spawn(monitor.run(daemon.clone()));
        }
        Err(error) => daemon.log_error(&format!("monitor socket: {error}")),
    }
    if let Err(error) = server.run(shutdown_signal()).await {
        daemon.log_error(&format!("server: {error}"));
    }
    daemon.shutdown().await;
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_key_path_reuses_absolute_and_home_relative_paths() {
        let home = std::path::Path::new("/fixture/home");
        assert_eq!(
            key_file(home, Some("/fixture/account/key.env")),
            std::path::PathBuf::from("/fixture/account/key.env")
        );
        assert_eq!(
            key_file(home, Some("~/keys/openrouter.env")),
            home.join("keys/openrouter.env")
        );
        assert_eq!(key_file(home, None), default_key_file(home));
    }
}
