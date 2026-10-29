# E8 UI API verification

Checked 2026-10-29:

- [Ratatui 0.30.2 initialization and panic restoration](https://docs.rs/ratatui/latest/ratatui/init/index.html), [Terminal and Frame drawing](https://docs.rs/ratatui/latest/ratatui/struct.Terminal.html), [TestBackend](https://docs.rs/ratatui/latest/ratatui/backend/struct.TestBackend.html), [Tabs](https://docs.rs/ratatui/latest/ratatui/widgets/struct.Tabs.html), [List](https://docs.rs/ratatui/latest/ratatui/widgets/struct.List.html). Versioned URLs returned errors from docs.rs; latest was 0.30.2 on this date, also checked the locally cached 0.30.2 source.
- [Crossterm 0.29.0 event polling](https://docs.rs/crossterm/0.29.0/crossterm/event/), [KeyEvent](https://docs.rs/crossterm/latest/crossterm/event/struct.KeyEvent.html).
- [Criterion 0.8.2](https://docs.rs/criterion/0.8.2/criterion/) and its criterion_group/criterion_main macros.
- [rusqlite Connection](https://docs.rs/rusqlite/latest/rusqlite/struct.Connection.html); the locally cached 0.41.1 source was also consulted because docs.rs latest reports 0.41.0.

## Known gaps

- Decisions have no timestamps, so read-only decision spend and model shares cannot be reconstructed accurately. Live decision spend is available from SpendUpdate; the seven-day ledger trend uses session start times.
- Fact notices omit scores; the snapshot score is retained, and newly noticed facts have an unknown score until reload.
- Live rows have no event IDs. Their why overlay asks for a reload; stored redirects are associated directly with their event IDs.
- The current stream has no turn number, triggering call list, per-model spend breakdown, or ledger updates. Savings shows recent ledger costs and the most recent shadow baseline with sample counts; a statistical baseline estimate is not calculated.
- Persisted session rows use their latest stored event and redirect rows use their own event IDs. Decisions have stored IDs but the schema contains no link from a decision to an event; other row types show the no-event message.
- The pane snapshots use fixed populated fixtures; the database-backed why test exercises a session event.

## Web APIs checked 2026-10-29

- [axum 0.8.9 routing and handlers](https://docs.rs/axum/0.8.9/axum/), [Router and state](https://docs.rs/axum/latest/axum/routing/struct.Router.html), [stateful middleware](https://docs.rs/axum/latest/axum/middleware/fn.from_fn_with_state.html), [Path](https://docs.rs/axum/latest/axum/extract/struct.Path.html), [State](https://docs.rs/axum/latest/axum/extract/struct.State.html), [Json](https://docs.rs/axum/latest/axum/struct.Json.html), [graceful shutdown](https://docs.rs/axum/0.8.9/axum/serve/struct.Serve.html), [serve](https://docs.rs/axum/0.8.9/axum/fn.serve.html). The `latest` pages reported 0.8.9 on this date; versioned pages were inaccessible for some items.
- [axum ServiceExt](https://docs.rs/axum/latest/axum/trait.ServiceExt.html) does not provide `oneshot`; [tower 0.5.3 ServiceExt](https://docs.rs/tower/0.5.3/tower/trait.ServiceExt.html) provides it with `util`, used only by tests.
- [HTTP header types and constants](https://docs.rs/http/latest/http/header/index.html), [HTTP status codes](https://docs.rs/http/latest/http/status/struct.StatusCode.html); [Tokio TCP listener](https://docs.rs/tokio/1.54.1/tokio/net/struct.TcpListener.html), [broadcast receiver](https://docs.rs/tokio/1.54.1/tokio/sync/broadcast/struct.Receiver.html), [oneshot](https://docs.rs/tokio/1.54.1/tokio/sync/oneshot/index.html), and [spawn](https://docs.rs/tokio/1.54.1/tokio/task/fn.spawn.html).

- [rusqlite 0.41.1 Statement::column_names](https://docs.rs/rusqlite/latest/rusqlite/struct.Statement.html#method.column_names) returns an iterator; the 0.41.1 source confirms IntegralValueOutOfRange for checked integer conversion. Checked 2026-10-29.
- [Ratatui 0.30.2 buffer snapshots](https://docs.rs/ratatui/latest/ratatui/backend/struct.TestBackend.html#method.assert_buffer) compare styles as well as text; [Buffer::set_style](https://docs.rs/ratatui/latest/ratatui/buffer/struct.Buffer.html#method.set_style) sets the selected tab in expected buffers. Checked 2026-10-29.

- [Axum 0.8.9 DefaultBodyLimit](https://docs.rs/axum/0.8.9/axum/extract/struct.DefaultBodyLimit.html) applies to JSON body extraction. The versioned Tokio pages were inaccessible, so [spawn_blocking](https://docs.rs/tokio/latest/tokio/task/fn.spawn_blocking.html) and [Semaphore](https://docs.rs/tokio/latest/tokio/sync/struct.Semaphore.html) were checked via latest, which reported 1.54.1.
