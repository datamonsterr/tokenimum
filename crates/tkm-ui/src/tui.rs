//! Pure terminal rendering and keyboard handling, plus the terminal event loop.
use crate::{
    Error,
    feed::{self, Mode},
    model::{self, MonitorState},
};
use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::{
    Frame,
    layout::{Constraint, Layout},
    widgets::{Block, Clear, List, ListState, Paragraph, Tabs, Wrap},
};
use rusqlite::{Connection, OptionalExtension};
use std::{
    sync::mpsc::TryRecvError,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tkm_config::paths::Paths;

/// A monitor pane, in specification order.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    /// Sessions.
    #[default]
    Sessions,
    /// Decisions.
    Decisions,
    /// Spend.
    Spend,
    /// Savings.
    Savings,
    /// Facts.
    Facts,
    /// Redirects.
    Redirects,
}
impl Pane {
    /// All panes in tab order.
    pub const ALL: [Self; 6] = [
        Self::Sessions,
        Self::Decisions,
        Self::Spend,
        Self::Savings,
        Self::Facts,
        Self::Redirects,
    ];
    const fn index(self) -> usize {
        match self {
            Self::Sessions => 0,
            Self::Decisions => 1,
            Self::Spend => 2,
            Self::Savings => 3,
            Self::Facts => 4,
            Self::Redirects => 5,
        }
    }
    const fn name(self) -> &'static str {
        match self {
            Self::Sessions => "Sessions",
            Self::Decisions => "Decisions",
            Self::Spend => "Spend",
            Self::Savings => "Savings",
            Self::Facts => "Facts",
            Self::Redirects => "Redirects",
        }
    }
}
/// State local to a terminal view.
#[derive(Debug, Clone)]
pub struct ViewState {
    /// Active pane.
    pub pane: Pane,
    /// Selection per pane.
    pub selection: [usize; 6],
    /// Whether help is open.
    pub help: bool,
    /// Explanation text, if open.
    pub overlay: Option<String>,
    /// Connection mode.
    pub mode: Mode,
    /// Last message time in Unix seconds, set by the event loop.
    pub last_message: Option<u64>,
}
impl Default for ViewState {
    fn default() -> Self {
        Self {
            pane: Pane::Sessions,
            selection: [0; 6],
            help: false,
            overlay: None,
            mode: Mode::ReadOnly,
            last_message: None,
        }
    }
}
/// An action requested by keyboard handling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Nothing further is needed.
    None,
    /// Exit the monitor.
    Quit,
    /// Reload the snapshot.
    Reload,
    /// Explain the selected row.
    Explain,
}
fn len(state: &MonitorState, pane: Pane) -> usize {
    match pane {
        Pane::Sessions => state.sessions.len(),
        Pane::Decisions => state.decisions.len(),
        Pane::Spend => 1,
        Pane::Savings => state
            .savings
            .iter()
            .map(|s| (&s.repo, &s.host))
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        Pane::Facts => state.facts.len(),
        Pane::Redirects => state.redirects.len(),
    }
}
/// Interpret a key without I/O.
#[must_use]
pub fn handle_key(view: &mut ViewState, key: KeyEvent, state: &MonitorState) -> Action {
    if key.kind == KeyEventKind::Release {
        return Action::None;
    }
    if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
        return Action::Quit;
    }
    if key.code == KeyCode::Esc {
        view.overlay = None;
        view.help = false;
        return Action::None;
    }
    if key.code == KeyCode::Char('q') {
        return Action::Quit;
    }
    if key.code == KeyCode::Char('?') {
        view.help = !view.help;
        return Action::None;
    }
    if view.help || view.overlay.is_some() {
        return Action::None;
    }
    match key.code {
        KeyCode::BackTab => view.pane = Pane::ALL[(view.pane.index() + 5) % 6],
        KeyCode::Tab if key.modifiers.contains(KeyModifiers::SHIFT) => {
            view.pane = Pane::ALL[(view.pane.index() + 5) % 6];
        }
        KeyCode::Tab => view.pane = Pane::ALL[(view.pane.index() + 1) % 6],
        KeyCode::Char(c @ '1'..='6') => {
            if let Some(index) = c.to_digit(10).and_then(|v| usize::try_from(v).ok()) {
                view.pane = Pane::ALL[index - 1];
            }
        }
        KeyCode::Char('j') | KeyCode::Down => {
            let index = view.pane.index();
            view.selection[index] =
                (view.selection[index] + 1).min(len(state, view.pane).saturating_sub(1));
        }
        KeyCode::Char('k') | KeyCode::Up => {
            let index = view.pane.index();
            view.selection[index] = view.selection[index].saturating_sub(1);
        }
        KeyCode::Char('r') => return Action::Reload,
        KeyCode::Enter => return Action::Explain,
        _ => {}
    }
    Action::None
}
fn rows(state: &MonitorState, pane: Pane) -> Vec<String> {
    match pane {
        Pane::Sessions => state
            .sessions
            .iter()
            .map(|s| {
                format!(
                    "{}  {}  {}  calls {} / {}  {}",
                    s.host.as_str(),
                    s.session_id,
                    s.repo.as_deref().unwrap_or("-"),
                    s.turn_calls,
                    s.envelope.map_or_else(|| "-".into(), |e| format!("{e:.1}")),
                    s.loop_state
                )
            })
            .collect(),
        Pane::Decisions => state
            .decisions
            .iter()
            .map(|d| {
                format!(
                    "{}  {}  {}  {} ms  ${:.6}  {}",
                    d.purpose,
                    d.model,
                    d.answers,
                    d.latency_ms,
                    d.cost_usd,
                    if d.shadow { "shadow" } else { "acted" }
                )
            })
            .collect(),
        Pane::Spend => vec![state.spend.as_ref().map_or_else(
            || "Today: n/a (no timestamps); model share and 7-day trend unavailable".into(),
            |s| {
                format!(
                    "Today: ${:.6} / ${:.2}  session cap ${:.2}; 7-day trend n/a (no timestamps)",
                    s.today_usd, s.daily_cap_usd, s.session_cap_usd
                )
            },
        )],
        Pane::Savings => savings_rows(state),
        Pane::Facts => state
            .facts
            .iter()
            .map(|f| {
                format!(
                    "{}  {}  {}  score {}  confirmations {}",
                    f.repo,
                    f.fact_id,
                    f.state,
                    f.score.map_or_else(|| "-".into(), |s| format!("{s:.2}")),
                    f.confirmations
                        .map_or_else(|| "-".into(), |n| n.to_string())
                )
            })
            .collect(),
        Pane::Redirects => state
            .redirects
            .iter()
            .map(|r| {
                format!(
                    "{}  {}  {}  {}  {}",
                    r.host.as_str(),
                    r.session_id,
                    r.kind,
                    if r.shadow { "shadow" } else { "acted" },
                    r.text
                )
            })
            .collect(),
    }
}
fn savings_rows(state: &MonitorState) -> Vec<String> {
    let mut groups = std::collections::BTreeMap::<(&str, &str), (Vec<f64>, Vec<f64>)>::new();
    for sample in &state.savings {
        let group = groups.entry((&sample.repo, &sample.host)).or_default();
        if sample.shadow {
            group.1.push(sample.cost);
        } else {
            group.0.push(sample.cost);
        }
    }
    groups
        .into_iter()
        .map(|((repo, host), (observed, baseline))| {
            let trend = observed
                .iter()
                .rev()
                .take(5)
                .rev()
                .map(|v| format!("{v:.2}"))
                .collect::<Vec<_>>()
                .join(",");
            let base = baseline
                .last()
                .map_or_else(|| "-".to_owned(), |v| format!("{v:.2}"));
            format!(
                "{repo}  {host}  recent [{trend}] n={}  baseline {base} n={}",
                observed.len(),
                baseline.len()
            )
        })
        .collect()
}
/// Render the entire screen without side effects or access to a clock.
pub fn render(state: &MonitorState, view: &ViewState, frame: &mut Frame<'_>) {
    let areas = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .split(frame.area());
    frame.render_widget(
        Tabs::new(Pane::ALL.map(Pane::name)).select(view.pane.index()),
        areas[0],
    );
    let list = List::new(rows(state, view.pane))
        .block(Block::bordered().title(view.pane.name()))
        .highlight_symbol("> ");
    let mut selection = ListState::default();
    if len(state, view.pane) > 0 {
        selection.select(Some(
            view.selection[view.pane.index()].min(len(state, view.pane) - 1),
        ));
    }
    frame.render_stateful_widget(list, areas[1], &mut selection);
    let status = match view.mode {
        Mode::Live => "live",
        Mode::ReadOnly => "daemon down: read-only, no live feed",
    };
    frame.render_widget(
        Paragraph::new(format!(
            "{status} | last: {} | ? help",
            view.last_message
                .map_or_else(|| "-".into(), |t| t.to_string())
        )),
        areas[2],
    );
    let text = if view.help {
        Some(
            "Tab / Shift-Tab or 1-6: pane\nj/k or arrows: select\nEnter: why  Esc: close\nr: reload  ?: help  q / Ctrl-C: quit",
        )
    } else {
        view.overlay.as_deref()
    };
    if let Some(text) = text {
        frame.render_widget(Clear, areas[1]);
        frame.render_widget(
            Paragraph::new(text)
                .block(Block::bordered().title(if view.help { "Help" } else { "Why" }))
                .wrap(Wrap { trim: false }),
            areas[1],
        );
    }
}
/// Provides the explanation for a selected row.
pub trait WhySource {
    /// Return an explanation, or a message indicating that no event exists.
    ///
    /// # Errors
    /// Returns an error if the stored event cannot be read or decoded.
    fn explain(&self, state: &MonitorState, pane: Pane, index: usize) -> Result<String, Error>;
}
/// Read explanations from a read-only SQLite connection.
pub struct DatabaseWhy<'a>(
    /// Connection to the store.
    pub &'a Connection,
);
impl WhySource for DatabaseWhy<'_> {
    fn explain(&self, state: &MonitorState, pane: Pane, index: usize) -> Result<String, Error> {
        let id = match pane {
            Pane::Redirects => state.redirect_event_ids.get(index).copied().flatten(),
            Pane::Sessions => state.session_event_ids.get(index).copied().flatten(),
            _ => None,
        };
        let Some(id) = id else {
            return Ok("no stored event for this row yet; press r to reload".into());
        };
        let row: Option<(String, String)> = self
            .0
            .query_row(
                "SELECT explanation_template_id, params FROM events WHERE id=?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let Some((kind, params)) = row else {
            return Ok("no stored event for this row yet; press r to reload".into());
        };
        let value: serde_json::Value = serde_json::from_str(&params)?;
        let wrapped =
            serde_json::json!({"template_id": kind, "params": {"kind": kind, "values": value}});
        let explanation: tkm_core::explain::Explanation = serde_json::from_value(wrapped)?;
        Ok(tkm_core::explain::render(&explanation))
    }
}
/// Run the terminal monitor against one initial snapshot and the live stream.
///
/// # Errors
/// Returns an error if the database or terminal cannot be accessed.
///
/// # Panics
/// Panics if terminal initialization fails, as specified by `ratatui::init`.
pub fn run(paths: &Paths) -> Result<(), Error> {
    let feed = feed::connect(paths);
    let conn = tkm_store::open_read_only(&paths.db)?;
    let mut state = model::load_snapshot(&conn)?;
    let mut view = ViewState {
        mode: feed.mode,
        ..ViewState::default()
    };
    let mut terminal = ratatui::init();
    let result = (|| -> Result<(), Error> {
        loop {
            if feed.resync.swap(false, std::sync::atomic::Ordering::AcqRel) {
                state = model::load_snapshot(&conn)?;
            }
            loop {
                match feed.receiver.try_recv() {
                    Ok(message) => {
                        if matches!(&message, tkm_proto::monitor::MonitorMessage::Session(update) if update.phase == "folded")
                        {
                            state.savings = model::load_snapshot(&conn)?.savings;
                        }
                        state.apply(message);
                        view.last_message = Some(
                            SystemTime::now()
                                .duration_since(UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs(),
                        );
                    }
                    Err(TryRecvError::Empty) => break,
                    Err(TryRecvError::Disconnected) => {
                        view.mode = Mode::ReadOnly;
                        break;
                    }
                }
            }
            terminal.draw(|frame| render(&state, &view, frame))?;
            if event::poll(Duration::from_millis(100))?
                && let Event::Key(key) = event::read()?
            {
                match handle_key(&mut view, key, &state) {
                    Action::Quit => break,
                    Action::Reload => state = model::load_snapshot(&conn)?,
                    Action::Explain => {
                        view.overlay = Some(DatabaseWhy(&conn).explain(
                            &state,
                            view.pane,
                            view.selection[view.pane.index()],
                        )?);
                    }
                    Action::None => {}
                }
            }
        }
        Ok(())
    })();
    ratatui::restore();
    result
}
