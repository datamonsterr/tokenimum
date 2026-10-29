#![forbid(unsafe_code)]
//! `tkmd` entry point. All logic lives in the library.

use std::process::ExitCode;
use std::sync::Arc;

use tkm_config::config::{LoadInputs, is_disabled, load};
use tkm_config::paths::Paths;
use tkm_decide::key::{default_key_file, load_key};
use tkm_integrations::{
    detect::{AdapterState, Mode, SystemClock},
    otherskill,
    runtime::Integrations,
};
use tkmd::daemon::{Daemon, DaemonOptions};
use tkmd::decider::build_decider;
use tkmd::monitor::Monitor;
use tkmd::server::{Bind, Server};
use tkmd::settings::{FileSettings, Settings};

fn write_integration_status(paths: &Paths, integrations: &Integrations) {
    let statuses: Vec<_> = integrations.status().into_iter().map(|status| {
        let state = match status.state { AdapterState::Enabled => "enabled", AdapterState::Disabled { .. } => "disabled", AdapterState::Backoff { .. } => "backoff" };
        let mode = match status.mode { Mode::Auto => "auto", Mode::On => "on", Mode::Off => "off" };
        serde_json::json!({"name": status.name, "mode": mode, "state": state, "reason": status.reason})
    }).collect();
    let directory = paths.data_dir.join("integrations");
    if std::fs::create_dir_all(&directory).is_ok()
        && let Ok(data) = serde_json::to_vec(&statuses)
    {
        let _ = std::fs::write(directory.join("status.json"), data);
    }
}

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
#[expect(clippy::too_many_lines, reason = "daemon startup wires its components")]
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

    let config = load(&LoadInputs {
        global_path: paths.global_config.clone(),
        repo_root: None,
        env: config_env.clone(),
        flags: Vec::new(),
    })
    .ok();
    let mode = |key: &str| {
        config
            .as_ref()
            .and_then(|c| c.get_str(key))
            .and_then(|s| s.parse::<Mode>().ok())
            .unwrap_or_default()
    };
    let home = var("HOME")
        .map(std::path::PathBuf::from)
        .unwrap_or_default();
    let cwd = std::env::current_dir().unwrap_or_default();
    let integrations = Arc::new(Integrations::new(
        mode("integrations.superorchestrator"),
        mode("integrations.otherskill"),
        std::env::var_os("PATH").unwrap_or_default(),
        home.clone(),
        env.iter().cloned().collect(),
        Arc::new(SystemClock),
    ));
    integrations.redetect_if_due(&cwd);
    write_integration_status(&paths, &integrations);
    let mut options = DaemonOptions::new(paths.clone());
    options.skills = integrations.clone();
    options.escalator = integrations.clone();
    options.ledger.set_spend_sink(
        Arc::new(tkm_integrations::superorchestrator::SpendReporter::new(
            paths
                .data_dir
                .join("integrations/superorchestrator-spend.jsonl"),
        )),
        integrations.clone(),
    );
    options.settings = Arc::new(FileSettings::new(paths.clone(), config_env.clone()));
    options.env = env.clone();
    let decide = Settings::load(&paths, None, config_env).decide;
    if decide.enabled {
        let own_key = default_key_file(&home);
        let key_env = var("OPENROUTER_API_KEY").or_else(|| {
            (config
                .as_ref()
                .and_then(|c| c.get_bool("integrations.otherskill_key_reuse"))
                == Some(true)
                && std::fs::symlink_metadata(&own_key)
                    .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound))
            .then(|| otherskill::read_key(&home))
            .flatten()
        });
        let key = load_key(key_env, &own_key, None).ok();
        let endpoint = var("TKM_DECIDE_ENDPOINT");
        options.decider = build_decider(decide, key, endpoint.as_deref(), options.ledger.clone());
    }
    let daemon = Daemon::new(options);

    let worker_integrations = integrations.clone();
    let worker_paths = paths.clone();
    let worker_daemon = daemon.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_mins(1));
        interval.tick().await;
        loop {
            interval.tick().await;
            let integrations = worker_integrations.clone();
            let paths = worker_paths.clone();
            let cwd = cwd.clone();
            let _ = tokio::task::spawn_blocking(move || {
                integrations.redetect_if_due(&cwd);
                integrations.refresh_runs();
                write_integration_status(&paths, &integrations);
            })
            .await;
            worker_daemon.refresh_integration_settings();
        }
    });

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
