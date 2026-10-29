#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::path::{Path, PathBuf};
use tkm_decide::redact::{RedactContext, Redactor, is_dotenv_path};
#[test]
fn secret_corpus_and_paths_are_redacted() {
    let redactor = Redactor::new(RedactContext {
        repo_root: PathBuf::from("/repo"),
        home: PathBuf::from("/home/test"),
        env: vec![("MY_TOKEN".into(), "fakeenvvalue123".into())],
    });
    let samples = [
        "AKIA1234567890ABCDEF",
        "ghp_abcdefghijklmnopqrstuvwxyz1234567890",
        "github_pat_abcdefghijklmnopqrstuvwxyz123456",
        "sk-or-v1-abcdef0123456789abcdef0123456789",
        "sk-ant-abcdef0123456789abcdef0123456789",
        "sk-proj-abcdef0123456789abcdef0123456789",
        "sk-svcacct-abcdef0123456789abcdef0123456789",
        "eyJabcdef.eyJabcdef.signature",
        "fakeenvvalue123",
        "/outside/private",
    ];
    let text = samples.join(" ");
    let output = redactor.redact(&text);
    for sample in samples {
        assert!(!output.contains(sample), "survived: {sample}");
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
fn fixture_covers_secret_patterns() {
    let redactor = Redactor::new(RedactContext {
        repo_root: PathBuf::from("/repo"),
        home: PathBuf::from("/home/test"),
        env: vec![("MY_TOKEN".into(), "fakeenvvalue123".into())],
    });
    let result = redactor.redact(include_str!("fixtures/redact-corpus.txt"));
    for secret in [
        "fakeawssecret0123456789",
        "fakemoonshotabcdef",
        "fakedeepseekabcdef",
        "fakecursorabcdef",
        "FAKEPRIVATEKEYDATA",
        "/home/test/private",
    ] {
        assert!(!result.contains(secret));
    }
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
        "[env:MY_TOKEN]",
        "[outside-repo]",
    ] {
        assert!(result.contains(marker), "missing {marker}");
    }
}
