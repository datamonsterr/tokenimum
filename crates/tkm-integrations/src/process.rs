use std::{
    ffi::OsStr,
    io::{self, Read},
    path::PathBuf,
    process::{Command, Stdio},
    sync::mpsc,
    thread,
    time::{Duration, Instant},
};

pub(crate) fn binary(path: &OsStr, name: &str) -> Option<PathBuf> {
    std::env::split_paths(path)
        .filter_map(|p| std::path::absolute(p.join(name)).ok())
        .find(|p| {
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                p.metadata()
                    .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
            }
            #[cfg(not(unix))]
            {
                p.is_file()
            }
        })
}
const MAX_OUTPUT: u64 = 1024 * 1024;

pub(crate) fn kill_group(child: &mut std::process::Child) {
    #[cfg(unix)]
    {
        let _ = Command::new("/bin/kill")
            .args(["-KILL", "--", &format!("-{}", child.id())])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    let _ = child.kill();
}

pub(crate) fn prepare_group(cmd: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
}

pub(crate) fn strip_orchestrator_env(cmd: &mut Command) {
    let keys: Vec<_> = std::env::vars_os()
        .map(|(key, _)| key)
        .chain(cmd.get_envs().map(|(key, _)| key.to_os_string()))
        .filter(|key| key.to_string_lossy().starts_with("SUPERORCHESTRATOR_"))
        .collect();
    for key in keys {
        cmd.env_remove(key);
    }
}

pub(crate) fn output(mut cmd: Command, timeout: Duration) -> io::Result<Vec<u8>> {
    strip_orchestrator_env(&mut cmd);
    prepare_group(&mut cmd);
    let mut child = cmd.stdout(Stdio::piped()).stderr(Stdio::null()).spawn()?;
    let stdout = child
        .stdout
        .take()
        .ok_or_else(|| io::Error::other("missing stdout"))?;
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let mut bytes = Vec::new();
        let result = stdout
            .take(MAX_OUTPUT + 1)
            .read_to_end(&mut bytes)
            .and_then(|_| {
                if bytes.len() as u64 > MAX_OUTPUT {
                    Err(io::Error::other("process output too large"))
                } else {
                    Ok(bytes)
                }
            });
        let _ = tx.send(result);
    });
    let deadline = Instant::now() + timeout;
    loop {
        match rx.try_recv() {
            Ok(Err(error)) => {
                kill_group(&mut child);
                let _ = child.wait();
                return Err(error);
            }
            Ok(Ok(bytes)) => {
                while Instant::now() < deadline {
                    if let Some(status) = child.try_wait()? {
                        return if status.success() {
                            Ok(bytes)
                        } else {
                            Err(io::Error::other("process failed"))
                        };
                    }
                    thread::sleep(Duration::from_millis(10));
                }
                kill_group(&mut child);
                let _ = child.wait();
                return Err(io::Error::new(io::ErrorKind::TimedOut, "process timeout"));
            }
            Err(mpsc::TryRecvError::Disconnected) => {
                return Err(io::Error::other("output reader failed"));
            }
            Err(mpsc::TryRecvError::Empty) => {}
        }
        if Instant::now() >= deadline {
            kill_group(&mut child);
            let _ = child.wait();
            return Err(io::Error::new(io::ErrorKind::TimedOut, "process timeout"));
        }
        thread::sleep(Duration::from_millis(10));
    }
}
