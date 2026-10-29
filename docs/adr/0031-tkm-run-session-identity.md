# ADR 0031: Wrapper session identity and immediate folding

Date: 2026-10-29. Status: accepted.

## Context

`tkm run` supplies its own session ID to the child and sends start and end events
using that ID. Harnesses may use a different, internally generated ID for their
session logs. The daemon's fold readers select logs using the host session ID.
The host library exposes no helper to discover a newly created session for a
working directory, and the daemon's existing path resolution does not discover it.

## Decision

Send the wrapper ID, host and working directory in the lifecycle requests. Do not
assume that the environment variable makes a harness adopt that ID. For agy,
which has no session-end event, rely on the idle watcher.

## Consequences

The end request triggers a fold for the wrapper ID, but cannot guarantee an
immediate fold of the harness's actual session. Daemon-side discovery and
correlation of the real session is follow-up work; the idle watcher remains the
fallback. No harness-specific discovery is added to the CLI.

## Signal handling

The CLI registers only SIGINT while waiting for the child, as required by ADR 0026.
SIGTERM and SIGHUP retain their default dispositions so an external stop does not
leave the wrapper waiting for a child that ignored the stop request.
