---
name: openrouter-decisions
description: Use when working on tkm-decide or any code that sends OpenRouter Decisions API requests (noul, choice, score questions), the model fallback chain, calibration thresholds, redaction of digests, or decision budgets in tokenimum.
---

# OpenRouter Decisions in tokenimum

SPEC.md section 8 is the source of truth.

## Before coding

Read the current API reference; do not write request or response types from memory:

- API reference: https://openrouter.ai/docs/api/api-reference/alphadecisions/submit-a-decisions-questions-and-answers-request.md
- Jev guide: https://openrouter.ai/docs/guides/community/jev
- Tool gating cookbook: https://openrouter.ai/docs/cookbook/building-agents/gate-tool-calls-with-jev
- Decision model list: https://openrouter.ai/api/v1/models?output_modalities=decisions
- OpenRouter's own agent skill (no license file upstream, so not copied here; read it online):
  https://github.com/OpenRouterTeam/skills/tree/main/skills/openrouter-decisions

## Shape (verify against the reference)

`POST https://openrouter.ai/api/alpha/decisions` with `{model, state, questions}`.
Question types: `noul` (`criteria: {true, false}`), `choice` (criteria keys are the options),
`score` (criteria is an ordered array). Response: `{id, model, provider, answers, usage}`.

## Rules

1. One request per purpose; batch all questions for that purpose.
2. `state` is a code-built digest, redacted, at most 16 KB. Never send raw logs, env values,
   secrets or paths outside the repository.
3. Model chain from config, tried in order: `inception/mercury-decide:free` (when live),
   `inception/mercury-decide`, `perplexity/pplx-decider-flash-v1.2`, `typesafe/jev-1.14`.
   Pin dated versions; never `-latest` aliases. The API has no fallback array; the client walks
   the chain.
4. Treat 404, 5xx, a 1,500 ms timeout, a missing answer key, or a probability outside 0 to 1 as a
   failure and try the next model.
5. A model version with no calibration row: store the answer, take no action.
6. Every request records `usage.cost`, latency and model in the `decisions` table and counts
   against the daily and per-session budgets. Over budget means code-only behaviour.
7. Tests use recorded responses. Live calls only behind `TKM_LIVE=1`.
8. The API key comes from `OPENROUTER_API_KEY` or a mode-600 key file. Never log it.
