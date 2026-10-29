# ADR 0035: Monitor UI architecture (`tkm top`, web dashboard)

Date: 2026-10-29. Status: accepted (epic E8).

## Context

SPEC 11.3 and 11.4 ask for a ratatui monitor over the E4.6 monitor stream (ADR 0019) and an
optional local web dashboard. Both show the same six panes: Sessions, Decisions, Spend, Savings,
Facts, Redirects. The TUI works read-only from the database when the daemon is down.

## Decisions

1. **One model, two views.** `tkm_ui::model` holds a pure `MonitorState`. It is built from a
   database snapshot (`tkm_store::open_read_only`) and then updated by applying
   `tkm_proto::monitor::MonitorMessage` values. It has no I/O and no clock reads. The TUI
   (`tkm_ui::tui`) and the web API (`tkm_ui::web`, feature `web`) both render this one type, so
   the panes cannot drift apart.
2. **Feed.** `tkm_ui::feed` connects to `tkmd-monitor.sock`, checks the `Hello` frame, and yields
   messages. When the socket is absent or the connection fails, the client runs in read-only
   mode: the snapshot only, with a visible "daemon down: read-only, no live feed" banner. Refresh
   is push-based. The database is read once at start and again only on an explicit `r` key.
3. **Crate edges.** `tkm-ui` depends on `tkm-proto`, `tkm-store`, `tkm-config` and `tkm-core`
   (for `explain`), never on `tkmd` or `tkm-cli`. `tkmd` and `tkm-cli` call into `tkm-ui`.
4. **The web server runs inside `tkmd`.** The token is per daemon start (SPEC 11.4), so the daemon
   owns the listener. `tkmd` gets a cargo feature `web = ["tkm-ui/web"]`. The server starts only
   when the binary has that feature and `ui.web.enabled = true`. Mutating actions go through a
   `tkm_ui::web::Actions` trait that `tkmd` implements, so `tkm-ui` stays free of daemon code.
5. **Where the dashboard reads its data.** The server subscribes to the daemon's broadcast channel
   and keeps its own `MonitorState`. History charts and evidence pages read the database
   read-only, on request.
6. **`tkm dashboard`.** The daemon writes `tkmd-web.json` (mode 600) next to the socket with the
   URL and token, and deletes it at shutdown. `tkm dashboard` starts the daemon when it is down,
   waits up to 2 s for the file, prints the URL and opens the browser unless `--no-open`. When
   the web feature or setting is off, it prints how to turn it on and exits 1.

Details of the web security model are in ADR 0036; TUI layout and keys in ADR 0037; the CI memory
and render budgets in ADR 0038.

7. **Resynchronization.** Clients subscribe before loading their initial snapshot and apply buffered
   messages afterward. The TUI bounds its socket queue at 1,024 messages and reloads the snapshot
   after overflow. The web dashboard marks its state degraded while reloading after broadcast lag.
   A folded session triggers a refresh of ledger savings rows.

8. **Lag recovery retry.** A failed web snapshot reload remains degraded and retries with
   exponential backoff from 250 ms to 5 s until a reload succeeds or the server stops.
