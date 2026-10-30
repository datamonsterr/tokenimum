# ADR 0045: Offline decision bench and outcome calibration

Date: 2026-10-29. Status: accepted for E10.1–E10.2.

The corpus has 50 synthetic sessions, 25 in each of the Claude Code and Codex JSONL shapes.
They contain only invented task text, commands, output and identifiers; no real user data.
Each has one completed or interrupted turn. The host readers normalize the logs, and the
production redactor and digest builder construct the state. The interrupted turns are labelled
`stuck`; normally completed turns are labelled `progress`. For the fact-filter comparison the
first command is a deliberately task-specific, harmless candidate, labelled unstable and safe.
These synthetic labels exercise the plumbing; they are not a representative evaluation set.

`cache.json` has a `source` and entries keyed by session, turn and model. Each entry carries the
SHA-256 of the session, the exact loop and fact question sets, candidate, typed responses,
latencies and costs. Recorded entries also retain the redacted request digest; replay rejects a changed digest. Synthetic entries are bound to the fixture hash and question sets. A missing, duplicate, stale or invalid response fails the run. The checked-in
cache's source is `synthetic test double`: answers, model names, timings and costs are invented,
not measurements. `tkm replay --bench fixtures/bench` validates and renders the report entirely
offline. The integration test compares that output byte-for-byte with the committed report.

Real per-model numbers require a recorded cache. With `OPENROUTER_API_KEY` set, run
`tkm replay --bench --record fixtures/bench` and redirect its output to
`docs/reports/m0-decision-bench.md`. Recording asks each configured dated model separately for
loop and fact decisions, uses redacted state, validates every response, and replaces the cache
only when all requests succeed. The cache identifies live recordings or a configured test
endpoint; no key is saved. The recording test uses a local mock. Running the command incurs API
costs and may transmit the synthetic fixture digests to the selected provider.

For live loop decisions, interruption or a rule backstop produces a `stuck` outcome; a good
completion after at least one further tool call produces `progress`. Ambiguous completions and
results that arrive after their turn has ended remain unlabelled. The outbox links outcomes to
persisted decision IDs, including when flushing is delayed. `tkm calibrate` fits the existing
labelled decisions per model and purpose. `--fixtures` fits the same policy to the fixture
outcomes. A fit requires at least 20 samples, recall at least 0.5, stuck precision at least 0.9,
and a threshold greater than `t_low`. The daemon accepts loop thresholds only when their stored
precision is in [0.9, 1.0]; missing, unscored or revoked rows stay in shadow. Other purposes do not
yet have a production outcome-label contract and are not calibrated by this command.
