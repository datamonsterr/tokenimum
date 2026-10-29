//! E4.2: socket server, framing, version and host handling.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::pedantic)]

mod common;

use std::os::unix::fs::PermissionsExt;
use std::sync::Arc;

use common::*;
use tkm_proto::{
    EventKind, Host, MAX_REPLY_FRAME, MAX_REQUEST_FRAME, PROTOCOL_VERSION, Reply, ReplyAction,
    Request, decode_len, encode_frame,
};
use tkmd::decider::NullDecider;
use tkmd::server::{Bind, Server};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

async fn serve(env: &Env) -> (Arc<tkmd::daemon::Daemon>, tokio::task::JoinHandle<()>) {
    let daemon = daemon_with(env, Default::default(), Arc::new(NullDecider));
    let Bind::Bound(server) = Server::bind(daemon.clone()).await.unwrap() else {
        panic!("socket should be free");
    };
    let handle = tokio::spawn(async move {
        server.run(std::future::pending::<()>()).await.unwrap();
    });
    (daemon, handle)
}

async fn roundtrip(env: &Env, request: &Request) -> Reply {
    let mut stream = UnixStream::connect(&env.paths.socket).await.unwrap();
    stream
        .write_all(&encode_frame(request, MAX_REQUEST_FRAME).unwrap())
        .await
        .unwrap();
    let mut header = [0_u8; 4];
    stream.read_exact(&mut header).await.unwrap();
    let len = decode_len(header, MAX_REPLY_FRAME).unwrap();
    let mut body = vec![0_u8; len];
    stream.read_exact(&mut body).await.unwrap();
    tkm_proto::decode_body(&body).unwrap()
}

#[tokio::test]
async fn serves_a_framed_request_and_returns_a_reply_frame() {
    let env = Env::new();
    let (_daemon, _server) = serve(&env).await;
    let reply = roundtrip(&env, &session_start()).await;
    assert_eq!(reply.version, PROTOCOL_VERSION);
    assert_eq!(reply.action, ReplyAction::Nothing);
    assert_eq!(reply.stdout, "");
}

#[tokio::test]
async fn socket_is_mode_600_in_a_private_directory() {
    let env = Env::new();
    let (_daemon, _server) = serve(&env).await;
    let mode = std::fs::metadata(&env.paths.socket)
        .unwrap()
        .permissions()
        .mode();
    assert_eq!(mode & 0o777, 0o600);
    let dir = std::fs::metadata(env.paths.socket.parent().unwrap()).unwrap();
    assert_eq!(dir.permissions().mode() & 0o777, 0o700);
}

#[tokio::test]
async fn version_mismatch_gets_the_noop_reply() {
    let env = Env::new();
    let (_daemon, _server) = serve(&env).await;
    let mut request = prompt();
    request.version = PROTOCOL_VERSION + 1;
    let reply = roundtrip(&env, &request).await;
    assert_eq!(reply.action, ReplyAction::Nothing);
    assert_eq!(reply.stdout, "");
}

#[tokio::test]
async fn unknown_host_adapter_gets_the_hosts_noop_reply() {
    let env = Env::new();
    let (_daemon, _server) = serve(&env).await;
    let mut request = tool_done();
    request.host = Host::Cursor;
    let reply = roundtrip(&env, &request).await;
    assert_eq!(reply.stdout, "{}");
    request.host = Host::Pi;
    assert_eq!(roundtrip(&env, &request).await.stdout, "");
}

#[tokio::test]
async fn oversize_request_header_closes_the_connection_without_reading_a_body() {
    let env = Env::new();
    let (_daemon, _server) = serve(&env).await;
    let mut stream = UnixStream::connect(&env.paths.socket).await.unwrap();
    let too_big = u32::try_from(MAX_REQUEST_FRAME + 1).unwrap();
    stream.write_all(&too_big.to_be_bytes()).await.unwrap();
    let mut rest = Vec::new();
    let read = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        stream.read_to_end(&mut rest),
    )
    .await
    .expect("server must close the connection");
    assert!(read.is_ok());
    assert!(rest.is_empty(), "no reply for an oversize header");
    // The daemon keeps serving afterwards.
    assert_eq!(roundtrip(&env, &session_start()).await.stdout, "");
    assert_eq!(
        env.error_lines().len(),
        1,
        "one errors.log line for the bad frame"
    );
}

#[tokio::test]
async fn malformed_json_body_closes_the_connection_and_logs_one_line() {
    let env = Env::new();
    let (_daemon, _server) = serve(&env).await;
    let mut stream = UnixStream::connect(&env.paths.socket).await.unwrap();
    let body = b"{not json";
    stream
        .write_all(&u32::try_from(body.len()).unwrap().to_be_bytes())
        .await
        .unwrap();
    stream.write_all(body).await.unwrap();
    let mut rest = Vec::new();
    stream.read_to_end(&mut rest).await.unwrap();
    assert!(rest.is_empty());
    assert_eq!(roundtrip(&env, &session_start()).await.stdout, "");
    assert_eq!(env.error_lines().len(), 1);
}

#[tokio::test]
async fn stale_socket_file_is_replaced_after_a_failed_connect_probe() {
    let env = Env::new();
    env.paths.ensure_runtime_dir().unwrap();
    // A bound-then-dropped listener leaves a socket file nobody listens on.
    drop(std::os::unix::net::UnixListener::bind(&env.paths.socket).unwrap());
    assert!(env.paths.socket.exists());
    let (_daemon, _server) = serve(&env).await;
    assert_eq!(roundtrip(&env, &session_start()).await.stdout, "");
}

#[tokio::test]
async fn a_live_daemons_socket_is_never_stolen() {
    let env = Env::new();
    let (_daemon, _server) = serve(&env).await;
    let second = daemon_with(&env, Default::default(), Arc::new(NullDecider));
    match Server::bind(second).await.unwrap() {
        Bind::AlreadyRunning => {}
        Bind::Bound(_) => panic!("second instance must not bind"),
    }
    // First daemon still answers.
    assert_eq!(roundtrip(&env, &session_start()).await.stdout, "");
}

#[test]
fn reply_larger_than_the_frame_limit_becomes_the_noop_reply() {
    let huge = Reply {
        version: PROTOCOL_VERSION,
        action: ReplyAction::Inject {
            text: "x".repeat(MAX_REPLY_FRAME),
        },
        stdout: "y".repeat(MAX_REPLY_FRAME),
    };
    let capped = tkmd::daemon::cap_reply(huge, Host::Cursor);
    assert_eq!(capped.stdout, "{}");
    assert_eq!(capped.action, ReplyAction::Nothing);
    let small = Reply {
        version: PROTOCOL_VERSION,
        action: ReplyAction::Nothing,
        stdout: "ok".into(),
    };
    assert_eq!(tkmd::daemon::cap_reply(small, Host::Claude).stdout, "ok");
    let _ = EventKind::ToolDone;
}
