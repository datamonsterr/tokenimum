# ADR 0015: Daemon architecture, state and flush

Date: 2026-10-29. Status: accepted (epic E4.2).

## Decisions

1. **Library first.** `crates/tkmd` is a library (`#![deny(missing_docs)]`) plus a thin `main.rs`.
   Tests and benches drive `tkmd::daemon::Daemon` directly.
2. **One router, fail open.** `Daemon::handle` checks the protocol version, picks the adapter from
   an `AdapterRegistry` (Claude, Codex, and `NoopAdapter` for every other host), maps the event,
   runs the handler and renders with `HostAdapter::render_reply`. Any error, a version mismatch or a
   reply over `MAX_REPLY_FRAME` becomes `Host::noop_reply` plus one `errors.log` line. A malformed or
   oversize request frame closes the connection (the hook fails open on EOF) and logs one line; the
   length is validated with `decode_len` before the body is allocated.
3. **Socket ownership.** The daemon takes an advisory lock (`File::try_lock`, stable since Rust 1.89)
   on `tkmd.lock` beside the socket before touching the socket. A second daemon gets `WouldBlock`,
   exits 0. A leftover socket file is removed only after the lock is held and a connect probe
   reports connection refused. A timeout or would-block probe is treated as a live daemon; other
   probe errors are returned without unlinking the socket.
   The runtime directory is mode 700 and the socket mode 600 (`Paths::ensure_runtime_dir`,
   `secure_socket`).
4. **State in memory.** Sessions and turns live in one `StateMap` keyed by `(host, session_id)`.
   `std::sync::Mutex` guards it; it is held only for in-memory work (microseconds) and never across
   `.await` or I/O. The flusher never takes it. "No blocking lock wait" is read as: no lock whose
   holder can block on disk or network.
5. **Outbox and flusher.** Handlers push rows (sessions, turns, events, decisions, envelope samples,
   calibration) into a `FlushBatch` outbox. The flusher drains it every 5 s, when a turn ends, and
   at shutdown, on `spawn_blocking`. Items leave the batch as they are written; after a failure
   (database locked) the unwritten rest is re-queued and one `errors.log` line is written. Session
   upserts are emitted at start and end only, so a later fold status is never overwritten.
   Turns and events whose session has not resolved remain queued while other rows progress; the
   existing per-kind queue cap applies on requeue, and dropped rows are reported in errors.log.
6. **Storage behind `Backing::open`.** All database access goes through one trait method, so a
   counting fake proves the `tool_done` handler makes no storage access. The default busy timeout
   is 100 ms (below the 300 ms `session_start` budget).
7. **Hot path.** `Daemon::handle_tool_done` is synchronous. It maps, folds the call into the
   `TurnGuard`, may `try_send` one job, pushes rows to the outbox and renders. Sessions first seen
   on `tool_done` (daemon restarted mid-session) are created from the payload alone; repository
   identity, settings and the stored envelope are resolved by the effect worker, off the path.
8. **Repository key** comes from `tkm_config::repo::repo_identity`, cached per cwd.
9. **Decision cost accounting.** `tkm-decide`'s `SpendLedger` and `CalibrationStore` are implemented
   in `tkmd` (`StoreLedger`, `StoreCalibration`), not in `tkm-store`, because `tkm-store` must not
   depend on `tkm-decide`. Spend caps are checked in memory; rows reach the `decisions` table through
   the outbox, where they carry the answers and the shadow flag. Spend of earlier daemon runs is not
   reloaded (the `decisions` table has no timestamp); this is a known gap, listed in the epic report.
10. **Test hooks.** `TKM_DECIDE_ENDPOINT` points the decision client at a local fake server
    (`HttpTransport::with_endpoint`, added additively to `tkm-decide`).
11. **Request ordering.** First frames register in accept order, then requests for each
    `(host, session_id)` execute in that order independently of other sessions and client
    disconnects. The server caps accepted connections at 1,024; excess clients fail open.
12. **Error logging.** Handler failures enqueue flattened lines with a nonblocking send to a
    bounded writer thread. A counter records dropped lines; shutdown waits for queued lines.
13. **Resolution lifetime.** Each in-memory session incarnation has a generation. Background
    resolution only applies to the unresolved incarnation that requested it, so a late result
    cannot reopen an ended session or overwrite a replacement.
14. **Hot-path instrumentation.** Linux tests count the calling thread's write syscalls in
    `/proc/thread-self/io`, along with backing opens and decider calls, including malformed
    payloads and the call that enqueues a judge job.

## Sources checked 2026-10-29

- tokio 1.54: <https://docs.rs/tokio/1.54.1/tokio/> (net::UnixListener, sync::{broadcast, mpsc,
  Notify}, signal::unix, task::spawn_blocking, time::timeout).
- Rust std `File::try_lock`: <https://doc.rust-lang.org/std/fs/struct.File.html#method.try_lock>.
- rusqlite 0.41 / tkm-store: WAL readers do not block on a writer; `open_read_only` for clients.

## Host routing and Cursor duplicates

The standard registry registers all real host adapters, retaining the no-op fallback for unregistered hosts. Claude requests with a string `cursor_version` in their payload are Cursor-generated duplicate hooks; the daemon silently returns the Claude no-op before mapping or changing state.
