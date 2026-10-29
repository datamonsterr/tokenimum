#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod support_ops;
#[test]
fn models_probe_offline_skips() {
    let s = support_ops::Sandbox::new();
    s.seed_db();
    let out = s.run(&["models", "probe"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("skipped (offline)"));
}
#[test]
fn models_lists_chain() {
    let s = support_ops::Sandbox::new();
    s.seed_db();
    let out = s.run(&["models"]);
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("mercury"));
}
#[test]
fn models_probe_uses_local_server() {
    use std::io::{Read, Write};
    use std::time::{Duration, Instant};
    let s = support_ops::Sandbox::new();
    s.seed_db();
    assert!(
        s.run(&[
            "config",
            "set",
            "decide.chain",
            "[\"test/model-20261029\"]",
            "--global"
        ])
        .status
        .success()
    );
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}/decisions", listener.local_addr().unwrap());
    let server = std::thread::spawn(move || {
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut buffer = [0; 4096];
                    let _ = stream.read(&mut buffer);
                    let body = r#"{"model":"test/model-20261029","answers":{"probe":{"type":"noul","noul":1.0}},"usage":{"input_tokens":1,"output_tokens":1,"cost":0.0}}"#;
                    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
                    return true;
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < until => {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(_) => return false,
            }
        }
    });
    let out = s.run_with_env(
        &["models", "probe"],
        &[
            ("OPENROUTER_API_KEY", "test-key"),
            ("TKM_DECIDE_ENDPOINT", &endpoint),
        ],
    );
    assert!(server.join().unwrap());
    assert!(out.status.success(), "{out:?}");
    assert!(String::from_utf8_lossy(&out.stdout).contains("test/model-20261029: available\n"));
}
