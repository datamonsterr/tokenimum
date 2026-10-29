# ADR 0014: Hook client design (`tkm-hook`)

Date: 2026-10-29. Status: accepted (epic E4.1).

## Context

Tokenimum injects advice and tracks sessions by intercepting host tool and turn events through CLI hooks.
The hook binary `tkm-hook` is invoked synchronously by host runners (Claude Code, Codex, Cursor, etc.) on every hook lifecycle event.
SPEC 5.1, 6.2, 13.1, 14, and EPICS E4.1 define strict operational constraints:
- **Startup latency**: P99 under 10 ms for the complete round-trip; no async runtime, no heavy framework init, no config file parsing on the hot path.
- **Fail open**: Any failure (socket miss, timeout, framing error, parse error, or panic) must output the host's exact no-op reply, log one line to `errors.log`, and exit 0 without disturbing host execution.
- **Silent stderr**: Host harnesses frequently display stderr directly to the user; stderr must remain completely silent on every code path.

## Command-line interface and parsing

The hook binary is invoked as:
```text
tkm-hook --host <host> --event <event>
```

- **Hand-rolled argument parsing**: We do not use `clap` or external CLI parsing libraries. Parsing is implemented with a single linear pass over `std::env::args()` using string matching and `Host::from_str` / `EventKind::from_str` from `tkm-proto`. This eliminates binary bloat and parsing startup cost.
- **Missing or unrecognized arguments**: If `--host` is valid and recognized but `--event` is missing or unrecognized, `tkm-hook` outputs the recognized host's no-op reply (`Host::noop_reply`), logs one error to `errors.log` (if paths are known), and exits 0. If `--host` is missing or unrecognized, it outputs empty stdout `""`, logs one error, and exits 0.

## Shim options (E6)

The shim accepts two additional options in any order alongside `--host` and `--event`:
- `--host-event <name>` sets `Request.host_event`, taking precedence over the payload's `hook_event_name` value.
- `--payload <json>` supplies the request payload instead of stdin. When present, stdin is not read, so hosts such as Pi that do not provide stdin cannot block the hook. Invalid JSON fails open: the request is still sent with a JSON `null` payload and one error line is appended to `errors.log`.

Unknown options and options with a missing value are ignored. The library keeps its byte-based `run_hook` API and provides a lazy stdin entry point for the binary so stdin is only read when `--payload` is absent.

## Environment allowlist

Host processes may have extensive or sensitive environment variables. To prevent leaking credentials or unrelated data across process boundaries, `tkm-hook` never dumps or forwards the full environment.

We enforce an explicit allowlist:
1. **Exact match keys**:
   - `CLAUDE_PROJECT_DIR`
   - `CLAUDE_CONFIG_DIR`
   - `CODEX_HOME`
   - `TERM_PROGRAM`
2. **Prefix match with secret filtering**:
   - `TKM_*`: All variables prefixed with `TKM_` are forwarded **unless** the variable name (case-insensitive) contains `KEY`, `TOKEN`, or `SECRET` (e.g., `TKM_API_KEY`, `TKM_TOKEN`, `TKM_SECRET`, `TKM_OAUTH_TOKEN` are excluded).
   - `SUPERORCHESTRATOR_*`: Forwarded per SPEC 9.3 for subagent and orchestrator context.

All other environment variables (e.g., `PATH`, `HOME`, `USER`, `SHELL`, AWS or GitHub credentials) are discarded before constructing the `Request`.

## Disable flag (`TKM_DISABLE`)

When `TKM_DISABLE` is set to a truthy value (`1`, `true`, `yes`, case-insensitive), `tkm-hook` immediately prints the host's no-op reply and exits 0.
It does not contact the socket, spawn the daemon, read payload stdin, or touch any log files.

## Unix-domain socket connect and timeout strategy

The client connects synchronously to the daemon's Unix-domain stream socket (`tkmd.sock`) resolved from `tkm_config::Paths::from_env()`.

### Bounding the connect in safe Rust

Per the Linux `unix(7)` manual:
- Unix-domain stream sockets (`AF_UNIX`, `SOCK_STREAM`) operate entirely within the kernel's virtual filesystem and IPC subsystem.
- If the socket file does not exist, `connect(2)` immediately returns `ENOENT`.
- If the socket file exists but no process is listening, `connect(2)` immediately returns `ECONNREFUSED`.
- If a server is listening and the listen queue has room, `connect(2)` succeeds immediately in microseconds.
- If the listen queue is full, a blocking `connect(2)` blocks until backlog space becomes available (`EAGAIN` only applies when non-blocking flags are set).

Standard Rust `std::os::unix::net::UnixStream` does not provide a timed connect, and workspace rules strictly forbid `unsafe` code (`#![forbid(unsafe_code)]`) or extra dependencies such as `socket2`.
To strictly bound connect latency in safe Rust:
- `tkm-hook` spawns a background thread running `UnixStream::connect` and awaits the result over an `std::sync::mpsc` channel with `recv_timeout(CONNECT_BOUND)`.
- `CONNECT_BOUND` is set to 10 ms (allowing a small margin over `tkm_proto::CONNECT_TIMEOUT` of 2 ms to absorb thread scheduling jitter on busy CI runners).
- On timeout, the outcome is treated as `DaemonBusy` (daemon running but listen backlog saturated). The hook logs "daemon busy", does **not** attempt to spawn a redundant daemon, outputs the host no-op reply, and does not join the worker thread (the hook process exit cleanly ends it).

### Write and read timeout strategy

1. **Write timeout**:
   Before writing the request frame, `tkm-hook` sets `stream.set_write_timeout(Some(request.event.read_timeout().max(Duration::from_millis(50))))`.
   When sending large tool payloads (up to 8 MiB) against a daemon that accepted the connection but stopped reading, the kernel socket buffer would otherwise fill up and cause `write_frame` to block indefinitely. The 50 ms floor guarantees prompt failure and recovery.
2. **Read timeout**:
   After writing the request, `tkm-hook` sets `stream.set_read_timeout(Some(request.event.read_timeout()))`:
   - `SessionStart`: 300 ms
   - `Prompt`: 1,500 ms
   - `ToolBefore`, `ToolDone`, `TurnEnd`, `SessionEnd`: 5 ms

The synchronous `tkm_proto::read_frame::<_, Reply>` call is bounded strictly by this read timeout. If the daemon does not respond within the deadline, `read_frame` returns a timeout error, which is logged to `errors.log`, followed by the host no-op reply and exit 0. For `ToolDone`, total hook latency is bounded under 200 ms even under total daemon stall.

## Working directory resolution

The wire `Request` contains an optional `cwd` string:
- If the input JSON payload contains a `"cwd"` property with a string value, that value is forwarded as `Request.cwd`.
- If payload `"cwd"` is absent or `null`, `tkm-hook` falls back to `std::env::current_dir()`, converted to a UTF-8 string if valid.

## Daemon spawning and storm guard

When the daemon socket is absent or dead (`ENOENT` or `ECONNREFUSED`):
1. **Detached spawn**: `tkm-hook` resolves the `tkmd` executable from:
   - `TKM_TKMD_BIN` env var (used for test harnesses and overrides).
   - Sibling executable `tkmd` in the same directory as `std::env::current_exe()`.
   - `tkmd` on `PATH`.
2. The daemon process is spawned detached using `std::os::unix::process::CommandExt::process_group(0)` with `stdin`, `stdout`, and `stderr` redirected to `Stdio::null()`. The hook does not wait for or reap the child.
3. **Storm guard**: To prevent repeated spawn storms when multiple hooks fire in rapid succession while the daemon initializes, the hook touches a marker file `tkmd.spawn` in the runtime directory. If `tkmd.spawn` has been modified within the last 2 seconds (`SPAWN_MARKER_MAX_AGE`), spawning is skipped.
4. After initiating spawn (or skipping via storm guard), the hook prints the host no-op reply and exits 0.

## Error logging and path isolation

- When `Paths::from_env()` returns `None` (for example, in containerized or test environments lacking a `HOME` directory), `tkm-hook` does not touch `/tmp`: it prints the host no-op reply (or empty if host is unknown) and exits 0 without logging.
- When paths are available, errors are written via `tkm_config::paths::ErrorLog::new(paths.errors_log, ERRORS_LOG_MAX_BYTES)`.
- The log file rotates at 1 MiB (`ERRORS_LOG_MAX_BYTES`) to prevent disk bloat.
- Logging is best-effort (`let _ = ...`), never failing the hook.

## Panic guard and stderr hygiene

All hook execution is wrapped in `std::panic::catch_unwind` via `run_with_panic_guard`:
- The default panic hook is temporarily replaced with an empty closure `Box::new(|_| {})` via `std::panic::set_hook` / `take_hook` during execution, preventing any panic trace from writing to `stderr`.
- If a panic occurs, the guard catches it, writes `host.noop_reply()` to stdout, and exits normally.
- Stderr remains completely empty on every single path.

## Exit codes

`tkm-hook` always exits with code 0.

## Documentation and APIs checked

APIs checked on 2026-10-29 against locked workspace dependencies:
- [Linux unix(7) man page](https://man7.org/linux/man-pages/man7/unix.7.html) (AF_UNIX stream semantics, connect backlog behavior, EAGAIN/ECONNREFUSED/ENOENT).
- [serde_json 1.0.154](https://docs.rs/serde_json/1.0.154/serde_json/) (`from_slice`, `Value`).
- [std::os::unix::net::UnixStream](https://doc.rust-lang.org/std/os/unix/net/struct.UnixStream.html) (`connect`, `set_read_timeout`).
- [std::os::unix::process::CommandExt](https://doc.rust-lang.org/std/os/unix/process/trait.CommandExt.html) (`process_group`).
- [std::panic](https://doc.rust-lang.org/std/panic/) (`take_hook`, `set_hook`, `catch_unwind`, `AssertUnwindSafe`).
- `tkm-proto` (ADR 0010: `Request`, `Reply`, `Host`, `EventKind`, framing, timeouts).
- `tkm-config` (ADR 0012: `Paths`, `ErrorLog`, `ERRORS_LOG_MAX_BYTES`).
