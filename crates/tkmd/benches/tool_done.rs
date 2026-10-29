#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
//! In-memory `tool_done` handler cost (SPEC 14: < 200 µs): a realistic Claude `PostToolUse`
//! request through map, fold, gate and render.

use std::collections::BTreeMap;
use std::hint::black_box;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use criterion::{BatchSize, Criterion, criterion_group, criterion_main};
use tkm_config::paths::Paths;
use tkm_core::model::{RepoKey, Timestamp};
use tkm_decide::api::{Answer, Usage};
use tkm_decide::calibrate::{CalibrationKey, CalibrationStore, Thresholds};
use tkm_decide::client::{DecideRequest, Decision, Outcome};
use tkm_proto::{EventKind, Host, PROTOCOL_VERSION, Request};
use tkm_store::{Repo, Store};
use tkmd::daemon::{Daemon, DaemonOptions};
use tkmd::decider::{BoxFuture, Decider};
use tkmd::settings::{FixedSettings, Settings};

const POST_TOOL_USE: &str = include_str!("../../../fixtures/claude/hooks/PostToolUse-bash.json");
const SESSION_START: &str = include_str!("../../../fixtures/claude/hooks/SessionStart.json");
const PROMPT: &str = include_str!("../../../fixtures/claude/hooks/UserPromptSubmit.json");
const FAILURE: &str = include_str!("../../../fixtures/claude/hooks/PostToolUseFailure-bash.json");
const STOP: &str = include_str!("../../../fixtures/claude/hooks/Stop.json");
const MODEL: &str = "benchmark/model-20260101";

struct StuckDecider;

struct ToggleAdapter {
    inner: tkm_hosts::claude::ClaudeAdapter,
    blind: Arc<AtomicBool>,
}

impl tkm_hosts::HostAdapter for ToggleAdapter {
    fn host(&self) -> Host {
        Host::Claude
    }
    fn map_event(
        &self,
        request: &Request,
    ) -> Result<Option<tkm_core::model::HookEvent>, tkm_hosts::AdapterError> {
        self.inner.map_event(request)
    }
    fn render_reply(&self, request: &Request, action: &tkm_proto::ReplyAction) -> String {
        if request.event == EventKind::ToolDone && self.blind.load(Ordering::Relaxed) {
            String::new()
        } else {
            self.inner.render_reply(request, action)
        }
    }
    fn read_session(
        &self,
        source: &tkm_hosts::SessionSource,
    ) -> Result<tkm_core::model::Session, tkm_hosts::AdapterError> {
        self.inner.read_session(source)
    }
    fn install(
        &self,
        context: &tkm_hosts::InstallContext,
    ) -> Result<tkm_hosts::InstallReport, tkm_hosts::AdapterError> {
        self.inner.install(context)
    }
    fn uninstall(
        &self,
        context: &tkm_hosts::InstallContext,
    ) -> Result<tkm_hosts::InstallReport, tkm_hosts::AdapterError> {
        self.inner.uninstall(context)
    }
    fn prune_target(&self) -> tkm_hosts::PruneTarget {
        self.inner.prune_target()
    }
}

impl Decider for StuckDecider {
    fn decide(&self, _request: DecideRequest) -> BoxFuture<'_, Outcome> {
        Box::pin(async {
            let mut answers = BTreeMap::new();
            answers.insert("stuck".into(), Answer::Noul { noul: 0.95 });
            answers.insert("progress".into(), Answer::Noul { noul: 0.1 });
            answers.insert(
                "redirect".into(),
                Answer::Choice {
                    choice: "reread_error".into(),
                    confidence: Some(0.9),
                    probabilities: None,
                },
            );
            Outcome::Decision(Decision {
                model_version: MODEL.into(),
                requested_model: MODEL.into(),
                provider: None,
                answers,
                usage: Usage::default(),
                latency_ms: 0,
                cached: false,
            })
        })
    }
}

fn request(event: EventKind, payload: &str) -> Request {
    let payload: serde_json::Value = serde_json::from_str(payload).unwrap();
    Request {
        version: PROTOCOL_VERSION,
        host: Host::Claude,
        event,
        host_event: payload["hook_event_name"].as_str().map(str::to_owned),
        payload,
        env: BTreeMap::new(),
        cwd: None,
    }
}

fn bench_gate_open(
    c: &mut Criterion,
    runtime: &tokio::runtime::Runtime,
    daemon: &Arc<Daemon>,
    failing: &Request,
) {
    c.bench_function("tool_done_gate_open", |b| {
        b.iter_batched(
            || {
                runtime.block_on(async {
                    daemon.settle().await;
                    daemon.handle(request(EventKind::TurnEnd, STOP)).await;
                    daemon.handle(request(EventKind::Prompt, PROMPT)).await;
                });
                for _ in 0..17 {
                    daemon.handle_tool_done(failing);
                }
            },
            |()| black_box(daemon.handle_tool_done(black_box(failing))),
            BatchSize::PerIteration,
        );
    });
}

fn bench_pending_redirect(
    c: &mut Criterion,
    runtime: &tokio::runtime::Runtime,
    daemon: &Arc<Daemon>,
    failing: &Request,
    blind: &AtomicBool,
) {
    c.bench_function("tool_done_pending_redirect", |b| {
        b.iter_batched(
            || {
                runtime.block_on(async {
                    daemon.settle().await;
                    daemon.handle(request(EventKind::TurnEnd, STOP)).await;
                    daemon.handle(request(EventKind::Prompt, PROMPT)).await;
                });
                for _ in 0..18 {
                    daemon.handle_tool_done(failing);
                }
                runtime.block_on(daemon.settle());
                blind.store(true, Ordering::Relaxed);
                daemon.handle_tool_done(failing);
                blind.store(false, Ordering::Relaxed);
            },
            |()| black_box(daemon.handle_tool_done(black_box(failing))),
            BatchSize::PerIteration,
        );
    });
}

fn tool_done(c: &mut Criterion) {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path().join("home");
    let paths = Paths::from_dirs(
        &home,
        Some(&dir.path().join("data")),
        Some(&dir.path().join("config")),
        Some(&dir.path().join("run")),
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let store = Store::open(&paths.db).unwrap();
    let repo = RepoKey("dir:~/dev/example".into());
    store
        .upsert_repo(&Repo {
            repo_key: repo.clone(),
            display_name: "example".into(),
            first_seen: Timestamp(1),
            last_seen: Timestamp(1),
            delegated_runs: 0,
        })
        .unwrap();
    for _ in 0..5 {
        store.push_envelope_sample(&repo, Host::Claude, 5).unwrap();
    }
    drop(store);
    let mut options = DaemonOptions::new(paths);
    // Steady state: the initial shadow window has elapsed, so a stuck verdict acts.
    let settings = Settings {
        shadow_days: 0,
        shadow_turns: 0,
        ..Settings::default()
    };
    options.settings = Arc::new(FixedSettings::new(settings));
    options.decider = Arc::new(StuckDecider);
    let daemon: Arc<Daemon> = Daemon::new(options);
    let blind = Arc::new(AtomicBool::new(false));
    daemon.adapters().register(Box::new(ToggleAdapter {
        inner: tkm_hosts::claude::ClaudeAdapter::new(),
        blind: blind.clone(),
    }));
    daemon.calibration().put(
        CalibrationKey {
            model_version: MODEL.into(),
            purpose: "loop_guard".into(),
        },
        Thresholds::default(),
    );
    runtime.block_on(async {
        daemon.start_workers();
        daemon
            .handle(request(EventKind::SessionStart, SESSION_START))
            .await;
        daemon.handle(request(EventKind::Prompt, PROMPT)).await;
    });
    let hot = request(EventKind::ToolDone, POST_TOOL_USE);
    c.bench_function("tool_done_claude_posttooluse", |b| {
        b.iter(|| black_box(daemon.handle_tool_done(black_box(&hot))));
    });

    let mut failing = request(EventKind::ToolDone, FAILURE);
    failing.payload["error"] = "ValueError: boom".into();
    runtime.block_on(async {
        daemon.settle().await;
        daemon.handle(request(EventKind::TurnEnd, STOP)).await;
        daemon.handle(request(EventKind::Prompt, PROMPT)).await;
    });
    for _ in 0..17 {
        daemon.handle_tool_done(&failing);
    }
    let mut monitor = daemon.subscribe();
    daemon.handle_tool_done(&failing);
    assert!(
        matches!(monitor.try_recv(), Ok(tkm_proto::monitor::MonitorMessage::Session(update)) if update.loop_state == "judging")
    );
    drop(monitor);
    runtime.block_on(daemon.settle());
    blind.store(true, Ordering::Relaxed);
    assert_eq!(daemon.handle_tool_done(&failing).stdout, "");
    blind.store(false, Ordering::Relaxed);
    let delivered = daemon.handle_tool_done(&failing);
    assert!(
        matches!(delivered.action, tkm_proto::ReplyAction::Inject { .. }),
        "redirect must be delivered once the host can show it: {delivered:?}"
    );
    bench_gate_open(c, &runtime, &daemon, &failing);
    bench_pending_redirect(c, &runtime, &daemon, &failing, &blind);
}

criterion_group!(benches, tool_done);
criterion_main!(benches);
