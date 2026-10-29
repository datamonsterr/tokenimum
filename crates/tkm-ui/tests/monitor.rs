#![forbid(unsafe_code)]
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{Terminal, backend::TestBackend};
use tkm_proto::{
    Host,
    monitor::{
        DecisionNotice, FactNotice, MonitorMessage, RedirectNotice, SessionUpdate, SpendUpdate,
    },
};
use tkm_ui::{
    feed::{self, Mode},
    model::{HISTORY_LIMIT, MonitorState},
    tui::{self, Action, Pane, ViewState},
};

fn decision(n: usize) -> MonitorMessage {
    MonitorMessage::Decision(DecisionNotice {
        purpose: format!("p{n}"),
        model: "model".into(),
        latency_ms: 4,
        cost_usd: 0.01,
        shadow: true,
        answers: serde_json::json!({"ok": true}),
    })
}

#[test]
fn apply_updates_each_kind_and_bounds_history() {
    let mut state = MonitorState::default();
    state.apply(MonitorMessage::Hello { version: 1 });
    state.apply(MonitorMessage::Session(SessionUpdate {
        host: Host::Claude,
        session_id: "s".into(),
        repo: Some("repo".into()),
        phase: "active".into(),
        turn_calls: 5,
        envelope: Some(9.0),
        loop_state: "watching".into(),
    }));
    state.apply(MonitorMessage::Spend(SpendUpdate {
        today_usd: 0.1,
        daily_cap_usd: 2.0,
        session_cap_usd: 1.0,
    }));
    state.apply(MonitorMessage::Fact(FactNotice {
        repo: "repo".into(),
        fact_id: "f".into(),
        kind: "promoted".into(),
    }));
    for n in 0..=HISTORY_LIMIT {
        state.apply(decision(n));
    }
    for n in 0..=HISTORY_LIMIT {
        state.apply(MonitorMessage::Redirect(RedirectNotice {
            host: Host::Claude,
            session_id: "s".into(),
            kind: "redirect".into(),
            text: n.to_string(),
            shadow: false,
        }));
    }
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
    assert_eq!(
        tui::handle_key(&mut view, key(KeyCode::Tab), &state),
        Action::None
    );
    assert_eq!(view.pane, Pane::Decisions);
    let _ = tui::handle_key(&mut view, key(KeyCode::Char('6')), &state);
    assert_eq!(view.pane, Pane::Redirects);
    assert_eq!(
        tui::handle_key(&mut view, key(KeyCode::Enter), &state),
        Action::Explain
    );
    assert_eq!(
        tui::handle_key(&mut view, key(KeyCode::Char('r')), &state),
        Action::Reload
    );
    assert_eq!(
        tui::handle_key(&mut view, key(KeyCode::Char('q')), &state),
        Action::Quit
    );
}

#[test]
fn populated_panes_match_whole_buffer_snapshots() {
    use tkm_ui::model::Savings;
    let mut state = MonitorState::default();
    state.apply(MonitorMessage::Session(SessionUpdate {
        host: Host::Claude,
        session_id: "s".into(),
        repo: Some("repo".into()),
        phase: "active".into(),
        turn_calls: 5,
        envelope: Some(9.0),
        loop_state: "watching".into(),
    }));
    state.apply(decision(0));
    state.apply(MonitorMessage::Spend(SpendUpdate {
        today_usd: 0.1,
        daily_cap_usd: 2.0,
        session_cap_usd: 1.0,
    }));
    state.apply(MonitorMessage::Fact(FactNotice {
        repo: "repo".into(),
        fact_id: "f".into(),
        kind: "promoted".into(),
    }));
    state.apply(MonitorMessage::Redirect(RedirectNotice {
        host: Host::Claude,
        session_id: "s".into(),
        kind: "redirect".into(),
        text: "try again".into(),
        shadow: false,
    }));
    state.savings.push(Savings {
        repo: "repo".into(),
        host: "claude".into(),
        cost: 1.25,
        shadow: false,
    });
    state.savings.push(Savings {
        repo: "repo".into(),
        host: "claude".into(),
        cost: 2.5,
        shadow: true,
    });
    let mut view = ViewState::default();
    let names = [
        "Sessions",
        "Decisions",
        "Spend",
        "Savings",
        "Facts",
        "Redirects",
    ];
    let content = [
        "claude  s  repo  calls 5 / 9.0  watching",
        r#"p0  model  {"ok":true}  4 ms  $0.010000  shadow"#,
        "Today: $0.100000 / $2.00  session cap $1.00; 7-day trend n/a (no timestamps)",
        "repo  claude  recent [1.25] n=1  baseline 2.50 n=1",
        "repo  f  active  score -  confirmations -",
        "claude  s  redirect  acted  try again",
    ];
    for ((pane, title), row) in Pane::ALL.into_iter().zip(names).zip(content) {
        view.pane = pane;
        let mut terminal = Terminal::new(TestBackend::new(120, 5)).unwrap();
        terminal
            .draw(|frame| tui::render(&state, &view, frame))
            .unwrap();
        let tabs = format!(
            "{:<120}",
            " Sessions │ Decisions │ Spend │ Savings │ Facts │ Redirects "
        );
        let top = format!("┌{title}{}┐", "─".repeat(118 - title.len()));
        let selected = format!("│> {row:<116}│");
        let bottom = format!("└{}┘", "─".repeat(118));
        let status = format!(
            "{:<120}",
            "daemon down: read-only, no live feed | last: - | ? help"
        );
        terminal
            .backend()
            .assert_buffer_lines([tabs, top, selected, bottom, status]);
    }
}

#[test]
fn help_and_why_match_whole_buffer_snapshots() {
    let state = MonitorState::default();
    let mut view = ViewState {
        help: true,
        ..ViewState::default()
    };
    let mut terminal = Terminal::new(TestBackend::new(80, 8)).unwrap();
    let tabs = format!(
        "{:<80}",
        " Sessions │ Decisions │ Spend │ Savings │ Facts │ Redirects "
    );
    let bottom = format!("└{}┘", "─".repeat(78));
    let status = format!(
        "{:<80}",
        "daemon down: read-only, no live feed | last: - | ? help"
    );
    let line = |text: &str| format!("│{text:<78}│");
    terminal
        .draw(|frame| tui::render(&state, &view, frame))
        .unwrap();
    terminal.backend().assert_buffer_lines([
        tabs.clone(),
        format!("┌Help{}┐", "─".repeat(74)),
        line("Tab / Shift-Tab or 1-6: pane"),
        line("j/k or arrows: select"),
        line("Enter: why  Esc: close"),
        line("r: reload  ?: help  q / Ctrl-C: quit"),
        bottom.clone(),
        status.clone(),
    ]);
    view.help = false;
    view.overlay = Some("No stored event".into());
    terminal
        .draw(|frame| tui::render(&state, &view, frame))
        .unwrap();
    terminal.backend().assert_buffer_lines([
        tabs,
        format!("┌Why{}┐", "─".repeat(75)),
        line("No stored event"),
        line(""),
        line(""),
        line(""),
        bottom,
        status,
    ]);
}

#[test]
fn missing_socket_is_read_only() {
    let dir = tempfile::tempdir().unwrap();
    let paths = tkm_config::paths::Paths::from_dirs(dir.path(), None, None, None);
    assert_eq!(feed::connect(&paths).mode, Mode::ReadOnly);
}

#[cfg(unix)]
#[test]
fn feed_receives_frames_then_disconnects() {
    use std::{os::unix::net::UnixListener, time::Duration};
    use tkm_proto::monitor::MONITOR_SOCKET_NAME;
    let dir = tempfile::tempdir().unwrap();
    let paths = tkm_config::paths::Paths::from_dirs(dir.path(), None, None, None);
    std::fs::create_dir_all(paths.socket.parent().unwrap()).unwrap();
    let listener = UnixListener::bind(paths.socket.with_file_name(MONITOR_SOCKET_NAME)).unwrap();
    let server = std::thread::spawn(move || {
        use std::io::Write;
        let (mut stream, _) = listener.accept().unwrap();
        for msg in [
            MonitorMessage::Hello {
                version: tkm_proto::PROTOCOL_VERSION,
            },
            decision(3),
        ] {
            stream
                .write_all(
                    &tkm_proto::encode_frame(&msg, tkm_proto::monitor::MONITOR_MAX_FRAME).unwrap(),
                )
                .unwrap();
        }
    });
    let feed = feed::connect(&paths);
    assert_eq!(feed.mode, Mode::Live);
    assert_eq!(
        feed.receiver.recv_timeout(Duration::from_secs(2)).unwrap(),
        decision(3)
    );
    server.join().unwrap();
    assert!(feed.receiver.recv_timeout(Duration::from_secs(2)).is_err());
}

#[cfg(unix)]
#[test]
fn bad_hello_is_read_only() {
    use std::os::unix::net::UnixListener;
    use tkm_proto::monitor::{MONITOR_MAX_FRAME, MONITOR_SOCKET_NAME};
    let dir = tempfile::tempdir().unwrap();
    let paths = tkm_config::paths::Paths::from_dirs(dir.path(), None, None, None);
    std::fs::create_dir_all(paths.socket.parent().unwrap()).unwrap();
    let listener = UnixListener::bind(paths.socket.with_file_name(MONITOR_SOCKET_NAME)).unwrap();
    let server = std::thread::spawn(move || {
        use std::io::Write;
        let (mut stream, _) = listener.accept().unwrap();
        let frame =
            tkm_proto::encode_frame(&MonitorMessage::Hello { version: 0 }, MONITOR_MAX_FRAME)
                .unwrap();
        stream.write_all(&frame).unwrap();
    });
    assert_eq!(feed::connect(&paths).mode, Mode::ReadOnly);
    server.join().unwrap();
}

#[test]
fn snapshot_reads_persisted_decision() {
    use tkm_store::{NewDecision, Store};
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("monitor.db");
    let store = Store::open(&path).unwrap();
    store
        .insert_decision(&NewDecision {
            model: "model".into(),
            purpose: "loop_guard".into(),
            questions: serde_json::json!({}),
            answers: serde_json::json!({"ok": true}),
            cost_usd: 0.1,
            latency_ms: 10,
            shadow: false,
        })
        .unwrap();
    use tkm_core::model::{RepoKey, Timestamp, Usage};
    use tkm_store::{FactRecord, FactState, LedgerRecord, NewEvent, NewSession, Repo, SessionKey};
    let repo = RepoKey("example/repo".into());
    store
        .upsert_repo(&Repo {
            repo_key: repo.clone(),
            display_name: "repo".into(),
            first_seen: Timestamp(1),
            last_seen: Timestamp(2),
            delegated_runs: 0,
        })
        .unwrap();
    let session = store
        .upsert_session(&NewSession {
            key: SessionKey {
                host: Host::Claude,
                agent_home: "home".into(),
                session_id: "s".into(),
            },
            repo_key: repo.clone(),
            cwd: dir.path().into(),
            started: Timestamp(1),
            ended: None,
            fold_status: "open".into(),
            shadow: false,
            integration_level: "full".into(),
        })
        .unwrap();
    store
        .upsert_ledger(&LedgerRecord {
            session_id: session,
            usage: Usage::default(),
            priced_cost: 1.25,
            overhead_tokens: 0,
            overhead_cost: 0.0,
        })
        .unwrap();
    store
        .insert_candidate_fact(&FactRecord {
            repo_key: repo,
            fact_id: "f".into(),
            family: "boundary".into(),
            scope: "neutral".into(),
            text_params: serde_json::json!({}),
            state: FactState::Candidate,
            score: 1.5,
            confirmations: 1,
            contradictions: 0,
            fire_count: 0,
            fire_sessions: 0,
        })
        .unwrap();
    let event_id = store.insert_event(&NewEvent { kind: "loop.redirect".into(), session_id: session, inputs: serde_json::json!({"shadow": false}), evidence_ids: vec![], explanation_template_id: "redirect".into(), params: serde_json::json!({"event":"event", "detail":"try again", "why":"reason", "model_checks":"none", "cost":"0", "undo":"none"}) }).unwrap();
    let conn = tkm_store::open_read_only(&path).unwrap();
    let state = tkm_ui::model::load_snapshot(&conn).unwrap();
    assert_eq!(state.sessions.len(), 1);
    assert_eq!(state.session_event_ids, vec![Some(event_id)]);
    assert_eq!(state.redirect_event_ids[0], Some(event_id));
    assert_eq!(state.savings[0].cost, 1.25);
    assert_eq!(state.facts[0].score, Some(1.5));
    use tkm_ui::tui::WhySource;
    let why = tkm_ui::tui::DatabaseWhy(&conn)
        .explain(&state, Pane::Sessions, 0)
        .unwrap();
    assert!(why.contains("tokenimum redirected the agent"));
    assert_eq!(state.decisions.len(), 1);
    assert_eq!(state.decisions[0].purpose, "loop_guard");
    assert_eq!(state.decisions[0].cost_usd, 0.1);
}
