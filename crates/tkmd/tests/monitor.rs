//! E4.6 monitor API.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]

mod common;

use std::sync::Arc;
use std::time::{Duration, Instant};

use common::*;
use tkm_proto::monitor::{MONITOR_MAX_FRAME, MONITOR_SOCKET_NAME, MonitorMessage};
use tkm_proto::{Host, PROTOCOL_VERSION, decode_body, decode_len};
use tkmd::decider::NullDecider;
use tkmd::monitor::Monitor;
use tkmd::settings::{LoopMode, Settings};
use tokio::io::AsyncReadExt;
use tokio::net::UnixStream;

async fn next_message(stream: &mut UnixStream) -> Option<MonitorMessage> {
    let mut header = [0_u8; 4];
    stream.read_exact(&mut header).await.ok()?;
    let len = decode_len(header, MONITOR_MAX_FRAME).ok()?;
    let mut body = vec![0_u8; len];
    stream.read_exact(&mut body).await.ok()?;
    decode_body(&body).ok()
}

async fn connect(env: &Env) -> UnixStream {
    let path = env.paths.socket.with_file_name(MONITOR_SOCKET_NAME);
    UnixStream::connect(path).await.unwrap()
}

#[tokio::test]
async fn subscriber_gets_hello_then_session_and_decision_messages() {
    let env = Env::new();
    env.seed_envelope(FIXTURE_REPO, Host::Claude, &[5; 5]);
    let settings = Settings {
        loop_mode: LoopMode::Shadow,
        ..Settings::default()
    };
    let daemon = daemon_with(&env, settings, ScriptedDecider::new(stuck_decision()));
    daemon.start_workers();
    let monitor = Monitor::bind(&daemon).await.unwrap();
    let mode = std::os::unix::fs::PermissionsExt::mode(
        &std::fs::metadata(monitor.path()).unwrap().permissions(),
    );
    assert_eq!(mode & 0o777, 0o600);
    tokio::spawn(monitor.run(daemon.clone()));

    let mut stream = connect(&env).await;
    assert_eq!(
        next_message(&mut stream).await,
        Some(MonitorMessage::Hello {
            version: PROTOCOL_VERSION
        })
    );
    assert!(eventually(5, || daemon.subscriber_count() == 1).await);

    daemon.handle(session_start()).await;
    daemon.handle(prompt()).await;
    for _ in 0..18 {
        daemon.handle_tool_done(&failing_tool_done());
    }
    daemon.settle().await;

    let mut saw_session = false;
    let mut saw_decision = false;
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline && !(saw_session && saw_decision) {
        let Ok(Some(message)) =
            tokio::time::timeout(Duration::from_secs(2), next_message(&mut stream)).await
        else {
            break;
        };
        match message {
            MonitorMessage::Session(update) => {
                assert_eq!(update.host, Host::Claude);
                saw_session = true;
            }
            MonitorMessage::Decision(notice) => {
                assert_eq!(notice.purpose, "loop_guard");
                assert!(notice.shadow);
                saw_decision = true;
            }
            _ => {}
        }
    }
    assert!(saw_session, "session message");
    assert!(saw_decision, "decision message");
}

#[tokio::test]
async fn slow_subscriber_is_dropped_and_handlers_never_block() {
    let env = Env::new();
    let mut options = tkmd::daemon::DaemonOptions::new(env.paths.clone());
    options.decider = Arc::new(NullDecider);
    options.settings = settings_fixed(Settings::default());
    options.monitor_capacity = 4;
    let daemon = tkmd::daemon::Daemon::new(options);
    let monitor = Monitor::bind(&daemon).await.unwrap();
    tokio::spawn(monitor.run(daemon.clone()));

    let _idle = connect(&env).await; // never reads
    assert!(eventually(5, || daemon.subscriber_count() == 1).await);
    daemon.handle(session_start()).await;
    daemon.handle(prompt()).await;
    let started = Instant::now();
    for _ in 0..20_000 {
        daemon.handle_tool_done(&tool_done());
    }
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "handlers must not wait for subscribers"
    );
    assert!(
        eventually(10, || daemon.subscriber_count() == 0).await,
        "slow subscriber dropped"
    );
}
