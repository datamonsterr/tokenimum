use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{backend::TestBackend, Terminal};
use tkm_proto::{monitor::{DecisionNotice, FactNotice, MonitorMessage, RedirectNotice, SessionUpdate, SpendUpdate}, Host};
use tkm_ui::{feed::{self, Mode}, model::{MonitorState, HISTORY_LIMIT}, tui::{self, Action, Pane, ViewState}};

fn decision(n: usize) -> MonitorMessage {
    MonitorMessage::Decision(DecisionNotice { purpose: format!("p{n}"), model: "model".into(), latency_ms: 4, cost_usd: 0.01, shadow: true, answers: serde_json::json!({"ok": true}) })
}

#[test]
fn apply_updates_each_kind_and_bounds_history() {
    let mut state = MonitorState::default();
    state.apply(MonitorMessage::Hello { version: 1 });
    state.apply(MonitorMessage::Session(SessionUpdate { host: Host::Claude, session_id: "s".into(), repo: Some("repo".into()), phase: "active".into(), turn_calls: 5, envelope: Some(9.0), loop_state: "watching".into() }));
    state.apply(MonitorMessage::Spend(SpendUpdate { today_usd: 0.1, daily_cap_usd: 2.0, session_cap_usd: 1.0 }));
    state.apply(MonitorMessage::Fact(FactNotice { repo: "repo".into(), fact_id: "f".into(), kind: "promoted".into() }));
    for n in 0..=HISTORY_LIMIT { state.apply(decision(n)); }
    for n in 0..=HISTORY_LIMIT { state.apply(MonitorMessage::Redirect(RedirectNotice { host: Host::Claude, session_id: "s".into(), kind: "redirect".into(), text: n.to_string(), shadow: false })); }
    assert_eq!(state.sessions.len(), 1);
    assert_eq!(state.spend.as_ref().unwrap().today_usd, 0.1);
    assert_eq!(state.facts[0].state, "active");
    assert_eq!(state.decisions.len(), HISTORY_LIMIT);
    assert_eq!(state.decisions.front().unwrap().purpose, "p1");
    assert_eq!(state.redirects.len(), HISTORY_LIMIT);
}

#[test]
fn keyboard_navigation_and_actions() {
    let state = MonitorState::default();
    let mut view = ViewState::default();
    let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
    assert_eq!(tui::handle_key(&mut view, key(KeyCode::Tab), &state), Action::None);
    assert_eq!(view.pane, Pane::Decisions);
    tui::handle_key(&mut view, key(KeyCode::Char('6')), &state);
    assert_eq!(view.pane, Pane::Redirects);
    assert_eq!(tui::handle_key(&mut view, key(KeyCode::Enter), &state), Action::Explain);
    assert_eq!(tui::handle_key(&mut view, key(KeyCode::Char('r')), &state), Action::Reload);
    assert_eq!(tui::handle_key(&mut view, key(KeyCode::Char('q')), &state), Action::Quit);
}

#[test]
fn each_pane_and_overlays_render_deterministically() {
    let state = MonitorState::default();
    let mut view = ViewState::default();
    for pane in Pane::ALL {
        view.pane = pane;
        let mut first = Terminal::new(TestBackend::new(80, 12)).unwrap();
        let mut second = Terminal::new(TestBackend::new(80, 12)).unwrap();
        first.draw(|frame| tui::render(&state, &view, frame)).unwrap();
        second.draw(|frame| tui::render(&state, &view, frame)).unwrap();
        assert_eq!(first.backend().buffer(), second.backend().buffer());
    }
    view.help = true;
    let mut terminal = Terminal::new(TestBackend::new(80, 12)).unwrap();
    terminal.draw(|frame| tui::render(&state, &view, frame)).unwrap();
    let help = terminal.backend().buffer().clone();
    view.help = false;
    view.overlay = Some("no stored event for this row yet; press r to reload".into());
    terminal.draw(|frame| tui::render(&state, &view, frame)).unwrap();
    assert_ne!(&help, terminal.backend().buffer());
}

#[test]
fn missing_socket_is_read_only() {
    let dir = tempfile::tempdir().unwrap();
    let paths = tkm_config::paths::Paths::from_dirs(dir.path(), None, None, None);
    assert_eq!(feed::connect(&paths).mode, Mode::ReadOnly);
}
