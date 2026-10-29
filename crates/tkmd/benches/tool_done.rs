#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
//! In-memory `tool_done` handler cost (SPEC 14: < 200 µs): a realistic Claude `PostToolUse`
//! request through map, fold, gate and render.

use std::collections::BTreeMap;
use std::hint::black_box;
use std::sync::Arc;

use criterion::{Criterion, criterion_group, criterion_main};
use tkm_config::paths::Paths;
use tkm_proto::{EventKind, Host, PROTOCOL_VERSION, Request};
use tkmd::daemon::{Daemon, DaemonOptions};

const POST_TOOL_USE: &str = include_str!("../../../fixtures/claude/hooks/PostToolUse-bash.json");
const SESSION_START: &str = include_str!("../../../fixtures/claude/hooks/SessionStart.json");
const PROMPT: &str = include_str!("../../../fixtures/claude/hooks/UserPromptSubmit.json");

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
    let daemon: Arc<Daemon> = Daemon::new(DaemonOptions::new(paths));
    runtime.block_on(async {
        daemon
            .handle(request(EventKind::SessionStart, SESSION_START))
            .await;
        daemon.handle(request(EventKind::Prompt, PROMPT)).await;
    });
    let hot = request(EventKind::ToolDone, POST_TOOL_USE);
    c.bench_function("tool_done_claude_posttooluse", |b| {
        b.iter(|| black_box(daemon.handle_tool_done(black_box(&hot))));
    });
}

criterion_group!(benches, tool_done);
criterion_main!(benches);
