#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Read and fold one session with decisions disabled. Wall time bounds this single-threaded CPU time.

use criterion::{Criterion, criterion_group, criterion_main};
use std::collections::BTreeSet;
use std::hint::black_box;
use std::path::PathBuf;
use tkm_core::facts::{FactsConfig, FactsState};
use tkm_core::model::RepoKey;
use tkm_hosts::{HostAdapter, SessionSource, claude::ClaudeAdapter, pi::PiAdapter};
use tkmd::fold::replay_fold;

fn fold_session(c: &mut Criterion) {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let fixtures: Vec<(&str, Box<dyn HostAdapter>, &str, &str)> = vec![
        (
            "fold_session_claude",
            Box::new(ClaudeAdapter::new()),
            "fixtures/claude/sessions/e928a2cf-e31b-4154-805b-919991d042ea.jsonl",
            "e928a2cf",
        ),
        (
            "fold_session_pi",
            Box::new(PiAdapter::new()),
            "crates/tkm-hosts/tests/fixtures/pi/session.jsonl",
            "00000000-0000-4000-8000-000000000001",
        ),
    ];
    for (name, adapter, path, id) in fixtures {
        let source = SessionSource {
            agent_home: "/synthetic/agent".into(),
            path: root.join(path),
            session_id: id.into(),
            repo_key: RepoKey("fixture".into()),
            cwd: "/home/user/project".into(),
        };
        let probe = adapter.read_session(&source).unwrap();
        assert!(
            !probe.partial && !probe.turns.is_empty(),
            "fixture must contain a complete session"
        );
        c.bench_function(name, |b| {
            b.iter(|| {
                let session = adapter.read_session(black_box(&source)).unwrap();
                black_box(replay_fold(
                    &session,
                    FactsState::default(),
                    &BTreeSet::new(),
                    &FactsConfig::default(),
                ));
            });
        });
    }
}
criterion_group!(benches, fold_session);
criterion_main!(benches);
