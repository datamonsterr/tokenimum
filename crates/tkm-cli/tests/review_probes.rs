#![forbid(unsafe_code)]
#![cfg(target_os = "linux")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]

mod support_init;
use std::fs;
use std::io::{Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::time::{Duration, Instant};
use support_init::{Fixture, stdout};
use tkm_core::model::{Host, RepoKey, Timestamp};
use tkm_store::{NewEvent, NewSession, SessionKey};

#[test]
fn doctor_reads_normalized_hook_observations() {
    let f = Fixture::new();
    f.init();
    let store = tkm_store::Store::open(&f.paths.db).unwrap();
    let repo = RepoKey("fixture".into());
    store
        .upsert_repo(&tkm_store::Repo {
            repo_key: repo.clone(),
            display_name: "fixture".into(),
            first_seen: Timestamp(1),
            last_seen: Timestamp(1),
            delegated_runs: 0,
        })
        .unwrap();
    let sid = store
        .upsert_session(&NewSession {
            key: SessionKey {
                host: Host::Claude,
                agent_home: f.home.join(".claude").to_str().unwrap().into(),
                session_id: "observed".into(),
            },
            repo_key: repo,
            cwd: f.home.clone(),
            started: Timestamp(1),
            ended: None,
            fold_status: "open".into(),
            shadow: true,
            integration_level: "hooks".into(),
        })
        .unwrap();
    for event in tkm_hosts::probe::expected_events(Host::Claude) {
        store
            .insert_event(&NewEvent {
                kind: "hook.seen".into(),
                session_id: sid,
                inputs: serde_json::json!({"native": event.native, "event": event.kind.as_str()}),
                evidence_ids: vec![],
                explanation_template_id: "observation".into(),
                params: serde_json::json!({}),
            })
            .unwrap();
    }
    let out = f.run(&["doctor"]);
    let report = stdout(&out);
    let events = report
        .lines()
        .find(|line| line.contains("claude events:"))
        .unwrap();
    assert!(events.starts_with("ok "), "{events}");
    assert!(
        events.contains("SessionStart") && events.contains("not observed []"),
        "{events}"
    );
}

fn paid_probe(command: &[&str], configured_path: bool) {
    let f = Fixture::new();
    let store = tkm_store::Store::open(&f.paths.db).unwrap();
    fs::create_dir_all(f.paths.global_config.parent().unwrap()).unwrap();
    let key_path = f.temp.path().join("configured-key.env");
    let key_setting = if configured_path {
        fs::write(&key_path, "OPENROUTER_API_KEY=fixture-probe-credential\n").unwrap();
        fs::set_permissions(&key_path, fs::Permissions::from_mode(0o600)).unwrap();
        format!("api_key_path = {:?}\n", key_path.to_str().unwrap())
    } else {
        String::new()
    };
    fs::write(
        &f.paths.global_config,
        format!("[decide]\nenabled = true\nchain = [\"test/model-20261029\"]\n{key_setting}"),
    )
    .unwrap();
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let endpoint = format!("http://{}/decisions", listener.local_addr().unwrap());
    let question = if command[0] == "doctor" {
        "available"
    } else {
        "probe"
    };
    let server = std::thread::spawn(move || {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    read_request(&mut stream);
                    let body = serde_json::json!({"model":"test/model-20261029", "answers":{question:{"type":"noul","noul":1.0}}, "usage":{"input_tokens":1,"output_tokens":1,"cost":0.000_001}}).to_string();
                    write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
                    return true;
                }
                Err(e)
                    if e.kind() == std::io::ErrorKind::WouldBlock && Instant::now() < deadline =>
                {
                    std::thread::sleep(Duration::from_millis(10))
                }
                Err(_) => return false,
            }
        }
    });
    let mut cmd = f.command(command);
    cmd.env("TKM_DECIDE_ENDPOINT", endpoint);
    if !configured_path {
        cmd.env("OPENROUTER_API_KEY", "fixture-probe-credential");
    }
    let out = cmd.output().unwrap();
    assert!(
        server.join().unwrap(),
        "probe never reached local server: {out:?}"
    );
    assert!(
        stdout(&out).contains("test/model-20261029: available"),
        "{out:?}"
    );
    if command[0] == "models" {
        assert!(out.status.success(), "{out:?}");
    }
    let rows = store.list_recent_decisions(-1).unwrap();
    assert_eq!(rows.len(), 1);
    assert!(rows[0].created_at.is_some_and(|t| t.0 > 0));
    assert_eq!(rows[0].cost_usd, 0.000_001);
    let status = f.run(&["status", "--json"]);
    let json: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(
        json["repositories"][0]["spend"]["decisions_today_usd"],
        0.000_001
    );
}

#[test]
fn models_probe_uses_configured_key_path_and_records_daily_spend() {
    paid_probe(&["models", "probe"], true);
}

#[test]
fn models_probe_with_environment_key_records_daily_spend() {
    paid_probe(&["models", "probe"], false);
}

#[test]
fn doctor_paid_probe_records_timestamp_and_daily_spend() {
    paid_probe(&["doctor", "--probe"], false);
}

fn read_request(stream: &mut std::net::TcpStream) {
    let mut request = Vec::new();
    loop {
        let mut buffer = [0; 4096];
        let count = stream.read(&mut buffer).unwrap();
        assert!(count > 0, "incomplete probe request");
        request.extend_from_slice(&buffer[..count]);
        if let Some(end) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&request[..end]);
            let length = headers
                .lines()
                .find_map(|line| {
                    let (name, value) = line.split_once(':')?;
                    name.eq_ignore_ascii_case("content-length")
                        .then(|| value.trim().parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            if request.len() >= end + 4 + length {
                break;
            }
        }
    }
}
