//! Service files and bounded child processes. All executable lookups use the supplied PATH.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use super::state::{self, ServiceRecord};
use crate::{ctx::Ctx, error::CliError};

pub(crate) fn command(ctx: &Ctx<'_>, program: &Path) -> Command {
    let mut command = Command::new(program);
    command
        .env_clear()
        .envs(ctx.env.iter().map(|(key, value)| (key, value)))
        .current_dir(&ctx.cwd)
        .stdin(Stdio::null())
        .stderr(Stdio::null());
    command
}

pub(crate) fn daemon_executable(ctx: &Ctx<'_>) -> Option<PathBuf> {
    state::env(ctx, "TKM_TKMD_BIN")
        .map(PathBuf::from)
        .or_else(|| state::executable(ctx, "tkmd"))
        .or_else(|| {
            let path = std::env::current_exe().ok()?.with_file_name("tkmd");
            path.is_file().then_some(path)
        })
}

pub(crate) fn plan(ctx: &Ctx<'_>) -> Result<ServiceRecord, CliError> {
    let home = state::home(ctx)?;
    let root = if cfg!(target_os = "macos") {
        home
    } else {
        state::env(ctx, "XDG_CONFIG_HOME")
            .filter(|value| !value.is_empty())
            .map_or_else(|| home.join(".config"), PathBuf::from)
    };
    let path = root.join(if cfg!(target_os = "macos") {
        "Library/LaunchAgents/com.tokenimum.tkmd.plist"
    } else {
        "systemd/user/tkmd.service"
    });
    let executable = daemon_executable(ctx).unwrap_or_else(|| PathBuf::from("tkmd"));
    let content = if cfg!(target_os = "macos") {
        tkmd::service::render_launchd_plist(&executable, &ctx.paths)
    } else {
        tkmd::service::render_systemd_unit(&executable, &ctx.paths)
    };
    let owned = !path.try_exists()?;
    let directories = state::missing_directories(std::slice::from_ref(&path));
    Ok(ServiceRecord {
        root,
        path,
        content,
        owned,
        detached: false,
        start_failed: false,
        directories,
    })
}

pub(crate) fn write_service(service: &ServiceRecord) -> Result<(), CliError> {
    tkm_hosts::writer::safe_target(&service.root, &service.path).map_err(state::adapter_error)?;
    if fs::symlink_metadata(&service.path).is_ok_and(|metadata| metadata.file_type().is_symlink()) {
        return Err(state::failure("service path is a symlink"));
    }
    match fs::read_to_string(&service.path) {
        Ok(current) if current == service.content => Ok(()),
        Ok(_) => Err(state::failure(format!(
            "service {} already exists with different contents",
            service.path.display()
        ))),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            if let Some(parent) = service.path.parent() {
                fs::create_dir_all(parent)?;
            }
            write_new_service(service)
        }
        Err(error) => Err(error.into()),
    }
}

fn write_new_service(service: &ServiceRecord) -> Result<(), CliError> {
    use std::sync::atomic::{AtomicU64, Ordering};
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let temporary = service.path.with_file_name(format!(
        ".tkm-service-{}-{}.tmp",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temporary)?;
    let result = (|| {
        file.write_all(service.content.as_bytes())?;
        file.sync_all()?;
        // A hard link publishes complete bytes and refuses a target created concurrently.
        fs::hard_link(&temporary, &service.path)
    })();
    drop(file);
    let _ = fs::remove_file(temporary);
    result.map_err(Into::into)
}

pub(crate) fn start(ctx: &mut Ctx<'_>, service: &mut ServiceRecord) -> Result<(), CliError> {
    service.start_failed = true;
    let manager = if cfg!(target_os = "macos") {
        "launchctl"
    } else {
        "systemctl"
    };
    if let Some(program) = state::executable(ctx, manager) {
        let started = if cfg!(target_os = "macos") {
            bounded_status(ctx, &program, &["load".as_ref(), service.path.as_os_str()])?
        } else {
            let reload = bounded_status(
                ctx,
                &program,
                &["--user".as_ref(), "daemon-reload".as_ref()],
            )?;
            let enable = bounded_status(
                ctx,
                &program,
                &[
                    "--user".as_ref(),
                    "enable".as_ref(),
                    "tkmd.service".as_ref(),
                ],
            )?;
            let start = bounded_status(
                ctx,
                &program,
                &["--user".as_ref(), "start".as_ref(), "tkmd.service".as_ref()],
            )?;
            reload && enable && start
        };
        if started {
            service.detached = false;
            service.start_failed = false;
            return Ok(());
        }
        writeln!(
            ctx.stderr,
            "warn service: {manager} failed; attempting detached tkmd"
        )?;
    } else {
        writeln!(
            ctx.stderr,
            "warn service: {manager} missing; attempting detached tkmd"
        )?;
    }
    let Some(program) = daemon_executable(ctx) else {
        writeln!(ctx.stderr, "warn daemon: tkmd missing; install it on PATH")?;
        return Ok(());
    };
    let mut child = command(ctx, &program);
    child.stdout(Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        child.process_group(0);
    }
    match child.spawn() {
        Ok(child) => {
            service.detached = true;
            service.start_failed = false;
            drop(child);
        }
        Err(_) => writeln!(ctx.stderr, "warn daemon: detached tkmd could not start")?,
    }
    Ok(())
}

pub(crate) fn stop(ctx: &mut Ctx<'_>, service: &ServiceRecord) -> Result<(), CliError> {
    let manager = if cfg!(target_os = "macos") {
        "launchctl"
    } else {
        "systemctl"
    };
    let program = state::executable(ctx, manager);
    if service.detached || service.start_failed && program.is_none() {
        if matches!(
            crate::daemon::probe(&ctx.paths),
            crate::daemon::DaemonState::Running { .. }
        ) || daemon_lock_held(ctx)?
            || hook_socket_live(ctx)?
        {
            return Err(state::failure(
                "detached tkmd is running; stop it with `pkill -x tkmd`, then re-run tkm uninstall",
            ));
        }
        return Ok(());
    }
    if let Some(program) = program {
        let stopped = if cfg!(target_os = "macos") {
            bounded_status(
                ctx,
                &program,
                &["unload".as_ref(), service.path.as_os_str()],
            )?
        } else {
            let stop = bounded_status(
                ctx,
                &program,
                &["--user".as_ref(), "stop".as_ref(), "tkmd.service".as_ref()],
            )?;
            let disable = bounded_status(
                ctx,
                &program,
                &[
                    "--user".as_ref(),
                    "disable".as_ref(),
                    "tkmd.service".as_ref(),
                ],
            )?;
            stop && disable
        };
        if !stopped {
            return Err(state::failure(format!(
                "{manager} could not stop/disable tkmd"
            )));
        }
    } else {
        return Err(state::failure(format!(
            "{manager} missing; cannot stop/disable tkmd"
        )));
    }
    Ok(())
}

fn daemon_lock_held(ctx: &Ctx<'_>) -> Result<bool, CliError> {
    // tkmd holds this advisory lock for its entire hook-server lifetime (ADR 0015).
    let file = match fs::OpenOptions::new()
        .write(true)
        .open(ctx.paths.socket.with_file_name("tkmd.lock"))
    {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => return Err(error.into()),
    };
    match file.try_lock() {
        Ok(()) => Ok(false), // Dropping the file releases our temporary lock.
        Err(fs::TryLockError::WouldBlock) => Ok(true),
        Err(fs::TryLockError::Error(error)) => Err(error.into()),
    }
}

#[cfg(unix)]
fn hook_socket_live(ctx: &Ctx<'_>) -> Result<bool, CliError> {
    let socket = ctx.paths.socket.clone();
    // A worker owns its runtime, so uninstall is also safe inside an existing runtime.
    std::thread::Builder::new()
        .name("tkm-hook-probe".into())
        .spawn(move || -> io::Result<bool> {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_io()
                .enable_time()
                .build()?;
            runtime.block_on(async {
                match tokio::time::timeout(
                    Duration::from_millis(100),
                    tokio::net::UnixStream::connect(socket),
                )
                .await
                {
                    Ok(Err(error))
                        if matches!(
                            error.kind(),
                            io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
                        ) =>
                    {
                        Ok(false)
                    }
                    // A saturated listener is still live. Other errors cannot establish death.
                    Err(_) | Ok(Ok(_)) => Ok(true),
                    Ok(Err(error)) => Err(error),
                }
            })
        })?
        .join()
        .map_err(|_| state::failure("hook socket probe worker failed"))?
        .map_err(Into::into)
}

#[cfg(not(unix))]
fn hook_socket_live(_ctx: &Ctx<'_>) -> Result<bool, CliError> {
    Err(state::failure(
        "cannot verify hook daemon liveness on this platform",
    ))
}

fn bounded_status(
    ctx: &Ctx<'_>,
    program: &Path,
    args: &[&std::ffi::OsStr],
) -> Result<bool, CliError> {
    let Ok(mut child) = command(ctx, program)
        .args(args)
        .stdout(Stdio::null())
        .spawn()
    else {
        return Ok(false);
    };
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status.success());
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(false);
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

pub(crate) fn version(ctx: &Ctx<'_>, program: &Path) -> Result<String, CliError> {
    let mut child = command(ctx, program)
        .arg("--version")
        .stdout(Stdio::piped())
        .spawn()?;
    let output = child
        .stdout
        .take()
        .ok_or_else(|| state::failure("version stdout unavailable"))?;
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    if let Err(error) = std::thread::Builder::new()
        .name("tkm-version-reader".into())
        .spawn(move || {
            let mut bytes = Vec::new();
            let result = output.take(4096).read_to_end(&mut bytes).map(|_| bytes);
            let _ = sender.send(result);
        })
    {
        let _ = child.kill();
        let _ = child.wait();
        return Err(error.into());
    }
    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        if let Some(status) = child.try_wait()? {
            if !status.success() {
                return Err(state::failure("--version exited unsuccessfully"));
            }
            let bytes = receiver
                .recv_timeout(deadline.saturating_duration_since(Instant::now()))
                .map_err(|_| state::failure("--version timed out after 1 s"))??;
            let version: String = String::from_utf8_lossy(&bytes).chars().take(256).collect();
            return Ok(version.replace(['\r', '\n'], " ").trim().to_owned());
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(state::failure("--version timed out after 1 s"));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
