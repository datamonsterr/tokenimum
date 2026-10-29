#![cfg(feature = "web")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]
use std::{
    io::{Read, Write},
    os::unix::fs::PermissionsExt,
    sync::Arc,
};
use tkm_config::paths::Paths;
use tkmd::{
    daemon::{Daemon, DaemonOptions},
    web::WebServer,
};

#[tokio::test(flavor = "multi_thread")]
async fn web_file_is_private_and_state_is_authorized() {
    let dir = tempfile::tempdir().unwrap();
    let paths = Paths::from_dirs(
        dir.path(),
        Some(dir.path()),
        Some(dir.path()),
        Some(dir.path()),
    );
    paths.ensure_runtime_dir().unwrap();
    let _store = tkm_store::Store::open(&paths.db).unwrap();
    let daemon = Daemon::new(DaemonOptions::new(paths.clone()));
    let web = WebServer::start(Arc::clone(&daemon), &paths, "127.0.0.1:0")
        .await
        .unwrap();
    let file = paths.socket.with_file_name("tkmd-web.json");
    assert_eq!(
        std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let info: serde_json::Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
    let url = info["url"]
        .as_str()
        .unwrap()
        .trim_start_matches("http://")
        .trim_end_matches('/');
    let mut stream = std::net::TcpStream::connect(url).unwrap();
    write!(stream, "GET /api/state HTTP/1.1\r\nHost: {url}\r\nAuthorization: Bearer {}\r\nConnection: close\r\n\r\n", info["token"].as_str().unwrap()).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    assert!(response.starts_with("HTTP/1.1 200"));
    web.shutdown().await;
    assert!(!file.exists());
}
#[test]
fn enabled_daemon_publishes_dashboard_and_bind_failure_fails_open() {
    use std::{
        process::{Command, Stdio},
        time::{Duration, Instant},
    };
    for occupied in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let runtime = dir.path().join("run");
        std::fs::create_dir_all(&runtime).unwrap();
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let bind = if occupied {
            listener.local_addr().unwrap().to_string()
        } else {
            "127.0.0.1:0".into()
        };
        let mut child = Command::new(env!("CARGO_BIN_EXE_tkmd"))
            .env("HOME", dir.path())
            .env("XDG_DATA_HOME", dir.path().join("data"))
            .env("XDG_CONFIG_HOME", dir.path().join("config"))
            .env("XDG_RUNTIME_DIR", &runtime)
            .env("TKM_DECIDE_ENABLED", "false")
            .env("TKM_UI_WEB_ENABLED", "true")
            .env("TKM_UI_WEB_BIND", bind)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let socket = runtime.join("tokenimum/tkmd.sock");
        let file = runtime.join("tokenimum/tkmd-web.json");
        let errors = dir.path().join("data/tokenimum/errors.log");
        let deadline = Instant::now() + Duration::from_secs(10);
        while Instant::now() < deadline
            && !(socket.exists()
                && if occupied {
                    errors.exists()
                } else {
                    file.exists()
                })
        {
            assert!(child.try_wait().unwrap().is_none(), "daemon exited early");
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(socket.exists());
        assert!(child.try_wait().unwrap().is_none());
        if occupied {
            assert!(
                std::fs::read_to_string(errors)
                    .unwrap()
                    .contains("web startup:")
            );
            assert!(!file.exists());
        } else {
            assert_eq!(
                std::fs::metadata(&file).unwrap().permissions().mode() & 0o777,
                0o600
            );
            let info: serde_json::Value =
                serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
            let host = info["url"]
                .as_str()
                .unwrap()
                .trim_start_matches("http://")
                .trim_end_matches('/');
            let mut stream = std::net::TcpStream::connect(host).unwrap();
            write!(stream, "GET /api/state HTTP/1.1\r\nHost: {host}\r\nAuthorization: Bearer {}\r\nConnection: close\r\n\r\n", info["token"].as_str().unwrap()).unwrap();
            let mut response = String::new();
            stream.read_to_string(&mut response).unwrap();
            assert!(response.starts_with("HTTP/1.1 200"));
        }
        child.kill().unwrap();
        child.wait().unwrap();
    }
}
