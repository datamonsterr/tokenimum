#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::{
    fs,
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use tkm_core::model::{Host, RepoKey, Timestamp, Usage};
use tkm_hosts::{
    HostAdapter, SessionSource,
    kimi::{KimiAdapter, md5_hex, session_dir},
};

const FIXTURES_SESSION: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../fixtures/kimi/sessions/90722f2638004be06d790eaac9ac1f8a/9a1b2c3d-4e5f-6a7b-8c9d-0e1f2a3b4c5d"
);

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "tkm-kimi-reader-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn md5_hex_rfc1321_test_vectors() {
    assert_eq!(md5_hex(b""), "d41d8cd98f00b204e9800998ecf8427e");
    assert_eq!(md5_hex(b"a"), "0cc175b9c0f1b6a831c399e269772661");
    assert_eq!(md5_hex(b"abc"), "900150983cd24fb0d6963f7d28e17f72");
    assert_eq!(
        md5_hex(b"message digest"),
        "f96b697d7cb7938d525a2f31aaf161d0"
    );
    assert_eq!(
        md5_hex(b"abcdefghijklmnopqrstuvwxyz"),
        "c3fcd3d76192e4007dfb496cca67e13b"
    );
    assert_eq!(
        md5_hex(b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789"),
        "d174ab98d277d9f5a5611c2c9f419d9f"
    );
    assert_eq!(
        md5_hex(
            b"12345678901234567890123456789012345678901234567890123456789012345678901234567890"
        ),
        "57edf4a22be3c955ac49da2e2107b67a"
    );
}

#[test]
fn session_dir_computation_and_assert_fixture_path() {
    let work_dir = "/home/user/project";
    let hash = md5_hex(work_dir.as_bytes());
    assert_eq!(hash, "90722f2638004be06d790eaac9ac1f8a");

    let share = Path::new("/var/kimi/share");
    let session_id = "9a1b2c3d-4e5f-6a7b-8c9d-0e1f2a3b4c5d";
    let dir = session_dir(share, work_dir, session_id);
    assert_eq!(
        dir,
        PathBuf::from(
            "/var/kimi/share/sessions/90722f2638004be06d790eaac9ac1f8a/9a1b2c3d-4e5f-6a7b-8c9d-0e1f2a3b4c5d"
        )
    );
}

#[test]
fn read_synthetic_session_fixture() {
    let adapter = KimiAdapter::new();
    let source = SessionSource {
        agent_home: PathBuf::from("/home/user/.kimi"),
        path: PathBuf::from(FIXTURES_SESSION),
        session_id: "9a1b2c3d-4e5f-6a7b-8c9d-0e1f2a3b4c5d".into(),
        repo_key: RepoKey("test-repo".into()),
        cwd: PathBuf::from("/home/user/project"),
    };

    let session = adapter.read_session(&source).unwrap();
    assert_eq!(session.host, Host::Kimi);
    assert_eq!(session.session_id, "9a1b2c3d-4e5f-6a7b-8c9d-0e1f2a3b4c5d");
    assert_eq!(session.started, Timestamp(1_730_198_400_100));
    assert_eq!(session.ended, Some(Timestamp(1_730_198_403_000)));
    assert!(!session.partial);
    assert_eq!(session.skipped_records, 0);

    // 1 user turn
    assert_eq!(session.turns.len(), 1);
    let turn = &session.turns[0];
    assert_eq!(turn.prompt, "Please check the project files");

    // 2 tool calls
    assert_eq!(turn.calls.len(), 2);
    let call1 = &turn.calls[0];
    assert_eq!(call1.tool, "bash");
    assert_eq!(call1.args_digest, "ls -la");
    assert!(!call1.is_error);
    assert!(call1.output_digest.contains("README.md"));

    let call2 = &turn.calls[1];
    assert_eq!(call2.tool, "bash");
    assert_eq!(call2.args_digest, "cargo test");
    assert!(call2.is_error);
    assert!(call2.output_digest.contains("compilation failed"));

    // Usage from wire.jsonl
    let expected_usage = Usage {
        input: 250,
        output: 60,
        cache_read: 150,
        cache_write: 10,
    };
    assert_eq!(turn.usage, expected_usage);
    assert_eq!(session.usage, expected_usage);
}

#[test]
fn read_session_without_wire_jsonl() {
    let scratch = Scratch::new();
    let context_path = scratch.0.join("context.jsonl");
    fs::write(
        &context_path,
        r#"{"_system_prompt":"sys"}
{"role":"user","content":"hello"}
{"role":"assistant","content":"hi","tool_calls":[{"id":"c1","type":"function","function":{"name":"echo","arguments":"{\"text\":\"hi\"}"}}]}
{"role":"tool","tool_call_id":"c1","content":"hi"}
"#,
    )
    .unwrap();

    let adapter = KimiAdapter::new();
    let source = SessionSource {
        agent_home: scratch.0.clone(),
        path: scratch.0.clone(),
        session_id: "s1".into(),
        repo_key: RepoKey("repo".into()),
        cwd: PathBuf::from("/cwd"),
    };

    let session = adapter.read_session(&source).unwrap();
    assert_eq!(session.session_id, "s1");
    assert_eq!(session.started, Timestamp::default());
    assert_eq!(session.ended, None);
    assert_eq!(session.usage, Usage::default());
    assert_eq!(session.turns.len(), 1);
    assert_eq!(session.turns[0].prompt, "hello");
    assert_eq!(session.turns[0].calls.len(), 1);
    assert_eq!(session.turns[0].calls[0].tool, "echo");
    assert!(!session.partial);
}

#[test]
fn partial_session_on_garbage_lines() {
    let scratch = Scratch::new();
    let context_path = scratch.0.join("context.jsonl");
    // 2 valid lines + 1 garbage line = 1/3 (33.3% > 20%) skipped -> partial
    fs::write(
        &context_path,
        r#"{"role":"user","content":"valid prompt"}
not a json line
{"role":"assistant","content":"reply"}
"#,
    )
    .unwrap();

    let adapter = KimiAdapter::new();
    let source = SessionSource {
        agent_home: scratch.0.clone(),
        path: scratch.0.clone(),
        session_id: "s_garbage".into(),
        repo_key: RepoKey("repo".into()),
        cwd: PathBuf::from("/cwd"),
    };

    let session = adapter.read_session(&source).unwrap();
    assert!(session.partial, "session should be marked partial");
    assert_eq!(session.skipped_records, 1);
    assert_eq!(session.total_records, 3);
}

#[test]
fn read_session_resolves_path_when_empty() {
    let scratch = Scratch::new();
    let work_dir = "/home/user/project";
    let hash = md5_hex(work_dir.as_bytes());
    let session_dir = scratch.0.join("sessions").join(&hash).join("sess_computed");
    fs::create_dir_all(&session_dir).unwrap();
    fs::write(
        session_dir.join("context.jsonl"),
        r#"{"role":"user","content":"computed path test"}
"#,
    )
    .unwrap();

    let adapter = KimiAdapter::new();
    let source = SessionSource {
        agent_home: scratch.0.clone(),
        path: PathBuf::new(), // Empty path!
        session_id: "sess_computed".into(),
        repo_key: RepoKey("repo".into()),
        cwd: PathBuf::from(work_dir),
    };

    let session = adapter.read_session(&source).unwrap();
    assert_eq!(session.session_id, "sess_computed");
    assert_eq!(session.turns.len(), 1);
    assert_eq!(session.turns[0].prompt, "computed path test");
}

#[test]
fn invalid_utf8_line_is_skipped_not_fatal() {
    let scratch = Scratch::new();
    let mut bytes: Vec<u8> = Vec::new();
    bytes.extend_from_slice(b"{\"role\":\"user\",\"content\":\"first\"}\n");
    bytes.extend_from_slice(b"{\"role\":\"user\",\"content\":\"bad \xff\xfe\"}\n");
    bytes.extend_from_slice(b"{\"role\":\"user\",\"content\":\"third\"}\n");
    fs::write(scratch.0.join("context.jsonl"), bytes).unwrap();

    let adapter = KimiAdapter::new();
    let source = SessionSource {
        agent_home: scratch.0.clone(),
        path: scratch.0.clone(),
        session_id: "s_utf8".into(),
        repo_key: RepoKey("repo".into()),
        cwd: PathBuf::from("/cwd"),
    };

    let session = adapter.read_session(&source).unwrap();
    assert_eq!(session.turns.len(), 2);
    assert_eq!(session.turns[0].prompt, "first");
    assert_eq!(session.turns[1].prompt, "third");
    assert_eq!(session.skipped_records, 1);
    assert_eq!(session.total_records, 3);
}

#[test]
fn invalid_utf8_wire_line_is_skipped_not_fatal() {
    let scratch = Scratch::new();
    fs::write(
        scratch.0.join("context.jsonl"),
        "{\"role\":\"user\",\"content\":\"hello\"}\n",
    )
    .unwrap();
    let mut bytes: Vec<u8> = Vec::new();
    bytes.extend_from_slice(b"{\"type\":\"metadata\",\"protocol_version\":\"1.0\"}\n");
    bytes.extend_from_slice(b"{\"timestamp\":1730198400.1,\"message\":{\"type\":\"TurnBegin\"}}\n");
    bytes.extend_from_slice(b"\xff\xfe broken\n");
    bytes.extend_from_slice(b"{\"timestamp\":1730198403.0,\"message\":{\"type\":\"TurnEnd\"}}\n");
    fs::write(scratch.0.join("wire.jsonl"), bytes).unwrap();

    let adapter = KimiAdapter::new();
    let source = SessionSource {
        agent_home: scratch.0.clone(),
        path: scratch.0.clone(),
        session_id: "s_wire_utf8".into(),
        repo_key: RepoKey("repo".into()),
        cwd: PathBuf::from("/cwd"),
    };

    let session = adapter.read_session(&source).unwrap();
    assert_eq!(session.skipped_records, 1);
    assert_eq!(session.ended, Some(Timestamp(1_730_198_403_000)));
}

#[test]
fn wire_first_line_event_is_not_skipped() {
    let scratch = Scratch::new();
    fs::write(
        scratch.0.join("context.jsonl"),
        "{\"role\":\"user\",\"content\":\"hello\"}\n",
    )
    .unwrap();
    fs::write(
        scratch.0.join("wire.jsonl"),
        "{\"timestamp\":1730198400.1,\"message\":{\"type\":\"TurnBegin\"}}\n{\"timestamp\":1730198403.0,\"message\":{\"type\":\"TurnEnd\"}}\n",
    )
    .unwrap();

    let adapter = KimiAdapter::new();
    let source = SessionSource {
        agent_home: scratch.0.clone(),
        path: scratch.0.clone(),
        session_id: "s_wire_first".into(),
        repo_key: RepoKey("repo".into()),
        cwd: PathBuf::from("/cwd"),
    };

    let session = adapter.read_session(&source).unwrap();
    assert_eq!(session.started, Timestamp(1_730_198_400_100));
    assert_eq!(session.ended, Some(Timestamp(1_730_198_403_000)));
    assert_eq!(session.skipped_records, 0);
}
