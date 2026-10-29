# ADR 0038: UI render and daemon memory budgets in CI

Date: 2026-10-29. Status: accepted (epic E8).

## Decisions

1. **Frame render under 50 ms.** Criterion bench `crates/tkm-ui/benches/render.rs` renders each pane
   on a 200x50 `TestBackend` with a full fixture (500 sessions, 1,000 decisions, 500 facts). The
   existing `scripts/check_bench_budget.py` checks every `render_` case against 50,000,000 ns.
2. **`tkmd` idle memory.** A `bench.yml` job builds release `tkmd` twice, without and with
   `--features web`, and starts each in a scratch `HOME` (the second with
   `TKM_UI_WEB_ENABLED=true`). It waits for the socket, waits 5 s more, then reads `VmRSS` from
   `/proc/<pid>/status` three times, 1 s apart. `scripts/check_idle_memory.py` takes the maximum
   and fails at 30 MiB or more (default build) and 40 MiB or more (`web`). The web run also checks
   that an `/api/state` request without a token gets 401. Results go to `$GITHUB_STEP_SUMMARY`.
