# ADR 0030: Store schema v2 for CLI reports

Date: 2026-10-29. Status: accepted.

## Context

SPEC 11.2 and 11.5 require action times, recent fact retirements, daily decision spend,
repository-scoped export and evidence-backed explanations. V001 did not retain action or
decision times, decision ownership or request state. The CLI must read the database without
creating it or migrating it while the daemon is down.

## Decisions

1. Append V002 without modifying V001. Add nullable Unix-millisecond columns
   `events.created_at`, `decisions.created_at`, `facts.state_changed_at`, and nullable
   `decisions.repo_key`, `decisions.session_id`, `decisions.digest`. Existing rows retain NULL;
   reports display `unknown time` and exclude undated rows from time-window counts.
2. Keep every existing store method signature and input record. Add `insert_event_at`,
   `insert_decision_with_context` and `transition_fact_at`. Legacy methods write NULL for
   the new context, including transitions whose actual time is unknown. Read records expose
   optional context with serde defaults. `from_read_only_connection` accepts only a connection
   SQLite reports as read-only, allowing the CLI to reuse its bounded database helper and the
   typed store queries without migrations or filesystem writes. V001 read-only connections
   project the new fields as NULL, so an upgrade does not require starting the daemon first.
3. The daemon captures action and decision times from its clock. Decision outbox entries retain
   the known repository, natural session key and redacted request state. Decisions flush before
   actions. Shared atomic evidence links resolve the exact request to its persisted decision ID,
   including across flushes; actions never guess from the latest decision in a session.
   The coordinated daemon glue extensions are `DecisionInsert`, timestamp/evidence fields on
   `EventInsert`, the contextual `FlushBatch.decisions` queue and `SessionState.loop_evidence`.
   These workspace-internal construction sites are updated together. Cache hits retain their
   answer evidence but record zero new cost. Fact evidence uses real fact
   rowids, with repository-local fact names retained in `inputs.fact_ids`; `inputs.decision_ids`
   disambiguates the two integer namespaces. Rule-only actions can have no decision evidence.
   Normalized `hook.seen` observations retain only event kind and time, allowing status to
   compare observed coverage against `tkm_hosts::probe` without persisting hook payloads.
4. Fact transitions during folds use the existing fold completion clock. The CLI's explicit
   `facts forget` operation retires through the timestamped store API and has no undo command.
5. Report reads use the existing database read-only. UTC time is supplied explicitly to
   `status::run_at` for deterministic snapshots; the command boundary supplies wall time.
   Daily spend is global because the decision client's daily cap is global. Savings use core's
   estimator, with baseline and treatment sample sizes and a 95% interval where available.
   Optional integration adapters currently have only no-op implementations; status reports
   them as off with their configured mode and the reason, without claiming a live adapter.
6. Export contains only current-repository records and relevant installation manifests, never
   configuration or environment values. Digest fields are omitted unless `--show-digests`.
   Stored digests are the redacted state supplied to the client; the unchanged `tkm-decide`
   redactor also protects exported text defensively. Email redaction is outside SPEC 8.4.
7. Extend protocol-1 `MonitorMessage::Hello` additively with optional `daemon_version` and
   `started_at_ms`, both `#[serde(default, skip_serializing_if = "Option::is_none")]`.
   Old `{"type":"hello","version":1}` frames still decode. The daemon fills package version
   and its original start time; the CLI carries them in `DaemonState::Running` and reports
   `unknown` for absent values. The wire protocol version remains a separate compatibility
   check and is never displayed as the daemon package version.

## Consequences

Historical times and repository-less decisions cannot be reconstructed. Unattributed decisions
still count toward dated global daily spend, but cannot be assigned to a repository export.
Older daemons retain useful read-only reports with unknown version/uptime. No dependency,
API key, model-generated explanation template or runtime migration is added to report reads.

The migration golden reflects SQLite's canonical ALTER TABLE text. Regression coverage includes
V001 upgrades, nullable/legacy writes, context round trips, daemon action evidence, old/new Hello
frames, deterministic status/why snapshots, fact retirement and export redaction. Rust tests,
clippy and compilation run in CI only, as required by the delivery brief.
