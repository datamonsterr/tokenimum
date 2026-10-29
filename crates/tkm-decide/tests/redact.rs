#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::path::{Path, PathBuf};
use tkm_decide::redact::{RedactContext, Redactor, is_dotenv_path};
#[test]
fn secret_corpus_and_paths_are_redacted() {
    let redactor = Redactor::new(RedactContext {
        repo_root: PathBuf::from("/repo"),
        home: PathBuf::from("/home/test"),
        env: vec![(
            "MY_TOKEN".into(),
            include_str!("fixtures/redact-env-value.txt").into(),
        )],
    });
    let samples: Vec<String> =
        serde_json::from_str(include_str!("fixtures/redact-samples.json")).unwrap();
    let text = samples.join(" ");
    let output = redactor.redact(&text);
    for sample in samples {
        assert!(!output.contains(&sample), "survived: {sample}");
    }
    assert!(
        output.contains("[redacted:aws")
            && output.contains("[redacted:github]")
            && output.contains("[env:MY_TOKEN]")
            && output.contains("[outside-repo]")
    );
    assert!(is_dotenv_path(Path::new("config/.env.local")));
}

#[test]
fn redaction_corpus_removes_every_secret_kind() {
    let redactor = Redactor::new(RedactContext {
        repo_root: PathBuf::from("/repo"),
        home: PathBuf::from("/home/test"),
        env: vec![(
            "MY_TOKEN".into(),
            include_str!("fixtures/redact-env-value.txt").into(),
        )],
    });
    let result = redactor.redact(include_str!("fixtures/redact-corpus.txt"));
    let forbidden: Vec<String> =
        serde_json::from_str(include_str!("fixtures/redact-forbidden.json")).unwrap();
    for secret in forbidden {
        assert!(!result.contains(&secret), "survived: {secret}");
    }
    assert!(!result.contains("/home/test"));
    for marker in [
        "[redacted:aws]",
        "[redacted:aws-secret]",
        "[redacted:github]",
        "[redacted:openrouter]",
        "[redacted:anthropic]",
        "[redacted:openai]",
        "[redacted:cursor]",
        "[redacted:jwt]",
        "[redacted:private-key]",
        "[redacted:bearer]",
        "[redacted:authorization]",
        "[redacted:url-credentials]",
        "[env:MY_TOKEN]",
        "[outside-repo]",
    ] {
        assert!(result.contains(marker), "missing {marker}");
    }
}

#[test]
fn home_environment_path_does_not_hide_repo_paths() {
    let redactor = Redactor::new(RedactContext {
        repo_root: PathBuf::from("/home/test/repo"),
        home: PathBuf::from("/home/test"),
        env: vec![("HOME".into(), "/home/test".into())],
    });
    assert_eq!(
        redactor.redact("/home/test/repo/src/main.rs"),
        "src/main.rs"
    );
    assert_eq!(redactor.redact("prefix/home/test/notes"), "prefix~/notes");
}

#[test]
fn redaction_handles_pem_labels_headers_credentials_phrases_and_path_delimiters() {
    let redactor = Redactor::new(RedactContext {
        repo_root: PathBuf::from("/repo"),
        home: PathBuf::from("/home/test"),
        env: vec![],
    });
    let fixtures: serde_json::Value =
        serde_json::from_str(include_str!("fixtures/redact-cases.json")).unwrap();
    for fixture in fixtures.as_array().unwrap() {
        assert_eq!(
            redactor.redact(fixture["text"].as_str().unwrap()),
            fixture["expected"].as_str().unwrap(),
            "case: {}",
            fixture["name"]
        );
    }
}

#[test]
fn secret_named_environment_values_are_redacted_even_with_path_prefixes() {
    let env: Vec<(String, String)> =
        serde_json::from_str(include_str!("fixtures/redact-secret-env.json")).unwrap();
    let redactor = Redactor::new(RedactContext {
        repo_root: PathBuf::from("/repo"),
        home: PathBuf::from("/home/test"),
        env: env.clone(),
    });
    for (name, value) in env {
        assert_eq!(redactor.redact(&value), format!("[env:{name}]"));
    }
}

#[test]
fn empty_repo_root_treats_every_absolute_path_as_outside() {
    let redactor = Redactor::new(RedactContext {
        repo_root: PathBuf::new(),
        home: PathBuf::new(),
        env: vec![],
    });
    assert_eq!(redactor.redact("/repo/src/main.rs"), "[outside-repo]");
}

#[test]
fn dotenv_names_include_dotfiles_variants_and_case_insensitive_extensions() {
    for path in [
        ".env",
        "config/.env.local",
        "config/service.env",
        "config/service.ENV",
        "config/.ENV.local",
    ] {
        assert!(is_dotenv_path(Path::new(path)), "missed {path}");
    }
    for path in ["env", "config/env.txt", "config/service.environment"] {
        assert!(!is_dotenv_path(Path::new(path)), "misclassified {path}");
    }
}
