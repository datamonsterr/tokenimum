#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use serde_json::json;
use tkm_hosts::digest::{args_digest, output_digest};

#[test]
fn shell_args_and_sorted_nested_json_are_deterministic() {
    assert_eq!(
        args_digest(
            "Bash",
            &json!({"command":"printf hi", "description":"ignored"})
        ),
        "printf hi"
    );
    assert_eq!(
        args_digest("Read", &json!({"z": {"b":2,"a":1},"a":[3,2]})),
        r#"{"a":[3,2],"z":{"a":1,"b":2}}"#
    );
}

#[test]
fn output_prefers_first_error_marker_else_last_nonempty_line() {
    for marker in ["error", "Error", "Exit code", "fatal", "panicked"] {
        let text = format!("ordinary\n{marker}: failed\nlast\n");
        assert_eq!(output_digest(&json!(text)), format!("{marker}: failed"));
    }
    assert_eq!(
        output_digest(&json!({"stdout":"one\ntwo\n", "stderr":""})),
        "two"
    );
    assert_eq!(
        output_digest(&json!([{"type":"text","text":"first\nlast"}])),
        "last"
    );
    assert_eq!(output_digest(&json!(null)), "");
}

#[test]
fn digests_cap_at_512_unicode_characters() {
    let long = "🦀".repeat(600);
    assert_eq!(
        args_digest("Bash", &json!({"command":long})),
        "🦀".repeat(512)
    );
    assert_eq!(output_digest(&json!(long)), "🦀".repeat(512));
}
