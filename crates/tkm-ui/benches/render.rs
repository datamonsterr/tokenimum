#![forbid(unsafe_code)]
use criterion::{Criterion, criterion_group, criterion_main};
use ratatui::{Terminal, backend::TestBackend};
use tkm_proto::{
    Host,
    monitor::{
        DecisionNotice, FactNotice, MonitorMessage, RedirectNotice, SessionUpdate, SpendUpdate,
    },
};
use tkm_ui::{
    model::{MonitorState, Savings},
    tui::{self, Pane, ViewState},
};

fn render_benches(c: &mut Criterion) {
    let mut state = MonitorState::default();
    for i in 0_u64..500 {
        state.apply(MonitorMessage::Session(SessionUpdate {
            host: Host::Claude,
            session_id: format!("session-{i}"),
            turn_id: Some(i),
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
    state.spend = Some(SpendUpdate {
        today_usd: 1.0,
        daily_cap_usd: 10.0,
        session_cap_usd: 2.0,
    });
    state.spend_days = (1..=7)
        .map(|day| (format!("2026-10-{day:02}"), f64::from(day)))
        .collect();
    for i in 0..500 {
        state.savings.push(Savings {
            repo: format!("repo-{}", i % 10),
            host: format!("host-{}", i % 3),
            cost: f64::from(i) / 100.0,
            shadow: i % 2 == 0,
        });
    }
    for i in 0..1000 {
        state.apply(MonitorMessage::Redirect(RedirectNotice {
            host: Host::Claude,
            session_id: format!("session-{i}"),
            triggering_calls: vec!["Bash".into()],
            kind: "redirect".into(),
            text: "try again".into(),
            shadow: false,
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
        let Ok(mut terminal) = Terminal::new(TestBackend::new(200, 50));
        c.bench_function(name, |b| {
            b.iter(|| {
                let _ = terminal.draw(|frame| tui::render(&state, &view, frame));
            });
        });
    }
}
criterion_group!(benches, render_benches);
criterion_main!(benches);
