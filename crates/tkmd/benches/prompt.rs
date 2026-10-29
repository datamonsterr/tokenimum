#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
//! Prompt handling with a deterministic relevance decision and the block cache.

use std::collections::BTreeMap;
use std::hint::black_box;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use tkm_config::paths::Paths;
use tkm_decide::api::{Answer, Usage};
use tkm_decide::client::{DecideRequest, Decision, Outcome};
use tkm_proto::{EventKind, Host, PROTOCOL_VERSION, Request};
use tkmd::daemon::{Daemon, DaemonOptions};
use tkmd::decider::{BoxFuture, Decider};
use tkmd::settings::{FixedSettings, Settings};

struct ScoreDecider(AtomicUsize);

impl Decider for ScoreDecider {
    fn decide(&self, request: DecideRequest) -> BoxFuture<'_, Outcome> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Box::pin(async move {
            let answers = request
                .questions
                .keys()
                .map(|key| {
                    (
                        key.clone(),
                        Answer::Score {
                            score: 3.0,
                            confidence: None,
                            legend: None,
                            probabilities: None,
                        },
                    )
                })
                .collect();
            Outcome::Decision(Decision {
                model_version: "benchmark/model-20260101".into(),
                requested_model: "benchmark/model-20260101".into(),
                provider: None,
                answers,
                usage: Usage::default(),
                latency_ms: 0,
                cached: false,
            })
        })
    }
}

fn request(kind: EventKind, fixture: &str, cwd: &str, prompt: &str, id: &str) -> Request {
    let mut payload: serde_json::Value = serde_json::from_str(fixture).unwrap();
    payload["cwd"] = cwd.into();
    payload["session_id"] = id.into();
    if kind == EventKind::Prompt {
        payload["prompt"] = prompt.into();
    }
    Request {
        version: PROTOCOL_VERSION,
        host: Host::Claude,
        event: kind,
        host_event: payload["hook_event_name"].as_str().map(str::to_owned),
        payload,
        env: BTreeMap::new(),
        cwd: Some(cwd.into()),
    }
}

fn prompt(c: &mut Criterion) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("repo");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::write(
        root.join("MEMORY.md"),
        "- [build](build.md) — build instructions\n- [tests](tests.md) — test instructions\n",
    )
    .unwrap();
    let cwd = root.to_str().unwrap();
    let paths = Paths::from_dirs(
        &dir.path().join("home"),
        Some(&dir.path().join("data")),
        Some(&dir.path().join("config")),
        Some(&dir.path().join("run")),
    );
    let decider = Arc::new(ScoreDecider(AtomicUsize::new(0)));
    let mut options = DaemonOptions::new(paths);
    options.settings = Arc::new(FixedSettings::new(Settings::default()));
    options.decider = decider.clone();
    let daemon = Daemon::new(options);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let start = include_str!("../../../fixtures/claude/hooks/SessionStart.json");
    let fixture = include_str!("../../../fixtures/claude/hooks/UserPromptSubmit.json");
    let mut n = 0_u64;
    let mut setup = |prompt: &str| {
        n += 1;
        let id = format!("benchmark-{n}");
        runtime.block_on(daemon.handle(request(EventKind::SessionStart, start, cwd, "", &id)));
        request(EventKind::Prompt, fixture, cwd, prompt, &id)
    };
    let first = setup("cached prompt");
    let reply = runtime.block_on(daemon.handle(first));
    assert!(
        matches!(reply.action, tkm_proto::ReplyAction::Inject { .. }),
        "scoring must produce a block"
    );
    let count = decider.0.load(Ordering::Relaxed);
    assert_eq!(count, 1, "fixture must trigger relevance scoring");
    let second = setup("cached prompt");
    let reply = runtime.block_on(daemon.handle(second));
    assert!(
        matches!(reply.action, tkm_proto::ReplyAction::Inject { .. }),
        "cached block must be delivered"
    );
    assert_eq!(
        decider.0.load(Ordering::Relaxed),
        count,
        "second prompt must hit cache"
    );
    let mut group = c.benchmark_group("prompt");
    group
        .sample_size(10)
        .measurement_time(std::time::Duration::from_secs(1))
        .warm_up_time(std::time::Duration::from_secs(1));
    group.bench_function("cached", |b| {
        b.iter_batched(
            || setup("cached prompt"),
            |req| black_box(runtime.block_on(daemon.handle(black_box(req)))),
            BatchSize::PerIteration,
        );
    });
    let mut prompt_id = 0_u64;
    group.bench_function("scored", |b| {
        b.iter_batched(
            || {
                prompt_id += 1;
                setup(&format!("uncached prompt {prompt_id}"))
            },
            |req| black_box(runtime.block_on(daemon.handle(black_box(req)))),
            BatchSize::PerIteration,
        );
    });
    group.finish();
    assert!(
        decider.0.load(Ordering::Relaxed) > count,
        "scored path must invoke the decider"
    );
}
criterion_group!(benches, prompt);
criterion_main!(benches);
