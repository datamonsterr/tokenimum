# E3 OpenRouter Decisions verification

Checked 2026-10-29 against the live documentation and model list.

## Decisions API

Source: <https://openrouter.ai/docs/api/api-reference/alphadecisions/submit-a-decisions-questions-and-answers-request.md>
(redirects to `.../submit-a-decisions-request.md`; OpenAPI operation `createApiAlphaDecisions`).

- `POST https://openrouter.ai/api/alpha/decisions`, bearer API key.
- Request `DecisionsRequest`: required `model` (string), `state` (string, object or array),
  `questions` (map name to question). Optional `provider` (`ProviderPreferences`),
  `session_id` (max 256 chars, never sent to the provider), `user` (max 256 chars),
  `trace`, `assets` (images; tokenimum does not send any).
- Questions are tagged by `type`:
  - `noul`: `instructions` required; `criteria` optional, and when present has both
    `true` and `false`.
  - `choice`: `instructions` and `criteria` (map option name to guidance) required.
  - `score`: `instructions` and `criteria` (ordered array, `minItems: 1`) required.
  - `box` (image bounding boxes) exists; tokenimum does not use it.
- Response `DecisionsResponse`: required `model`, `answers`, `usage`; optional `id`,
  `provider`. `usage` has required `input_tokens`, `output_tokens` and optional `cost`
  (double). The `model` field returns the dated version, e.g. `typesafe/jev-1.13-20260917`.
- Answers are tagged by `type`:
  - `noul`: `{type, noul}` where `noul` is the probability of `true`.
  - `choice`: `{type, choice, confidence?, probabilities?}`.
  - `score`: `{type, score, confidence?, legend?, probabilities?}`; in the example
    `score` is the expected index (1.99 for three levels, range 0 to n-1), not a 0–1 value.
- Errors: `{error: {code, message}}` with 400, 401, 402, 403, 404, 413, 429, 500, 502,
  503, 524, 529.
- `ProviderPreferences` has `additionalProperties: false`; `allow_fallbacks` (bool) and
  `data_collection` (`"deny"` or `"allow"`) exist as SPEC 8.4 assumes.

Differences from SPEC 8.1 that the implementation follows (the live reference wins):
answers are typed objects (`noul` / `choice` / `score` fields), not bare probabilities;
`usage.cost` is optional; `noul.criteria` is optional.

Validation rule used by `tkm-decide` (SPEC 8.2 "invalid answer"): every asked question has an
answer of the same type; every probability (`noul`, `confidence`, `probabilities` values) is
finite and within 0–1; a `choice` names a criteria key; a `score` is within 0 to n-1.

## Decision models

Source: <https://openrouter.ai/api/v1/models?output_modalities=decisions>, snapshot stored as
`crates/tkm-decide/tests/fixtures/models-decisions-2026-10-29.json`.

| Slug | Canonical (dated) slug | Prompt price per token | Context |
| --- | --- | --- | --- |
| `inception/mercury-decide` | `inception/mercury-decide-20260930` | 0.00000002 | 65,536 |
| `perplexity/pplx-decider-flash-v1.2` | `perplexity/pplx-decider-flash-v1.2-20261016` | 0.00000001 | 262,144 |
| `typesafe/jev-1.14` | `typesafe/jev-1.14-20261014` | 0.000000042 | 262,144 |

- `inception/mercury-decide:free` is not listed (still unavailable for free, as SPEC 8.2 notes).
  It stays at chain priority 0 behind the `:free` re-probe.
- Completion price is `0` for every decisions model.
- `~...-latest` aliases exist (`~typesafe/jev-latest`, `~perplexity/pplx-decider-flash-latest`);
  the chain rejects them.

## Still to verify live

- Whether the request `model` field accepts the dated canonical slug directly. The default chain
  sends the dated slug; the `TKM_LIVE=1` tests check it per model.
