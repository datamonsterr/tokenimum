#![forbid(unsafe_code)]
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
    calls[14].output_digest = include_str!("fixtures/digest-dotenv-output.txt").into();
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
    assert!(
        !result.contains(include_str!("fixtures/digest-dotenv-output.txt"))
            && !result.contains(&"x".repeat(501))
    );
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
    let edit_lines: Vec<_> = digest.split_once("edits:\n").unwrap().1.lines().collect();
    assert_eq!(edit_lines.len(), 51);
    assert!(
        edit_lines[..50]
            .iter()
            .all(|line| line.starts_with("src/very-long-file-name-"))
    );
    assert_eq!(edit_lines[50], "... 2950 more files");
}

#[test]
fn dotenv_calls_hide_inline_args_and_output_for_reads_and_edits() {
    let fixtures: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/digest-dotenv.json")).unwrap();
    for fixture in fixtures.as_array().unwrap() {
        let calls = [Call {
            tool: "write".into(),
            args_digest: fixture["args"].as_str().unwrap().into(),
            output_digest: fixture["output"].as_str().unwrap().into(),
            is_error: true,
            edits: fixture["edit"]
                .as_str()
                .map(|path| FileEdit {
                    path: PathBuf::from(path),
                    kind: EditKind::Modify,
                    via: EditVia::Tool,
                })
                .into_iter()
                .collect(),
            ..Call::default()
        }];
        let digest = build_digest(
            DigestInput {
                prompt: "save configuration",
                calls: &calls,
                tests: None,
            },
            &redactor(),
        );
        assert!(digest.contains("write | [dotenv] | ? | "));
        assert!(!digest.contains(&calls[0].args_digest));
        assert!(!digest.contains(&calls[0].output_digest));
    }
}

#[test]
fn digest_caps_tool_and_error_lines_at_unicode_character_boundaries() {
    let calls = [Call {
        tool: "界".repeat(1_000),
        args_digest: "🦀".repeat(1_000),
        output_digest: format!("{}\nsecond line", "é".repeat(1_000)),
        is_error: true,
        ..Call::default()
    }];
    let digest = build_digest(
        DigestInput {
            prompt: "bounds",
            calls: &calls,
            tests: None,
        },
        &redactor(),
    );
    let line = digest.lines().find(|line| line.contains(" | ")).unwrap();
    let fields: Vec<_> = line.split(" | ").collect();
    assert_eq!(fields[0].chars().count(), 64);
    assert_eq!(fields[1].chars().count(), 120);
    assert_eq!(fields[3].chars().count(), 120);
    assert!(!digest.contains("second line"));
    assert!(digest.len() <= MAX_DIGEST_BYTES);
}
