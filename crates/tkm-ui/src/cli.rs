//! Entry point for `tkm top`.
use tkm_config::paths::Paths;
/// Run the monitor.
///
/// # Errors
/// Returns an error when the snapshot or terminal cannot be used.
pub fn run_top(paths: &Paths) -> Result<(), crate::Error> {
    crate::tui::run(paths)
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
    fn discovery_file_returns_fragment_url_and_opens_browser() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("tkmd-web.json"),
            r#"{"url":"http://127.0.0.1:1234/","token":"abc"}"#,
        )
        .unwrap();
        let fake = Fake {
            live: true,
            ..Fake::default()
        };
        let url = run_dashboard_with(
            &paths(dir.path()),
            &DashboardArgs { no_open: false },
            &fake,
            Duration::ZERO,
        )
        .unwrap();
        assert_eq!(url, "http://127.0.0.1:1234/#token=abc");
        assert!(fake.opened.get());
        assert!(!fake.spawned.get());
    }
    #[test]
    fn missing_discovery_file_has_enable_hint() {
        let dir = tempfile::tempdir().unwrap();
        let fake = Fake::default();
        let error = run_dashboard_with(
            &paths(dir.path()),
            &DashboardArgs { no_open: true },
            &fake,
            Duration::ZERO,
        )
        .unwrap_err()
        .to_string();
        assert!(error.contains("tkm config set ui.web.enabled true"));
        assert!(error.contains("web"));
        assert!(fake.spawned.get());
    }
    #[test]
    fn no_open_skips_browser() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("tkmd-web.json"),
            r#"{"url":"http://127.0.0.1:1234/","token":"abc"}"#,
        )
        .unwrap();
        let fake = Fake {
            live: true,
            ..Fake::default()
        };
        run_dashboard_with(
            &paths(dir.path()),
            &DashboardArgs { no_open: true },
            &fake,
            Duration::ZERO,
        )
        .unwrap();
        assert!(!fake.opened.get());
    }
}
