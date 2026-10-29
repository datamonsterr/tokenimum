# E8 UI API verification

Checked 2026-10-29:

- [Ratatui 0.30.2 initialization and panic restoration](https://docs.rs/ratatui/latest/ratatui/init/index.html), [Terminal and Frame drawing](https://docs.rs/ratatui/latest/ratatui/struct.Terminal.html), [TestBackend](https://docs.rs/ratatui/latest/ratatui/backend/struct.TestBackend.html), [Tabs](https://docs.rs/ratatui/latest/ratatui/widgets/struct.Tabs.html), [List](https://docs.rs/ratatui/latest/ratatui/widgets/struct.List.html). Versioned URLs returned errors from docs.rs; latest was 0.30.2 on this date, also checked the locally cached 0.30.2 source.
- [Crossterm 0.29.0 event polling](https://docs.rs/crossterm/0.29.0/crossterm/event/), [KeyEvent](https://docs.rs/crossterm/latest/crossterm/event/struct.KeyEvent.html).
- [Criterion 0.8.2](https://docs.rs/criterion/0.8.2/criterion/) and its criterion_group/criterion_main macros.
- [rusqlite Connection](https://docs.rs/rusqlite/latest/rusqlite/struct.Connection.html); the locally cached 0.41.1 source was also consulted because docs.rs latest reports 0.41.0.

## Known gaps

- Decisions have no timestamps, so read-only daily spend, model shares and a seven-day trend cannot be reconstructed accurately. Live spend is available from SpendUpdate.
- Fact notices omit scores; the snapshot score is retained, and newly noticed facts have an unknown score until reload.
- Live rows have no event IDs. Their why overlay asks for a reload; stored redirects are associated directly with their event IDs.
- The current stream has no turn number, triggering call list, per-model spend breakdown, or ledger updates. Savings shows recent ledger costs and the most recent shadow baseline with sample counts; a statistical baseline estimate is not calculated.
- Persisted session rows use their latest stored event and redirect rows use their own event IDs. Decisions have stored IDs but the schema contains no link from a decision to an event; other row types show the no-event message.
- The pane snapshots use fixed populated fixtures; the database-backed why test exercises a session event.
