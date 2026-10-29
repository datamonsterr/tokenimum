#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::path::PathBuf;
use tkm_core::model::{Call, EditKind, EditVia, FileEdit};
use tkm_decide::{
    digest::{DigestInput, MAX_DIGEST_BYTES, build_digest},
    redact::{RedactContext, Redactor},
};
fn redactor() -> Redactor {
    Redactor::new(RedactContext {
        repo_root: PathBuf::from("/repo"),
        home: PathBuf::from("/home/test"),
        env: vec![],
    })
}
#[test]
fn digest_last_calls_and_dotenv_output() {
    let mut calls: Vec<_> = (0..15)
        .map(|i| Call {
            tool: format!("tool{i}"),
            args_digest: "same".into(),
            ..Call::default()
        })
        .collect();
    calls[14].args_digest = "cat .env".into();
    calls[14].output_digest = "fake dotenv content".into();
    calls[14].is_error = true;
    let result = build_digest(
        DigestInput {
            prompt: &"x".repeat(600),
            calls: &calls,
            tests: None,
        },
        &redactor(),
    );
    assert!(!result.contains("tool2") && result.contains("tool3") && result.contains("tool14"));
    assert!(!result.contains("fake dotenv content") && !result.contains(&"x".repeat(501)));
    assert!(result.contains("errors: 1"));
}
#[test]
fn digest_never_exceeds_16kb() {
    for size in [1, 500, 20_000] {
        let text = "🦀".repeat(size);
        let calls = vec![
            Call {
                tool: text.clone(),
                args_digest: text.clone(),
                output_digest: text.clone(),
                is_error: true,
                ..Call::default()
            };
            30
        ];
        let digest = build_digest(
            DigestInput {
                prompt: &text,
                calls: &calls,
                tests: None,
            },
            &redactor(),
        );
        assert!(digest.len() <= MAX_DIGEST_BYTES);
    }
    digest_limits_many_calls_and_large_edit_tail();
}

fn digest_limits_many_calls_and_large_edit_tail() {
    let calls = vec![
        Call {
            args_digest: "a".repeat(10_240),
            ..Call::default()
        };
        5_000
    ];
    let digest = build_digest(
        DigestInput {
            prompt: "stress",
            calls: &calls,
            tests: None,
        },
        &redactor(),
    );
    assert!(digest.len() <= MAX_DIGEST_BYTES);
    let edits = (0..3_000)
        .map(|i| FileEdit {
            path: PathBuf::from(format!("src/very-long-file-name-{i:04}.rs")),
            kind: EditKind::Modify,
            via: EditVia::Tool,
        })
        .collect();
    let calls = [Call {
        edits,
        ..Call::default()
    }];
    let digest = build_digest(
        DigestInput {
            prompt: "stress",
            calls: &calls,
            tests: None,
        },
        &redactor(),
    );
    assert!(digest.len() <= MAX_DIGEST_BYTES);
    assert_eq!(digest.len(), MAX_DIGEST_BYTES);
}
