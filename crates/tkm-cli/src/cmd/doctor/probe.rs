//! Bounded, isolated capability probes; only event names leave the scratch HOME.

use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::process::{Command, ExitCode, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde_json::Value;
use tkm_hosts::probe::{expected_events, headless_args};
use tkm_proto::Host;

use super::{line, state};
use crate::{ctx::Ctx, daemon, error::CliError};

const PROMPT: &str = "Read the file probe.txt using a tool, then reply with only tokenimum. Do not change files or ask questions.";

/// Entry point for the isolated recorder symlink, before normal CLI argument parsing.
/// No payload or credentials are read, persisted, or sent to the daemon.
#[must_use]
pub fn record_probe_hook() -> Option<ExitCode> {
    let args: Vec<String> = std::env::args().collect();
    if Path::new(args.first()?).file_name()?.to_str()? != "tkm-hook" {
        return None;
    }
    let destination = std::env::var_os("TKM_PROBE_EVENTS")?;
    let value = |flag: &str| {
        args.windows(2)
            .find(|pair| pair[0] == flag)
            .map(|pair| pair[1].as_str())
    };
    let host = value("--host").and_then(|name| name.parse::<Host>().ok())?;
    let native = value("--host-event").or_else(|| value("--event"))?;
    // This private recorder accepts names only, and cannot forge multiple records via newlines.
    if native.len() <= 100 && !native.contains(['\r', '\n']) {
        let result = OpenOptions::new()
            .create(true)
            .append(true)
            .open(destination)
            .and_then(|mut file| writeln!(file, "{native}"));
        if result.is_err() {
            return Some(ExitCode::FAILURE);
        }
    }
    let _ = std::io::stdout().write_all(host.noop_reply().as_bytes());
    Some(ExitCode::SUCCESS)
}

#[allow(clippy::too_many_lines)] // One probe report includes every independent event result.
pub(super) fn run(ctx: &mut Ctx<'_>, host: Host) -> Result<bool, CliError> {
    let store = daemon::open_db_for_write(&ctx.paths)?;
    let now = i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis(),
    )
    .unwrap_or(i64::MAX);
    let (seen, healthy, detail) = if let Some(args) = headless_args(host) {
        match state::binaries(host)
            .iter()
            .find_map(|name| state::executable(ctx, name))
        {
            Some(binary) => match session(ctx, host, &binary, args) {
                Ok((seen, success)) => (
                    seen,
                    success,
                    if success {
                        "isolated probe completed"
                    } else {
                        "probe failed or timed out"
                    },
                ),
                Err(error) => {
                    line(
                        ctx,
                        "fail",
                        &format!("{} probe", host.as_str()),
                        &error.to_string(),
                    )?;
                    (BTreeSet::new(), false, "isolated probe unavailable")
                }
            },
            None => (BTreeSet::new(), false, "binary unavailable"),
        }
    } else {
        (
            store
                .observed_host_events(host)
                .map_err(state::failure)?
                .into_iter()
                .collect(),
            true,
            "recorded live events (headless hooks unavailable or require persisted trust)",
        )
    };
    for event in expected_events(host) {
        // Historical observations may contain only normalized kinds. Isolated probes always
        // carry native names, preserving success/failure hook independence.
        let arrived = seen.contains(event.native)
            || (headless_args(host).is_none() && seen.contains(event.kind.as_str()));
        store
            .record_host_event_probe(host, event.native, event.kind, arrived, now)
            .map_err(state::failure)?;
        line(
            ctx,
            if arrived { "ok" } else { "warn" },
            &format!("{} {}", host.as_str(), event.native),
            if arrived {
                "arrived"
            } else {
                "idle-fold (not observed)"
            },
        )?;
    }
    // Partial event coverage is per-event fallback, not an installation failure.
    // A broken probe command disables the host live path until a subsequent passing probe.
    if !healthy {
        store
            .record_host_health(host, false)
            .map_err(state::failure)?;
    }
    line(
        ctx,
        if healthy { "ok" } else { "fail" },
        &format!("{} probe", host.as_str()),
        detail,
    )?;
    Ok(healthy)
}

fn rewrite_json(value: &mut Value, native: Option<&str>, host: Host) {
    match value {
        Value::Object(object) => {
            for (key, value) in object {
                let native = expected_events(host)
                    .iter()
                    .find(|event| event.native == key.as_str())
                    .map(|event| event.native)
                    .or(native);
                if key == "command"
                    && let (Some(native), Some(command)) = (native, value.as_str())
                {
                    *value = Value::String(format!("{command} --host-event {native}"));
                } else {
                    rewrite_json(value, native, host);
                }
            }
        }
        Value::Array(array) => {
            for value in array {
                rewrite_json(value, native, host);
            }
        }
        _ => {}
    }
}

fn configure_recorders(paths: &[std::path::PathBuf], host: Host) -> Result<(), CliError> {
    for path in paths {
        if path
            .extension()
            .is_some_and(|extension| extension == "json")
        {
            let mut value: Value =
                serde_json::from_slice(&fs::read(path)?).map_err(state::failure)?;
            rewrite_json(&mut value, None, host);
            fs::write(path, serde_json::to_vec(&value).map_err(state::failure)?)?;
        } else if host == Host::Kimi
            && path
                .extension()
                .is_some_and(|extension| extension == "toml")
        {
            let mut doc = fs::read_to_string(path)?
                .parse::<toml_edit::DocumentMut>()
                .map_err(state::failure)?;
            if let Some(hooks) = doc
                .get_mut("hooks")
                .and_then(toml_edit::Item::as_array_of_tables_mut)
            {
                for hook in hooks.iter_mut() {
                    if let (Some(native), Some(command)) = (
                        hook.get("event").and_then(toml_edit::Item::as_str),
                        hook.get("command").and_then(toml_edit::Item::as_str),
                    ) {
                        let command = format!("{command} --host-event {native}");
                        hook.insert("command", toml_edit::value(command));
                    }
                }
            }
            fs::write(path, doc.to_string())?;
        }
    }
    Ok(())
}

fn session(
    ctx: &Ctx<'_>,
    host: Host,
    binary: &Path,
    args: &[&str],
) -> Result<(BTreeSet<String>, bool), CliError> {
    let scratch = tempfile::Builder::new()
        .prefix("tkm-doctor-probe-")
        .tempdir()?;
    let home = scratch.path().join("home");
    let work = scratch.path().join("work");
    let bin = scratch.path().join("bin");
    for directory in [&home, &work, &bin] {
        fs::create_dir_all(directory)?;
    }
    fs::write(work.join("probe.txt"), "tokenimum\n")?;
    let adapter_home = state::host_home(&home, host);
    let report = tkmd::registry::AdapterRegistry::standard()
        .get(host)
        .install(&state::context(&adapter_home, false))
        .map_err(state::adapter_error)?;
    configure_recorders(&report.changed_files, host)?;
    // The real executable's recorder entry point avoids shell scripts and never starts tkmd.
    std::os::unix::fs::symlink(std::env::current_exe()?, bin.join("tkm-hook"))?;
    let arrivals = scratch.path().join("events");
    let path = std::env::join_paths(std::iter::once(bin).chain(std::env::split_paths(
        state::env(ctx, "PATH").unwrap_or_default(),
    )))
    .map_err(state::failure)?;
    let mut command = Command::new(binary);
    command
        .args(args)
        .arg(PROMPT)
        .current_dir(&work)
        .env_clear()
        .envs(ctx.env.iter().cloned())
        .env("HOME", &home)
        .env("PATH", path)
        .env("TKM_PROBE_EVENTS", &arrivals)
        .env("CLAUDE_CONFIG_DIR", home.join(".claude"))
        .env("CODEX_HOME", home.join(".codex"))
        .env("KIMI_SHARE_DIR", home.join(".kimi"))
        .env("CODEWHALE_HOME", home.join(".codewhale"))
        .env("PI_CODING_AGENT_DIR", home.join(".pi/agent"))
        .env_remove("TKM_DISABLE")
        .env_remove("TKM_CONFIG")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for (name, directory) in [
        ("XDG_CONFIG_HOME", ".config"),
        ("XDG_DATA_HOME", ".local/share"),
        ("XDG_STATE_HOME", ".local/state"),
        ("XDG_CACHE_HOME", ".cache"),
        ("XDG_RUNTIME_DIR", ".run"),
    ] {
        let directory = home.join(directory);
        fs::create_dir_all(&directory)?;
        command.env(name, directory);
    }
    let mut child = command.spawn()?;
    let deadline = Instant::now() + Duration::from_secs(30);
    let success = loop {
        if let Some(status) = child.try_wait()? {
            break status.success();
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            break false;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    let text = fs::read_to_string(arrivals).unwrap_or_default();
    let seen = text
        .lines()
        .filter(|line| {
            expected_events(host)
                .iter()
                .any(|event| event.native == *line)
        })
        .map(str::to_owned)
        .collect();
    Ok((seen, success))
}
