//! Entry point for `tkm top`.
use tkm_config::paths::Paths;
/// Run the monitor.
///
/// # Errors
/// Returns an error when the snapshot or terminal cannot be used.
pub fn run_top(paths: &Paths) -> Result<(), crate::Error> {
    crate::tui::run(paths)
}

/// Options for opening the local dashboard.
#[derive(Debug, Clone, Default)]
pub struct DashboardArgs {
    /// Print the URL without opening a browser.
    pub no_open: bool,
    /// Whether the effective configuration sets `ui.web.enabled = true`.
    pub web_enabled: bool,
}

trait DashboardProcess {
    fn socket_live(&self, socket: &std::path::Path) -> bool;
    fn spawn_daemon(&self) -> std::io::Result<()>;
    fn open_browser(&self, url: &str) -> std::io::Result<()>;
}

/// Contents of `tkmd-web.json`: a live dashboard, or why the daemon cannot serve one.
#[derive(serde::Deserialize)]
#[serde(untagged)]
enum Discovery {
    Live { url: String, token: String },
    Unavailable { unavailable: String },
}

fn unavailable(message: &str) -> crate::Error {
    std::io::Error::new(std::io::ErrorKind::NotFound, message.to_owned()).into()
}

struct SystemProcess;
impl DashboardProcess for SystemProcess {
    fn socket_live(&self, socket: &std::path::Path) -> bool {
        std::os::unix::net::UnixStream::connect(socket).is_ok()
    }
    fn spawn_daemon(&self) -> std::io::Result<()> {
        let sibling = std::env::current_exe()?.with_file_name("tkmd");
        let binary = if sibling.is_file() {
            sibling
        } else {
            "tkmd".into()
        };
        std::process::Command::new(binary)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
        Ok(())
    }
    fn open_browser(&self, url: &str) -> std::io::Result<()> {
        let opener = if cfg!(target_os = "macos") {
            "open"
        } else {
            "xdg-open"
        };
        std::process::Command::new(opener)
            .arg(url)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()?;
        Ok(())
    }
}

/// Start the daemon if needed and return its dashboard URL.
///
/// # Errors
/// Returns an error if the daemon cannot start, discovery fails, or the dashboard is unavailable.
pub fn run_dashboard(paths: &Paths, args: &DashboardArgs) -> Result<String, crate::Error> {
    run_dashboard_with(
        paths,
        args,
        &SystemProcess,
        std::time::Duration::from_secs(2),
    )
}

fn run_dashboard_with(
    paths: &Paths,
    args: &DashboardArgs,
    process: &impl DashboardProcess,
    timeout: std::time::Duration,
) -> Result<String, crate::Error> {
    if !args.web_enabled {
        return Err(unavailable(
            "Dashboard disabled: run `tkm config set ui.web.enabled true`",
        ));
    }
    let file = paths.socket.with_file_name("tkmd-web.json");
    if !process.socket_live(&paths.socket) {
        match std::fs::remove_file(&file) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        process.spawn_daemon()?;
    }
    let started = std::time::Instant::now();
    let contents = loop {
        match std::fs::read(&file) {
            Ok(contents) => break contents,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
        let remaining = timeout.saturating_sub(started.elapsed());
        if remaining.is_zero() {
            return Err(unavailable(
                "Dashboard unavailable: tkmd has not started it; restart tkmd to apply `ui.web.enabled`, or check the daemon log",
            ));
        }
        std::thread::sleep(remaining.min(std::time::Duration::from_millis(50)));
    };
    let url = match serde_json::from_slice(&contents)? {
        Discovery::Live { url, token } => format!("{url}#token={token}"),
        Discovery::Unavailable {
            unavailable: reason,
        } if reason == "feature" => {
            return Err(unavailable(
                "Dashboard unavailable: this tkmd was built without the `web` feature; install a release build or rebuild with `cargo build -p tkmd --features web`",
            ));
        }
        Discovery::Unavailable {
            unavailable: reason,
        } => {
            return Err(unavailable(&format!("Dashboard unavailable: {reason}")));
        }
    };
    if !args.no_open
        && let Err(error) = process.open_browser(&url)
    {
        eprintln!("warning: could not open dashboard browser: {error}");
    }
    Ok(url)
}

#[cfg(test)]
mod dashboard_tests {
    use super::*;
    use std::{cell::Cell, io, path::Path, time::Duration};

    #[derive(Default)]
    struct Fake {
        live: bool,
        spawned: Cell<bool>,
        opened: Cell<bool>,
    }
    impl DashboardProcess for Fake {
        fn socket_live(&self, _: &Path) -> bool {
            self.live
        }
        fn spawn_daemon(&self) -> io::Result<()> {
            self.spawned.set(true);
            Ok(())
        }
        fn open_browser(&self, _: &str) -> io::Result<()> {
            self.opened.set(true);
            Ok(())
        }
    }
    fn paths(dir: &Path) -> Paths {
        let mut paths = Paths::from_dirs(dir, None, None, None);
        paths.socket = dir.join("tkmd.sock");
        paths
    }
    #[test]
    fn discovery_file_returns_fragment_url_and_opens_browser()
    -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        std::fs::write(
            dir.path().join("tkmd-web.json"),
            r#"{"url":"http://127.0.0.1:1234/","token":"abc"}"#,
        )?;
        let fake = Fake {
            live: true,
            ..Fake::default()
        };
        let url = run_dashboard_with(
            &paths(dir.path()),
            &DashboardArgs {
                no_open: false,
                web_enabled: true,
            },
            &fake,
            Duration::ZERO,
        )?;
        assert_eq!(url, "http://127.0.0.1:1234/#token=abc");
        assert!(fake.opened.get());
        assert!(!fake.spawned.get());
        Ok(())
    }
    fn dashboard_error(
        dir: &Path,
        fake: &Fake,
        web_enabled: bool,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let Err(error) = run_dashboard_with(
            &paths(dir),
            &DashboardArgs {
                no_open: true,
                web_enabled,
            },
            fake,
            Duration::ZERO,
        ) else {
            return Err("missing dashboard should fail".into());
        };
        Ok(error.to_string())
    }
    #[test]
    fn disabled_config_names_only_the_flag() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let fake = Fake::default();
        let error = dashboard_error(dir.path(), &fake, false)?;
        assert!(
            error.contains("tkm config set ui.web.enabled true"),
            "{error}"
        );
        assert!(!error.contains("feature"), "{error}");
        assert!(!fake.spawned.get());
        Ok(())
    }
    #[test]
    fn build_without_web_feature_names_only_the_feature() -> Result<(), Box<dyn std::error::Error>>
    {
        let dir = tempfile::tempdir()?;
        std::fs::write(
            dir.path().join("tkmd-web.json"),
            r#"{"unavailable":"feature"}"#,
        )?;
        let fake = Fake {
            live: true,
            ..Fake::default()
        };
        let error = dashboard_error(dir.path(), &fake, true)?;
        assert!(error.contains("`web` feature"), "{error}");
        assert!(!error.contains("tkm config set"), "{error}");
        Ok(())
    }
    #[test]
    fn enabled_without_discovery_asks_for_restart() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        let fake = Fake::default();
        let error = dashboard_error(dir.path(), &fake, true)?;
        assert!(error.contains("restart tkmd"), "{error}");
        assert!(!error.contains("tkm config set"), "{error}");
        assert!(!error.contains("feature"), "{error}");
        assert!(fake.spawned.get());
        Ok(())
    }
    #[test]
    fn no_open_skips_browser() -> Result<(), Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        std::fs::write(
            dir.path().join("tkmd-web.json"),
            r#"{"url":"http://127.0.0.1:1234/","token":"abc"}"#,
        )?;
        let fake = Fake {
            live: true,
            ..Fake::default()
        };
        run_dashboard_with(
            &paths(dir.path()),
            &DashboardArgs {
                no_open: true,
                web_enabled: true,
            },
            &fake,
            Duration::ZERO,
        )?;
        assert!(!fake.opened.get());
        Ok(())
    }
}
