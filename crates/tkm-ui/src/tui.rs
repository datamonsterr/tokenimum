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
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
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
    /// Whether an overflow follow-up snapshot is pending.
    pub resyncing: bool,
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
            resyncing: false,
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
/// One-line explanation shown in place of an empty pane.
fn empty_message(state: &MonitorState, pane: Pane) -> String {
    match pane {
        Pane::Sessions => "No active sessions. Start an agent session in a repository where `tkm init` installed hooks.".into(),
        Pane::Decisions => match &state.decisions_unavailable {
            Some(_) => "No decisions yet. Decisions are off: set decide.enabled = true and configure an OpenRouter key (tkm models probe).".into(),
            None => "No decisions yet.".into(),
        },
        Pane::Spend => "No decisions yet; per-model share appears here.".into(),
        Pane::Savings => "No priced sessions yet. Savings compare session cost per repository once sessions end with a known price.".into(),
        Pane::Facts => "No facts yet; facts are learned from decisions over several sessions.".into(),
        Pane::Redirects => "No redirects yet. Redirects appear when the loop guard steers an agent (loop.mode is shadow or on).".into(),
    }
}
fn rows(state: &MonitorState, pane: Pane) -> Vec<String> {
    match pane {
        Pane::Sessions => state
            .sessions
            .iter()
            .map(|s| {
                format!(
                    "{}  {}  {}  turn {}  calls {} / {}  {}",
                    s.host.as_str(),
                    s.session_id,
                    s.repo.as_deref().unwrap_or("-"),
                    s.turn_id
                        .map_or_else(|| "-".into(), |turn| turn.to_string()),
                    s.turn_calls,
                    envelope_cell(s),
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
        Pane::Spend => spend_rows(state),
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
                    "{}  {}  {}  {}  {}  calls [{}]",
                    r.host.as_str(),
                    r.session_id,
                    r.kind,
                    if r.shadow { "shadow" } else { "acted" },
                    r.text,
                    r.triggering_calls.join(", ")
                )
            })
            .collect(),
    }
}
fn envelope_cell(session: &tkm_proto::monitor::SessionUpdate) -> String {
    match (
        session.envelope,
        session.envelope_samples,
        session.envelope_min_samples,
    ) {
        (Some(envelope), _, _) => format!("{envelope:.1}"),
        (None, Some(samples), Some(min)) if samples < min => format!("{samples}/{min} samples"),
        _ => "-".into(),
    }
}
fn money(usd: f64) -> String {
    if usd == 0.0 {
        "$0.00".into()
    } else {
        format!("${usd:.6}")
    }
}
fn spend_rows(state: &MonitorState) -> Vec<String> {
    let mut rows = vec![state.spend.as_ref().map_or_else(
        || "Today: unknown (caps not loaded)".into(),
        |spend| {
            format!(
                "Today: {} / ${:.2} daily cap; session cap ${:.2}",
                money(spend.today_usd),
                spend.daily_cap_usd,
                spend.session_cap_usd
            )
        },
    )];
    if state.unknown_decision_costs > 0 {
        rows.push(format!(
            "Decision cost unknown: {} attempts",
            state.unknown_decision_costs
        ));
    }
    let mut models = std::collections::BTreeMap::<&str, f64>::new();
    for decision in &state.decisions {
        *models.entry(&decision.model).or_default() += decision.cost_usd;
    }
    let total: f64 = models.values().sum();
    if models.is_empty() {
        rows.push(empty_message(state, Pane::Spend));
    }
    rows.extend(models.into_iter().map(|(model, cost)| {
        format!(
            "{model}: ${cost:.6} ({:.1}% of observed decisions)",
            cost * 100.0 / total.max(f64::EPSILON)
        )
    }));
    rows.push("7-day spend".into());
    rows
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
    let mut rows: Vec<String> = groups
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
        .collect();
    if state.unknown_session_costs > 0 {
        rows.push(format!(
            "Session cost unknown for {} sessions",
            state.unknown_session_costs
        ));
    }
    if rows.is_empty() {
        rows.push(empty_message(state, Pane::Savings));
    }
    rows
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
    if view.pane == Pane::Spend {
        render_spend(state, areas[1], frame);
    } else if len(state, view.pane) == 0 && rows(state, view.pane).is_empty() {
        frame.render_widget(
            Paragraph::new(empty_message(state, view.pane))
                .block(Block::bordered().title(view.pane.name()))
                .wrap(Wrap { trim: true }),
            areas[1],
        );
    } else {
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
    }
    let status = match view.mode {
        Mode::Live => "live",
        Mode::ReadOnly => "daemon down: read-only, no live feed",
    };
    frame.render_widget(
        Paragraph::new(format!(
            "{status}{} | last: {} | ? help",
            if view.resyncing { " | resyncing" } else { "" },
            view.last_message.map_or_else(
                || "-".into(),
                |t| format!(
                    "{:02}:{:02}:{:02} UTC",
                    t % 86_400 / 3_600,
                    t % 3_600 / 60,
                    t % 60
                )
            )
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
fn render_spend(state: &MonitorState, area: ratatui::layout::Rect, frame: &mut Frame<'_>) {
    use ratatui::{
        layout::Constraint,
        widgets::{Paragraph, Sparkline},
    };
    let inner = Block::bordered().title("Spend").inner(area);
    frame.render_widget(Block::bordered().title("Spend"), area);
    let split = Layout::vertical([Constraint::Length(3), Constraint::Min(1)]).split(inner);
    let labels = spend_rows(state);
    frame.render_widget(
        Paragraph::new(labels.into_iter().take(3).collect::<Vec<_>>().join("\n")),
        split[0],
    );
    let mut data = vec![0; 7usize.saturating_sub(state.spend_days.len().min(7))];
    data.extend(
        state
            .spend_days
            .iter()
            .rev()
            .take(7)
            .rev()
            .map(|(_, cost)| spark_height(*cost)),
    );
    frame.render_widget(Sparkline::default().data(&data), split[1]);
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
fn subscribe_then_snapshot<F, S, E>(
    subscribe: impl FnOnce() -> F,
    snapshot: impl FnOnce() -> Result<S, E>,
) -> Result<(F, S), E> {
    let feed = subscribe();
    let state = snapshot()?;
    Ok((feed, state))
}
fn resync_snapshot<E>(
    feed: &feed::Feed,
    load: impl FnOnce() -> Result<MonitorState, E>,
) -> Result<(MonitorState, u64), E> {
    let watermark = feed.sequence.load(std::sync::atomic::Ordering::Acquire);
    Ok((load()?, watermark))
}
/// ADR 0015 flushes the outbox every five seconds; allow one extra second.
const RESYNC_FOLLOWUP_DELAY: Duration = Duration::from_secs(6);

fn tick_resync<E>(
    feed: &feed::Feed,
    state: &mut MonitorState,
    watermark: &mut u64,
    deadline: &mut Option<Instant>,
    mut now: impl FnMut() -> Instant,
    mut load: impl FnMut() -> Result<MonitorState, E>,
) -> Result<(), E> {
    if feed.resync.swap(false, std::sync::atomic::Ordering::AcqRel) {
        (*state, *watermark) = resync_snapshot(feed, &mut load)?;
        *deadline = Some(now() + RESYNC_FOLLOWUP_DELAY);
    } else if deadline.is_some_and(|at| now() >= at) {
        *state = load()?;
        *deadline = None;
    }
    Ok(())
}
fn apply_queued<E>(
    state: &mut MonitorState,
    sequence: u64,
    watermark: u64,
    message: tkm_proto::monitor::MonitorMessage,
    load: impl FnOnce() -> Result<(Vec<model::Savings>, Vec<(String, f64)>), E>,
) -> Result<bool, E> {
    if sequence <= watermark {
        return Ok(false);
    }
    state.apply_with_loader(message, load)?;
    Ok(true)
}
/// Global settings the snapshot needs; an unreadable configuration leaves them unknown.
/// Why decisions cannot run: switched off, or no OpenRouter key where the daemon looks for one.
fn decisions_unavailable(config: &tkm_config::config::EffectiveConfig) -> Option<String> {
    if config.get_bool("decide.enabled") == Some(false) {
        return Some("decide.enabled is false".into());
    }
    if std::env::var("OPENROUTER_API_KEY").is_ok_and(|key| !key.trim().is_empty()) {
        return None;
    }
    let home = std::env::var_os("HOME").map(std::path::PathBuf::from);
    let path = config
        .get_str("decide.api_key_path")
        .map(|path| match (path.strip_prefix("~/"), &home) {
            (Some(rest), Some(home)) => home.join(rest),
            _ => std::path::PathBuf::from(path),
        })
        .or_else(|| {
            home.as_ref()
                .map(|home| home.join(".config/tokenimum/openrouter.env"))
        })?;
    (!path.exists()).then(|| "no OpenRouter key".into())
}
fn snapshot_config(paths: &Paths) -> model::SnapshotConfig {
    use tkm_config::config::{Layer, LoadInputs, load};
    let Ok(config) = load(&LoadInputs::from_env(
        paths.global_config.clone(),
        None,
        Vec::new(),
    )) else {
        return model::SnapshotConfig::default();
    };
    let explicit = |key: &str| {
        config
            .source(key)
            .is_some_and(|layer| layer != Layer::Default)
    };
    let mut loop_cfg =
        tkm_core::loopguard::LoopConfig::preset(config.get_str("loop.level").unwrap_or("default"));
    if explicit("loop.envelope_min_samples")
        && let Some(n) = config
            .get_int("loop.envelope_min_samples")
            .and_then(|n| usize::try_from(n).ok())
    {
        loop_cfg.envelope_min_samples = n;
    }
    if explicit("loop.envelope_margin")
        && let Some(margin) = config.get_float("loop.envelope_margin")
    {
        loop_cfg.envelope_margin = margin;
    }
    if explicit("loop.envelope_pct")
        && let Some(pct) = config
            .get_int("loop.envelope_pct")
            .and_then(|n| u32::try_from(n).ok())
    {
        loop_cfg.envelope_pct = f64::from(pct);
    }
    model::SnapshotConfig {
        caps: config
            .get_float("decide.budget_daily_usd")
            .zip(config.get_float("decide.budget_session_usd")),
        loop_mode: config.get_str("loop.mode").map(str::to_owned),
        loop_cfg,
        now_ms: None,
        decisions_unavailable: decisions_unavailable(&config),
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
    let config = snapshot_config(paths);
    let snapshot = |conn: &Connection| {
        let config = model::SnapshotConfig {
            now_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .ok()
                .and_then(|d| i64::try_from(d.as_millis()).ok()),
            ..config.clone()
        };
        model::load_snapshot_with(conn, &config)
    };
    let (feed, (conn, mut state)) = subscribe_then_snapshot(
        || feed::connect(paths),
        || {
            let conn = tkm_store::open_read_only(&paths.db)?;
            let snapshot = snapshot(&conn)?;
            Ok::<_, Error>((conn, snapshot))
        },
    )?;
    let mut view = ViewState {
        mode: feed.mode,
        ..ViewState::default()
    };
    let mut watermark = 0;
    let mut deadline = None;
    let mut terminal = ratatui::init();
    let result = (|| -> Result<(), Error> {
        loop {
            tick_resync(
                &feed,
                &mut state,
                &mut watermark,
                &mut deadline,
                Instant::now,
                || snapshot(&conn),
            )?;
            view.resyncing = deadline.is_some();
            loop {
                match feed.receiver.try_recv() {
                    Ok((seq, message)) => {
                        if apply_queued(&mut state, seq, watermark, message, || {
                            let snapshot = snapshot(&conn)?;
                            Ok::<_, Error>((snapshot.savings, snapshot.spend_days))
                        })? {
                            view.last_message = Some(
                                SystemTime::now()
                                    .duration_since(UNIX_EPOCH)
                                    .unwrap_or_default()
                                    .as_secs(),
                            );
                        }
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
                    Action::Reload => state = snapshot(&conn)?,
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

/// Whole millionths of a dollar for a sparkline bar; negative or non-finite costs draw as zero.
///
/// Decision costs are fractions of a cent, so thousandths would round real spend to nothing.
/// `Sparkline` scales bars to the tallest value, and an all-zero week draws no bars.
#[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
fn spark_height(cost: f64) -> u64 {
    let micro = (cost * 1_000_000.0).round();
    if micro.is_finite() && micro > 0.0 {
        // Clamped below 2^53, so the conversion is exact and cannot lose sign.
        micro.min(9_007_199_254_740_991.0) as u64
    } else {
        0
    }
}

#[cfg(test)]
mod ordering_tests {
    use super::*;
    use std::{cell::RefCell, rc::Rc};
    fn test_feed(
        overflow: bool,
    ) -> (
        feed::Feed,
        std::sync::mpsc::Sender<(u64, tkm_proto::monitor::MonitorMessage)>,
    ) {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, AtomicU64},
            mpsc,
        };
        let (sender, receiver) = mpsc::channel();
        (
            feed::Feed {
                mode: Mode::Live,
                receiver,
                resync: Arc::new(AtomicBool::new(overflow)),
                sequence: Arc::new(AtomicU64::new(1)),
            },
            sender,
        )
    }

    fn new_session() -> tkm_proto::monitor::MonitorMessage {
        tkm_proto::monitor::MonitorMessage::Session(tkm_proto::monitor::SessionUpdate {
            host: tkm_proto::Host::Claude,
            session_id: "new".into(),
            turn_id: None,
            repo: None,
            phase: "active".into(),
            turn_calls: 1,
            envelope: None,
            envelope_samples: None,
            envelope_min_samples: None,
            loop_state: "off".into(),
        })
    }

    #[test]
    fn overflow_followup_loads_previously_unflushed_session() -> Result<(), ()> {
        let (feed, sender) = test_feed(true);
        sender.send((1, new_session())).map_err(|_| ())?;
        let start = std::time::Instant::now();
        let mut deadline = None;
        let mut watermark = 0;
        let mut state = MonitorState::default();
        let mut calls = 0;
        let mut load = || {
            calls += 1;
            let mut snapshot = MonitorState::default();
            if calls > 1 {
                snapshot.apply(new_session());
            }
            Ok::<_, ()>(snapshot)
        };
        tick_resync(
            &feed,
            &mut state,
            &mut watermark,
            &mut deadline,
            || start,
            &mut load,
        )?;
        let (seq, message) = feed.receiver.try_recv().map_err(|_| ())?;
        assert!(!apply_queued(
            &mut state,
            seq,
            watermark,
            message,
            || Ok::<_, ()>((vec![], vec![]))
        )?);
        assert_eq!(state.sessions.len(), 0);
        tick_resync(
            &feed,
            &mut state,
            &mut watermark,
            &mut deadline,
            || start + Duration::from_secs(5),
            &mut load,
        )?;
        assert_eq!(state.sessions.len(), 0);
        tick_resync(
            &feed,
            &mut state,
            &mut watermark,
            &mut deadline,
            || start + Duration::from_secs(6),
            &mut load,
        )?;
        assert_eq!(state.sessions[0].session_id, "new");
        assert!(deadline.is_none());
        tick_resync(
            &feed,
            &mut state,
            &mut watermark,
            &mut deadline,
            || start + Duration::from_secs(7),
            &mut load,
        )?;
        assert_eq!(calls, 2);
        Ok(())
    }

    #[test]
    fn no_overflow_does_not_schedule_followup() -> Result<(), ()> {
        let (feed, _sender) = test_feed(false);
        let mut state = MonitorState::default();
        let mut watermark = 0;
        let mut deadline = None;
        let mut calls = 0;
        let start = std::time::Instant::now();
        tick_resync(
            &feed,
            &mut state,
            &mut watermark,
            &mut deadline,
            || start + Duration::from_secs(10),
            &mut || {
                calls += 1;
                Ok::<_, ()>(MonitorState::default())
            },
        )?;
        assert_eq!(calls, 0);
        assert!(deadline.is_none());
        Ok(())
    }

    #[test]
    fn status_line_shows_resyncing_while_followup_pending() -> Result<(), Box<dyn std::error::Error>>
    {
        let backend = ratatui::backend::TestBackend::new(80, 5);
        let mut terminal = ratatui::Terminal::new(backend)?;
        let view = ViewState {
            resyncing: true,
            ..ViewState::default()
        };
        terminal.draw(|frame| render(&MonitorState::default(), &view, frame))?;
        let buffer = terminal.backend().buffer();
        let line: String = (0..80).map(|x| buffer[(x, 4)].symbol()).collect();
        assert!(line.contains("resyncing"));
        Ok(())
    }
    #[test]
    fn overflow_does_not_replay_session_before_snapshot() -> Result<(), ()> {
        use std::sync::{
            Arc,
            atomic::{AtomicBool, AtomicU64},
            mpsc,
        };
        let (sender, receiver) = mpsc::channel();
        sender
            .send((
                1,
                tkm_proto::monitor::MonitorMessage::Session(tkm_proto::monitor::SessionUpdate {
                    host: tkm_proto::Host::Claude,
                    session_id: "ended".into(),
                    turn_id: None,
                    repo: None,
                    phase: "active".into(),
                    turn_calls: 1,
                    envelope: None,
                    envelope_samples: None,
                    envelope_min_samples: None,
                    loop_state: "off".into(),
                }),
            ))
            .map_err(|_| ())?;
        let feed = feed::Feed {
            mode: Mode::Live,
            receiver,
            resync: Arc::new(AtomicBool::new(true)),
            sequence: Arc::new(AtomicU64::new(1)),
        };
        let (mut state, watermark) = resync_snapshot(&feed, || {
            sender
                .send((
                    2,
                    tkm_proto::monitor::MonitorMessage::Session(
                        tkm_proto::monitor::SessionUpdate {
                            host: tkm_proto::Host::Claude,
                            session_id: "new".into(),
                            turn_id: None,
                            repo: None,
                            phase: "active".into(),
                            turn_calls: 1,
                            envelope: None,
                            envelope_samples: None,
                            envelope_min_samples: None,
                            loop_state: "off".into(),
                        },
                    ),
                ))
                .map_err(|_| ())?;
            Ok::<_, ()>(MonitorState::default())
        })?;
        while let Ok((seq, message)) = feed.receiver.try_recv() {
            apply_queued(&mut state, seq, watermark, message, || {
                Ok::<_, ()>((Vec::new(), Vec::new()))
            })?;
        }
        assert_eq!(state.sessions.len(), 1);
        assert_eq!(state.sessions[0].session_id, "new");
        Ok(())
    }
    #[test]
    fn subscribe_precedes_snapshot_and_buffered_messages_apply_afterward() -> Result<(), ()> {
        let trace = Rc::new(RefCell::new(Vec::new()));
        let (buffer, mut snapshot) = subscribe_then_snapshot(
            || {
                trace.borrow_mut().push("subscribe");
                vec![tkm_proto::monitor::MonitorMessage::Session(
                    tkm_proto::monitor::SessionUpdate {
                        host: tkm_proto::Host::Claude,
                        session_id: "session".into(),
                        turn_id: None,
                        repo: None,
                        phase: "active".into(),
                        turn_calls: 2,
                        envelope: None,
                        envelope_samples: None,
                        envelope_min_samples: None,
                        loop_state: "off".into(),
                    },
                )]
            },
            || {
                trace.borrow_mut().push("snapshot");
                Ok::<_, ()>(MonitorState::default())
            },
        )?;
        for message in buffer {
            snapshot.apply(message);
        }
        assert_eq!(*trace.borrow(), ["subscribe", "snapshot"]);
        assert_eq!(
            snapshot.sessions.first().map(|session| session.turn_calls),
            Some(2)
        );
        Ok(())
    }
}
