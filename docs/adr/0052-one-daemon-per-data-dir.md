# ADR 0052: One daemon per data directory

Status: accepted 2026-11-03.

## Context

The socket lived at `$XDG_RUNTIME_DIR/tokenimum/tkmd.sock`, falling back to
`<data>/tokenimum/run/tkmd.sock` when the variable was unset, and the daemon's advisory lock sat
beside the socket. systemd user units set `XDG_RUNTIME_DIR=/run/user/<uid>`; Claude Code hook and
shell environments often do not. On one machine the systemd daemon listened on
`/run/user/1000/tokenimum/tkmd.sock` while `tkm-hook`, finding no socket at the data-directory
path, spawned a second `tkmd` there. Both wrote the same database (`flush: database is locked`),
the systemd daemon saw no hook events, and `tkm top` showed whichever daemon its shell resolved.

## Decision

1. Runtime directory: an absolute `$XDG_RUNTIME_DIR`; else `/run/user/<uid>` when it is a
   directory (not a symlink) owned by the caller with no group or other permission bits; else
   `<data>/tokenimum/run`. `tkm_config::paths::resolve_runtime_dir` implements it; the uid comes
   from the owner of `/proc/self`, so platforms without `/proc` use the data-directory fallback
   consistently. The socket keeps its SPEC location in the common case.
2. Identity: the daemon lock is `<data>/tokenimum/tkmd.lock` (`Paths::daemon_lock`). `tkmd` takes
   it first thing in `main`, before opening the database, and holds it for its lifetime. A
   second daemon on the same data directory exits 0 whatever runtime directory it resolved.
3. `tkm-hook` checks the lock before spawning: when the socket is missing but the lock is held,
   it logs `daemon holds the data-directory lock but is not reachable at <socket>; not spawning`
   and replies with the no-op.
4. `tkm uninstall` treats either the data-directory lock or the legacy lock beside the socket as
   a running daemon, so daemons from older builds are still detected.

Sockets were not moved under the data directory unconditionally: SPEC names the runtime
directory, home directories may sit on filesystems without socket support, and the fallback
already makes every process of one user agree in the reported case.

## Consequences

A caller whose `$XDG_RUNTIME_DIR` names a different absolute directory than the daemon's still
cannot reach it, but it can no longer start a competing daemon; the error log names the socket it
tried.

## Hook read timeouts

`tkm-hook: read_frame: frame I/O failed: Resource temporarily unavailable (os error 11)` is the
hook's read timeout firing (`EventKind::read_timeout`: 5 ms for `tool_before`, `tool_done`,
`turn_end` and `session_end`). In the audited log the ten occurrences were spread over about
26 hours, none next to the single `database is locked` line, one 2 s after a cold daemon spawn.
The daemon's `tool_done` and `turn_end` paths take only in-memory locks; flushes run on a
blocking task without holding them. The timeouts are the section 14 budget being hit under
scheduling load, not a code defect, and the hook already fails open with the no-op reply.
