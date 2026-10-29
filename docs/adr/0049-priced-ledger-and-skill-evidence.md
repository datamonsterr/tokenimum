# ADR 0049: Priced ledger and skill evidence

## Decision

SPEC 7.5 requires session tokens by class to be "priced with a local price table (`pricing.toml`, refreshed weekly from OpenRouter `/api/v1/models` and harness vendor prices)". The daemon refreshes stale prices from [OpenRouter's models endpoint](https://openrouter.ai/docs/api/api-reference/models/get-models), checked 2026-10-29, into `$XDG_CONFIG_HOME/tokenimum/pricing.toml`; `fetched_at` is Unix milliseconds. Endpoint prompt and completion rates are per token; the local table uses dollars per million. The refresh preserves unrelated local model entries, records available cache rates, rejects an empty listing, replaces the file only after a successful fetch and parse, and retries failures without discarding the previous table. Vendor rates can be supplied locally. A bare model name matches an OpenRouter model only if its last path component is unique; a missing rate or incomplete model attribution produces an unknown price.

Migration 4 adds nullable priced session and decision costs while retaining legacy non-null columns for compatibility. Old zero-valued session rows are ambiguous and migrate as unknown. The status report and dashboard count unknowns; the export serializes unknown costs as null. Savings exclude unpriced sessions. Budgets sum known decision attempts and count unknown attempts separately. Decision attempts are persisted without prompt text, restored on daemon startup, and drained on graceful shutdown. The migration records a boundary so legacy decision records and newly journaled attempts are not counted twice.

SPEC 7.3 says "count skills loaded (injected or listed) vs invoked, per repository"; SPEC 9.4 and 11 require `tkm skills --json`. Migration 3 stores one replaceable report per complete session. Folds count injected skill IDs and normalized skill catalogs as loaded, and recognized `Skill`/`skill` calls as used. The CLI reads persistent reports and reconstructs up to 30 legacy sessions when no fold marker exists. Replays replace evidence rather than adding it twice.

## Limits imposed by source data

SPEC 7.5 requires pricing from "the session log". A model cannot be inferred accurately when the source does not associate usage with a model, especially when a session may switch models:

- Claude Code, Codex, opencode, and Pi readers attribute reported usage to model IDs when those IDs are present. Missing or partially attributed records remain unknown.
- Antigravity's `fixtures/agy/brain/.../transcript_full.jsonl` records token usage but no model ID. The reader also handles `model` on a planner response when supplied. `fixtures/agy/hooks/SessionStart.json` has `modelName`, but a single hook value cannot prove the model for each response in a multi-model transcript.
- Kimi's `fixtures/kimi/sessions/.../wire.jsonl` `StatusUpdate` records token usage without a model ID; its separate config fixture describes a current configuration, not necessarily the model that produced historical usage.
- Cursor's `fixtures/cursor/transcripts/` contain conversation and tool messages without model or token usage. The available transcript cannot establish a session price.
- Codewhale's `fixtures/codewhale/sessions/550e8400-e29b-41d4-a716-446655440000.json` has `metadata.model` but no token usage; the reader retains the model on turns and does not invent usage from it.

SPEC 7.3 also requires skills "injected or listed". Individual loaded skill catalogs are not present in the normalized data from these recorded host transcripts; the generic skill tool itself is not a catalog of skill names. Counts therefore reflect tokenimum injections, explicit normalized catalog entries and recognizable invocations, not a claim that every host's available skills were listed. Partial logs remain ledger-only as required by SPEC 6.4.
