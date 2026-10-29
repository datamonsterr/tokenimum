use criterion::{Criterion, criterion_group, criterion_main};
use ratatui::{Terminal, backend::TestBackend};
use tkm_proto::{
    Host,
    monitor::{DecisionNotice, FactNotice, MonitorMessage, SessionUpdate},
};
use tkm_ui::{
    model::MonitorState,
    tui::{self, Pane, ViewState},
};

fn render_benches(c: &mut Criterion) {
    let mut state = MonitorState::default();
    for i in 0..500 {
        state.apply(MonitorMessage::Session(SessionUpdate {
            host: Host::Claude,
            session_id: format!("session-{i}"),
            repo: Some("example/repo".into()),
            phase: "active".into(),
            turn_calls: 20,
            envelope: Some(40.0),
            loop_state: "watching".into(),
        }));
        state.apply(MonitorMessage::Fact(FactNotice {
            repo: "example/repo".into(),
            fact_id: format!("fact-{i}"),
            kind: "promoted".into(),
        }));
    }
    for _ in 0..1000 {
        state.apply(MonitorMessage::Decision(DecisionNotice {
            purpose: "loop_guard".into(),
            model: "example-model".into(),
            latency_ms: 50,
            cost_usd: 0.0001,
            shadow: false,
            answers: serde_json::json!({"safe": true}),
        }));
    }
    for (pane, name) in Pane::ALL.into_iter().zip([
        "render_sessions",
        "render_decisions",
        "render_spend",
        "render_savings",
        "render_facts",
        "render_redirects",
    ]) {
        let view = ViewState {
            pane,
            ..ViewState::default()
        };
        let Ok(mut terminal) = Terminal::new(TestBackend::new(200, 50)) else {
            return;
        };
        c.bench_function(name, |b| {
            b.iter(|| {
                let _ = terminal.draw(|frame| tui::render(&state, &view, frame));
            })
        });
    }
}
criterion_group!(benches, render_benches);
criterion_main!(benches);
