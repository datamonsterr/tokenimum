# ADR 0049: Fold pricing and skill evidence

## Context

SPEC 7.5 requires session token classes to be "priced with a local price table (`pricing.toml`, refreshed weekly from OpenRouter `/api/v1/models` and harness vendor prices)" and per-decision cost. SPEC 7.3 requires: "count skills loaded (injected or listed) vs invoked, per repository." SPEC 9.4 and 11 require that evidence from `tkm skills --json`.

## Decision

Folds read the cache-aware per-million-token `[models."model-id"]` table at `$XDG_CONFIG_HOME/tokenimum/pricing.toml` (or `~/.config/tokenimum/pricing.toml`). Each session's exact model usage is priced by the existing core pricing engine. The decisions table remains the source of recorded API-reported decision costs; daemon startup restores those costs for budget checks. Exports already contain ledger and decision rows and status already reads decision spend and session savings from them.

Migration 3 adds session-keyed skill evidence and a completion marker. Complete folds replace their own evidence atomically, avoiding double counting on replay. Injected skill IDs count as loaded, and the normalized catalog and skill calls supply the remaining evidence. The CLI aggregates all folded evidence and retains the existing bounded read-only reconstruction for legacy sessions without a marker.

OpenRouter documents model prices at [the models endpoint](https://openrouter.ai/docs/api/api-reference/models/get-models); prices are model-specific and prompt and completion rates differ. Local rates may be supplied for harness vendors too.

## Limitations and remaining work

The existing OpenRouter conversion writes a different table schema and is not scheduled; automatic weekly refresh and vendor-price acquisition remain unimplemented. A missing table, unknown model, missing cache rate, or absent model attribution currently yields zero session priced cost rather than an explicit unknown value; consumers must not interpret that as proof the session was free. Some readers omit model attribution and aggregate counts cannot be accurately priced across multiple models. Those limitations require a schema that represents incomplete pricing and a refresh pipeline.

Successful decision responses already carry API-reported cost and are restored from persisted decision rows, but attempts without a persisted decision row, and unflushed decisions at a crash, cannot be restored. Historical rows without time or session identity cannot be restored to a budget. Existing decisions do not persist token counts. These are remaining gaps against SPEC 7.5 and 8.4.

Host readers currently identify generic skill tools rather than individual skill catalog entries; they do not reliably expose a complete list of loaded skills. Consequently loaded counts cover tokenimum's live injected skills and any explicit normalized skill catalog entries, not every skill a host made available. Invocation counts depend on recognized `Skill`/`skill` calls with a `skill` argument. After daemon restart, injected IDs for unfinished sessions are not persisted, so their loaded evidence can be missing. Partial logs remain ledger-only by SPEC 6.4. No skill is hidden or removed.
