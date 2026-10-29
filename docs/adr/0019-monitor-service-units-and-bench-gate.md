# ADR 0019: Monitor API, service units, diagnostics and bench gate

Date: 2026-10-29. Status: accepted (epics E4.6 to E4.8).

## Monitor API (E4.6)

- A **second socket**, `tkmd-monitor.sock` beside the hook socket (mode 600), instead of a hello
  frame on the hook socket: the hook path stays minimal and a TUI client never shares a listener
  with latency-critical requests.
- Framing is the `tkm-proto` framing (limit 64 KiB). The first frame is `Hello`; then
  `MonitorMessage` frames (`tkm_proto::monitor`, additive): `Session`, `Decision`, `Spend`, `Fact`,
  `Redirect`. The stream is read-only.
- A `tokio::sync::broadcast` channel (capacity 256) feeds subscribers. A subscriber that lags or
  does not drain a write within one second is dropped. Handlers publish only when there is a
  receiver and never wait.
- **Fallback when the daemon is down:** the socket is absent; clients open the database with
  `tkm_store::open_read_only` (WAL allows readers beside a writer) and show data without the live
  feed.

## Service units and diagnostics (E4.7)

- Templates `crates/tkmd/service/tkmd.service.in` (systemd user unit) and
  `com.tokenimum.tkmd.plist.in` (launchd agent) with `{{EXE}}` and `{{ERRORS_LOG}}` placeholders,
  rendered by `service::render_systemd_unit` / `render_launchd_plist` (pure, tested; `tkm init`
  installs them in E7). Both send stderr to `errors.log`.
- Diagnostics: `tracing` events at `warn` and above and panics go to `errors.log` through
  `ErrorLog` (rotation at 5 MB, one `.1` backup), because a hook-spawned daemon has no useful
  stderr and `tkm status` needs one file to read.

## Bench gate (E4.8)

- Criterion bench `crates/tkmd/benches/tool_done.rs`: a Claude `PostToolUse` fixture through the
  in-memory handler. `scripts/check_bench_budget.py` fails when mean plus deviation exceeds 200 µs.
- `bench.yml` builds release `tkmd` and `tkm-hook`, starts `tkmd` in a scratch `HOME`/`XDG_*`, waits
  for the socket, runs hyperfine (`--shell=none --input <fixture>`), and
  `scripts/check_hook_latency.py` computes p50/p99 from the `times` array: fail at p50 >= 3 ms or
  p99 >= 10 ms, summary to `$GITHUB_STEP_SUMMARY`. The tkmd bench output is appended to the
  existing Criterion comparison, which keeps its 20% regression gate.

Sources (2026-10-29): <https://docs.rs/criterion/0.8.2/criterion/>,
<https://github.com/sharkdp/hyperfine> (`--input`, `--shell=none`, JSON `times`).
