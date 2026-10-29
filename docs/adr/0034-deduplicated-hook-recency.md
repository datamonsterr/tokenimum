# ADR 0034: Recency for deduplicated hook observations

Date: 2026-10-29. Status: accepted.

## Context

`hook.seen` is deduplicated per session and native event so repeated tool hooks do not
allocate observation rows or lock the outbox. CLI status derives its last-event time from
these rows, which otherwise retain the first occurrence's timestamp indefinitely.

## Decisions

1. Keep the tool handler and its deduplication unchanged. Reuse the session's existing
   `last_activity_ms`, which already advances on tool hooks and also reflects log activity.
2. At the existing periodic/turn-end flush, take a short in-memory snapshot of activity for
   live sessions with hook observations. This narrows ADR 0015's "flusher never takes" the
   state lock: the snapshot performs no disk or network operations, and the guard is dropped
   before storage work or an await. Session upserts still occur only at start/end.
3. Queue internal timestamp refreshes after observation inserts in the drained batch. The
   blocking flusher advances only the latest `hook.seen` row per session. An unchanged or
   older timestamp writes nothing. The operation inserts no new event and leaves coverage
   inputs intact. Failed refreshes use the existing outbox retry and queue-cap behavior.
4. A session-end hook has its own observation before the session leaves memory, preserving
   its final event time. Idle-folded sessions retain their last flushed activity time.

## Consequences

- Status recency follows activity within the normal five-second flush delay, without a
  per-call clock read, allocation, outbox lock, or disk write beyond existing handler work.
- Coverage row counts and the CLI/monitor wire format remain unchanged.
- `Store::refresh_hook_observation` is an additive API; existing public signatures and the
  database schema stay unchanged.
