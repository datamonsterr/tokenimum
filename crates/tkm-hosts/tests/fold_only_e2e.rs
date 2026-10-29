#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
//! `--no-shims` end to end: session fixtures -> facts -> inject render -> marked block.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use tkm_core::facts::render::claim;
use tkm_core::facts::{FactsConfig, FactsState, SessionEvidence, update};
use tkm_core::inject::{Config, InjectCandidate, Source, estimate_tokens, render, select};
use tkm_core::model::{RepoKey, Session};
use tkm_hosts::claude::ClaudeAdapter;
use tkm_hosts::codex::CodexAdapter;
use tkm_hosts::fold_only::instructions::{ApplyOptions, apply};
use tkm_hosts::fold_only::{IntegrationMode, mode_for};
use tkm_hosts::{HostAdapter, SessionSource};
use tkm_proto::Host;

const BEGIN: &str = "<!-- tokenimum:begin -->";
const END: &str = "<!-- tokenimum:end -->";

struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn scratch_repo() -> Scratch {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let path = std::env::temp_dir().join(format!(
        "tkm-fold-e2e-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    fs::create_dir_all(&path).unwrap();
    let status = Command::new("git")
        .args(["init", "-q", "--initial-branch=main"])
        .current_dir(&path)
        .status()
        .unwrap();
    assert!(status.success());
    Scratch(path)
}

fn fixture(relative: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures")
        .join(relative)
}

fn read(adapter: &dyn HostAdapter, relative: &str, session_id: &str) -> Session {
    adapter
        .read_session(&SessionSource {
            agent_home: "/synthetic/agent".into(),
            path: fixture(relative),
            session_id: session_id.into(),
            repo_key: RepoKey("synthetic-origin".into()),
            cwd: "/home/user/project".into(),
        })
        .unwrap()
}

fn fold(sessions: Vec<Session>) -> FactsState {
    let cfg = FactsConfig::default();
    sessions
        .into_iter()
        .fold(FactsState::default(), |state, session| {
            let evidence = SessionEvidence {
                session,
                injected_ids: BTreeSet::new(),
                decisions: None,
            };
            update(state, &evidence, &cfg).0
        })
}

fn body_for(state: &FactsState) -> String {
    let items: Vec<_> = state
        .facts
        .values()
        .map(|fact| claim(&fact.candidate))
        .filter(|text| !text.is_empty())
        .map(|text| InjectCandidate {
            id: text.clone(),
            source: Source::Facts,
            tokens: estimate_tokens(&text),
            text,
            fire_gate_prevents: false,
        })
        .collect();
    render(&select(&items, None, Config::default()))
}

#[test]
fn no_shims_fold_produces_facts_and_regenerates_marker_block() {
    // Hosts without shims fall back to fold-only; hook hosts keep hooks.
    assert_eq!(mode_for(Host::Opencode, false), IntegrationMode::FoldOnly);

    let sessions = vec![
        read(
            &ClaudeAdapter::new(),
            "claude/sessions/e928a2cf-e31b-4154-805b-919991d042ea.jsonl",
            "e928a2cf",
        ),
        read(
            &CodexAdapter::new(),
            "codex/sessions/2026/10/29/rollout-2026-10-29T09-00-20-01a18562-fb1c-7522-9939-dda89fa83662.jsonl",
            "01a18562-fb1c-7522-9939-dda89fa83662",
        ),
    ];
    let state = fold(sessions);
    assert_eq!(state.processed.len(), 2, "both fixture sessions fold");
    assert!(!state.facts.is_empty(), "fixtures yield candidate facts");

    let first_body = body_for(&state);
    assert!(first_body.starts_with("# Context\n"), "{first_body:?}");

    let repo = scratch_repo();
    let agents = repo.0.join("AGENTS.md");
    let prefix = "# Project guide\n\nHand-written rules stay.\n";
    let suffix = "\n## License\n\nSee LICENSE.\n";
    fs::write(&agents, format!("{prefix}{suffix}")).unwrap();

    let opts = ApplyOptions::default();
    apply(&repo.0, Host::Opencode, &first_body, opts).unwrap();
    let after_first = fs::read_to_string(&agents).unwrap();
    assert!(after_first.starts_with(prefix));
    assert!(after_first.contains(first_body.trim_end()));

    let second_body = format!("{first_body}- Second regeneration marker.\n");
    apply(&repo.0, Host::Opencode, &second_body, opts).unwrap();
    let after_second = fs::read_to_string(&agents).unwrap();
    let start = after_second.find(BEGIN).unwrap();
    let end = after_second.find(END).unwrap() + END.len();
    assert!(after_second[start..end].contains("Second regeneration marker."));
    assert_eq!(after_second.matches(BEGIN).count(), 1);
    assert!(after_second.starts_with(prefix));
    // Outside the markers: identical to what the first apply left.
    let outside = |text: &str| {
        let s = text.find(BEGIN).unwrap();
        let e = text.find(END).unwrap() + END.len();
        format!("{}{}", &text[..s], &text[e..])
    };
    assert_eq!(outside(&after_first), outside(&after_second));

    // A pre-existing file is not excluded; repeated applies never touch exclude.
    let exclude = fs::read_to_string(repo.0.join(".git/info/exclude")).unwrap_or_default();
    assert_eq!(exclude.lines().filter(|l| *l == "/AGENTS.md").count(), 0);
}

#[test]
fn created_instructions_file_is_excluded_exactly_once() {
    let repo = scratch_repo();
    let opts = ApplyOptions::default();
    apply(&repo.0, Host::Opencode, "# Context\n- one\n", opts).unwrap();
    apply(&repo.0, Host::Opencode, "# Context\n- two\n", opts).unwrap();
    let exclude = fs::read_to_string(repo.0.join(".git/info/exclude")).unwrap();
    assert_eq!(exclude.lines().filter(|l| *l == "/AGENTS.md").count(), 1);
    assert!(
        fs::read_to_string(repo.0.join("AGENTS.md"))
            .unwrap()
            .contains("- two")
    );
}
