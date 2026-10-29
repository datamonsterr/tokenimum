# ADR 0018: Fold workers and idle fold

Date: 2026-10-29. Status: accepted (epic E4.5).

1. **Session end.** `session_end` removes the session from memory, queues a session upsert with
   `fold_status = 'pending'`, replies at once, and spawns a detached fold. The fold first flushes
   the outbox (the session row must exist), then runs on `spawn_blocking`: `HostAdapter::read_session`,
   `facts::update` (observe, extract, lifecycle), pruning evidence (`prune::fold`, applied through
   the store's `record_tool_*` counters), and the ledger row. Status becomes `folded`, `partial`,
   `skipped` (no log path) or `error`.
2. **Partial rule.** More than 20% skipped records (or the reader's own `partial` flag) writes the
   ledger only, status `partial`; no facts, no pruning evidence.
3. **Idempotence.** The fact `processed` set is rebuilt from sessions whose status is `folded`. A
   session that was folded and resumes refreshes only its ledger.
4. **Errors.** Every fold error is one `errors.log` line and status `error`; a panic inside the
   blocking task becomes a `JoinError`, also logged. Nothing propagates to the daemon.
5. **Log location.** The hook payload names it (`transcript_path`, `rollout_path`, `session_path`).
   When the payload lacks a path, the fold resolves the session in the host store described in
   SPEC 6.4. Codex and Pi look up session filenames, Kimi derives its workdir hash, opencode
   uses the shared database or export, and other hosts resolve their session-specific files.
6. **Idle fold.** A session idle for 30 minutes (hook events or, via `notify` 8.2, writes to its
   log directory) is folded. The watcher task wakes every 60 s while sessions exist and sleeps on a
   `Notify` otherwise, so idle CPU is zero with no sessions. This replaces per-session deadlines
   with one cheap periodic wake; if `notify` cannot start, only the periodic wake remains (one
   `errors.log` line).
7. **Next session start.** At `session_start`, live sessions of the same repository idle for at
   least 5 minutes, and stored `open`/`pending` sessions of the repository, are folded. The grace
   period keeps two concurrent live sessions in one repository from folding each other.
8. **Facts storage.** Facts map to `FactRecord` (`text_params = {params, contradiction_streak}`,
   `scope` = serde JSON). `tkm-store` gained `update_fact_params` (additive). Candidate evidence
   references are not stored.
9. **Turn index memory.** Remembered turn bases are removed after a fold verifies that all turns
   through that base are durable, unless the session has resumed. Resolution restores the next
   index from the greatest persisted turn index.

Sources (2026-10-29): <https://docs.rs/notify/8.2.1/notify/>.
