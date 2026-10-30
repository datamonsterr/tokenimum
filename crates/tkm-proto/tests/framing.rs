#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::io::{self, Cursor, Read, Write};
use std::time::Duration;

use proptest::{arbitrary::any, prop_assert_eq, proptest};
use serde_json::{Value, json};
use tkm_proto::{
    CONNECT_TIMEOUT, EventKind, FrameError, Host, MAX_REPLY_FRAME, MAX_REQUEST_FRAME,
    PROTOCOL_VERSION, Reply, ReplyAction, Request, decode_body, decode_len, encode_frame,
    read_frame, write_frame,
};

fn request(payload: Value) -> Request {
    Request {
        version: PROTOCOL_VERSION,
        host: Host::Claude,
        event: EventKind::ToolDone,
        host_event: Some("PostToolUse".into()),
        payload,
        env: BTreeMap::from([("TKM_SESSION_ID".into(), "synthetic-session".into())]),
        cwd: Some("/synthetic/repo".into()),
    }
}

#[test]
fn request_round_trip_preserves_raw_payload_and_env() {
    let expected = request(json!({"tool": "shell", "output": [null, true, "λ\n"]}));
    let mut bytes = Vec::new();
    write_frame(&mut bytes, &expected, MAX_REQUEST_FRAME).unwrap();
    let actual: Request = read_frame(Cursor::new(bytes), MAX_REQUEST_FRAME).unwrap();
    assert_eq!(actual, expected);
}

#[test]
fn reply_round_trip_preserves_every_action_and_verbatim_stdout() {
    for action in [
        ReplyAction::Nothing,
        ReplyAction::Inject {
            text: "fact".into(),
        },
        ReplyAction::Notice {
            text: "notice".into(),
        },
        ReplyAction::InjectAndNotice {
            inject: "fact".into(),
            notice: "notice".into(),
        },
    ] {
        let expected = Reply {
            version: PROTOCOL_VERSION,
            pending: None,
            action,
            stdout: "{\"host_specific\":true}\n".into(),
        };
        let bytes = encode_frame(&expected, MAX_REPLY_FRAME).unwrap();
        let actual: Reply = read_frame(Cursor::new(bytes), MAX_REPLY_FRAME).unwrap();
        assert_eq!(actual, expected);
    }
}

#[test]
fn pure_helpers_use_big_endian_body_length_excluding_header() {
    let frame = encode_frame(&json!("x"), 3).unwrap();
    assert_eq!(frame, b"\0\0\0\x03\"x\"");
    assert_eq!(decode_len([0, 0, 0, 3], 3).unwrap(), 3);
    assert_eq!(decode_body::<Value>(&frame[4..]).unwrap(), json!("x"));
}

#[test]
fn encode_rejects_body_larger_than_limit() {
    assert!(matches!(
        encode_frame(&json!("x"), 2),
        Err(FrameError::Oversize { len: 3, max: 2 })
    ));
}

#[test]
fn oversize_write_emits_no_partial_frame() {
    let mut bytes: Vec<u8> = Vec::new();
    assert!(matches!(
        write_frame(&mut bytes, &json!("x"), 2),
        Err(FrameError::Oversize { .. })
    ));
    assert_eq!(bytes, Vec::<u8>::new());
}

#[test]
fn decode_rejects_oversize_header_without_reading_body() {
    // Reading past the header would produce Truncated, proving the early limit check.
    let mut input = Cursor::new([0, 0, 0, 9]);
    assert!(matches!(
        read_frame::<_, Value>(&mut input, 8),
        Err(FrameError::Oversize { len: 9, max: 8 })
    ));
    assert_eq!(input.position(), 4);
}

#[test]
fn truncated_headers_of_zero_through_three_bytes_are_rejected() {
    for length in 0..4 {
        assert!(matches!(
            read_frame::<_, Value>(Cursor::new(&[0; 4][..length]), MAX_REQUEST_FRAME),
            Err(FrameError::Truncated)
        ));
    }
}

#[test]
fn truncated_body_is_rejected() {
    assert!(matches!(
        read_frame::<_, Value>(Cursor::new(b"\0\0\0\x04nul"), MAX_REQUEST_FRAME),
        Err(FrameError::Truncated)
    ));
}

#[test]
fn garbage_json_body_is_malformed() {
    assert!(matches!(
        read_frame::<_, Request>(Cursor::new(b"\0\0\0\x03bad"), MAX_REQUEST_FRAME),
        Err(FrameError::Malformed(_))
    ));
}

#[test]
fn wrong_shape_json_is_malformed() {
    assert!(matches!(
        decode_body::<Request>(b"{\"host\":42}"),
        Err(FrameError::Malformed(_))
    ));
}

#[test]
fn invalid_utf8_and_trailing_json_are_malformed() {
    for body in [&b"\xff"[..], &b"null true"[..]] {
        assert!(matches!(
            decode_body::<Value>(body),
            Err(FrameError::Malformed(_))
        ));
    }
}

#[test]
fn zero_length_header_and_empty_body_are_rejected() {
    assert!(matches!(
        decode_len([0; 4], MAX_REQUEST_FRAME),
        Err(FrameError::Empty)
    ));
    assert!(matches!(decode_body::<Value>(b""), Err(FrameError::Empty)));
    assert!(matches!(
        read_frame::<_, Value>(Cursor::new([0; 4]), MAX_REQUEST_FRAME),
        Err(FrameError::Empty)
    ));
}

#[test]
fn null_payload_supports_env_only_events() {
    let mut expected = request(Value::Null);
    expected.host = Host::Deepseek;
    expected.event = EventKind::SessionStart;
    expected.host_event = None;
    expected.cwd = None;
    let bytes = encode_frame(&expected, MAX_REQUEST_FRAME).unwrap();
    assert_eq!(
        read_frame::<_, Request>(Cursor::new(bytes), MAX_REQUEST_FRAME).unwrap(),
        expected
    );
}

#[test]
fn protocol_version_mismatch_remains_visible_to_caller() {
    let mut expected = request(Value::Null);
    expected.version = PROTOCOL_VERSION + 1;
    let bytes = encode_frame(&expected, MAX_REQUEST_FRAME).unwrap();
    let decoded: Request = read_frame(Cursor::new(bytes), MAX_REQUEST_FRAME).unwrap();
    assert_ne!(decoded.version, PROTOCOL_VERSION);
    let reply = Reply {
        version: PROTOCOL_VERSION + 1,
        pending: None,
        action: ReplyAction::Nothing,
        stdout: String::new(),
    };
    let frame = encode_frame(&reply, MAX_REPLY_FRAME).unwrap();
    let decoded: Reply = read_frame(Cursor::new(frame), MAX_REPLY_FRAME).unwrap();
    assert_ne!(decoded.version, PROTOCOL_VERSION);
}

#[test]
fn reading_one_frame_leaves_the_next_frame_untouched() {
    let mut bytes = encode_frame(&json!("first"), MAX_REQUEST_FRAME).unwrap();
    bytes.extend(encode_frame(&json!("second"), MAX_REQUEST_FRAME).unwrap());
    let mut input = Cursor::new(bytes);
    assert_eq!(
        read_frame::<_, Value>(&mut input, MAX_REQUEST_FRAME).unwrap(),
        json!("first")
    );
    assert_eq!(
        read_frame::<_, Value>(&mut input, MAX_REQUEST_FRAME).unwrap(),
        json!("second")
    );
}

struct BrokenIo;

impl Read for BrokenIo {
    fn read(&mut self, _buffer: &mut [u8]) -> io::Result<usize> {
        Err(io::Error::new(io::ErrorKind::TimedOut, "synthetic timeout"))
    }
}

impl Write for BrokenIo {
    fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
        Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "synthetic closed pipe",
        ))
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn io_errors_preserve_source_kind() {
    assert!(matches!(
        read_frame::<_, Value>(BrokenIo, MAX_REQUEST_FRAME),
        Err(FrameError::Io(error)) if error.kind() == io::ErrorKind::TimedOut
    ));
    assert!(matches!(
        write_frame(BrokenIo, &Value::Null, MAX_REPLY_FRAME),
        Err(FrameError::Io(error)) if error.kind() == io::ErrorKind::BrokenPipe
    ));
}

#[test]
fn host_names_and_noop_replies_match_spec_6_3() {
    for (host, name, noop) in [
        (Host::Claude, "claude", ""),
        (Host::Codex, "codex", ""),
        (Host::Agy, "agy", "{}"),
        (Host::Cursor, "cursor", "{}"),
        (Host::Kimi, "kimi", ""),
        (Host::Deepseek, "deepseek", ""),
        (Host::Opencode, "opencode", ""),
        (Host::Pi, "pi", ""),
    ] {
        assert_eq!(host.as_str(), name);
        assert_eq!(name.parse::<Host>().unwrap(), host);
        assert_eq!(
            serde_json::to_vec(&host).unwrap(),
            serde_json::to_vec(name).unwrap()
        );
        assert_eq!(host.noop_reply(), noop);
    }
    assert!("unknown".parse::<Host>().is_err());
    assert!("Claude".parse::<Host>().is_err());
}

#[test]
fn event_names_and_read_timeouts_match_spec_6_2() {
    assert_eq!(CONNECT_TIMEOUT, Duration::from_millis(2));
    for (event, name, milliseconds) in [
        (EventKind::SessionStart, "session_start", 300),
        (EventKind::Prompt, "prompt", 1_500),
        (EventKind::ToolBefore, "tool_before", 5),
        (EventKind::ToolDone, "tool_done", 5),
        (EventKind::TurnEnd, "turn_end", 5),
        (EventKind::SessionEnd, "session_end", 5),
    ] {
        assert_eq!(event.as_str(), name);
        assert_eq!(name.parse::<EventKind>().unwrap(), event);
        assert_eq!(event.read_timeout(), Duration::from_millis(milliseconds));
        assert_eq!(
            serde_json::to_vec(&event).unwrap(),
            serde_json::to_vec(name).unwrap()
        );
    }
    assert!("unknown".parse::<EventKind>().is_err());
}

#[test]
fn reply_actions_have_stable_snake_case_tags() {
    let action = ReplyAction::InjectAndNotice {
        inject: "fact".into(),
        notice: "notice".into(),
    };
    assert_eq!(
        decode_body::<Value>(&serde_json::to_vec(&action).unwrap()).unwrap(),
        json!({
            "kind": "inject_and_notice", "inject": "fact", "notice": "notice"
        })
    );
}

proptest! {
    #[test]
    fn arbitrary_payload_strings_round_trip(payload in any::<String>()) {
        let expected = request(Value::String(payload));
        let bytes = encode_frame(&expected, MAX_REQUEST_FRAME).unwrap();
        let actual: Request = read_frame(Cursor::new(bytes), MAX_REQUEST_FRAME).unwrap();
        prop_assert_eq!(actual, expected);
    }
}

#[test]
fn reply_preserves_pending_signal_through_framing() {
    let reply = Reply {
        version: PROTOCOL_VERSION,
        pending: Some(true),
        action: ReplyAction::Nothing,
        stdout: String::new(),
    };
    let frame = encode_frame(&reply, MAX_REPLY_FRAME).unwrap();
    let decoded: Reply = read_frame(Cursor::new(frame), MAX_REPLY_FRAME).unwrap();
    assert_eq!(decoded.pending, Some(true));
}
