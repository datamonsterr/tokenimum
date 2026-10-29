# ADR 0040: E9 integration adapters (superorchestrator, otherskill)

Date: 2026-10-29. Status: accepted.

Closes open question 5 of ADR 0001 (superorchestrator channel) and the two "decided in M7" items
of SPEC 9.3 (escalation channel, spend format). Surfaces were read on 2026-10-29 from the
installed tools: superorchestrator 0.1.0 (`superorchestrator --help`, `runs list --help`,
`account --help`, the hidden `mcp` subcommand, and its source) and otherskill (launcher release
`bqLzN9`: `otherskill --help`, `find --help`, `select` routing result type `RouteResult` in
`lib/otherskill/routing.ts`). Evidence is in `docs/verified/e9-integrations.md`.

## Context

- superorchestrator has no "post an event" CLI command. `runs` offers only `list`, `show`,
  `watch` and `cancel`. Messages travel over its MCP server (`superorchestrator mcp`, stdio).
- Every agent process a run spawns gets `SUPERORCHESTRATOR_RUN_ID`, `SUPERORCHESTRATOR_RUN_TOKEN`,
  `SUPERORCHESTRATOR_SOCKET` and `SUPERORCHESTRATOR_CONFIG_DIR`. `tkm-hook` already forwards
  `SUPERORCHESTRATOR_*` variables to `tkmd` (ADR 0014). An MCP server started with a run's id and
  token acts as that run, which adds the subagent tool `report_progress(step, status, note?)`.
  `report_progress` stores a `progress` message for the parent run and wakes the orchestrator's
  `wait_agent`.
- superorchestrator has no API to ingest usage from another program. Its usage crate reads
  harness rate-limit data only.
- The run list is `superorchestrator runs list --all --json`: an array of objects with `id`,
  `state`, `cwd`, `worktree` (nullable), `harness`, `account`, `session_id` and more.
  (`superorchestrator runs --json` from SPEC 9.3 does not exist.)
- Account files `~/.config/superorchestrator/accounts/*.toml` carry `name`, `harness`, `home`,
  `binary`, `enabled` and other keys. The config dir honours `SUPERORCHESTRATOR_CONFIG_DIR`.
- otherskill has no `--version`; `otherskill --help` is the probe. `otherskill select --json
  --agent <a> --prompt <text>` prints `{status: ok|partial|failed, selected: [{name,
  description, path, category, probability, confidence, ...}], categories, logicalCalls,
  diagnostics}`. Agents are `claude | codex | antigravity | opencode`. Errors exit 0 with an empty
  selection.

## Decision

1. **Escalation channel.** For a delegated run whose hook environment carries
   `SUPERORCHESTRATOR_RUN_ID` and `SUPERORCHESTRATOR_RUN_TOKEN`, `tkmd` starts
   `superorchestrator mcp` (no shell) with exactly those two variables plus
   `SUPERORCHESTRATOR_SOCKET` and `SUPERORCHESTRATOR_CONFIG_DIR` when present, speaks MCP
   JSON-RPC over stdio (`initialize`, `notifications/initialized`, `tools/call`), and calls
   `report_progress` with `step = "tokenimum.loop"`, `status = <suggest>` and
   `note = <the SPEC 9.3 event JSON, one line>`. One call per escalation, 5 s deadline, the child
   is killed on timeout. tokenimum never calls `cancel_agent`, `send_message` or any run-control
   tool; the orchestrator session decides.
   The run token stays in memory for the session's lifetime and is never logged, stored or put
   in an event.
2. **No token.** A run detected only by the cwd fallback has no token. Its escalation uses the
   standalone path (stored `loop.escalation` event plus optional `loop.escalate_command`).
3. **`suggest`.** One more decision, choice `continue | redirect | cancel | reroute`, with the
   digest and the run's routing level as state. When decisions are off or fail, `suggest` is
   `redirect` and `suggest_probs` is empty.
4. **Delegated-run detection.** Primary: `SUPERORCHESTRATOR_RUN_ID` in the hook env. Fallback:
   the session cwd equals or lies under a `worktree` (or `cwd`) of a run with state
   `running | queued | waiting` in `superorchestrator runs list --json`. The list is refreshed by
   the detection pass and by a background worker at most every 30 s; the hook path reads only
   the cache.
5. **Spend report.** superorchestrator cannot ingest it yet. tokenimum appends one JSON line per
   decision to `<tokenimum data dir>/integrations/superorchestrator-spend.jsonl`:
   `{"account":"tokenimum-decisions","ts":<unix s>,"purpose":"...","model":"...","cost_usd":
   <f64>,"run_id":<string|null>}`. The file is tokenimum's; tokenimum never writes into the
   superorchestrator config dir. A follow-up on superorchestrator adds ingestion of this file
   under the synthetic account. This is the M7 deferral of "spend visible in superorchestrator
   status".
6. **Detection.** As SPEC 9.1, with the probes above (`superorchestrator --version`,
   `otherskill --help`), each with a 1 s deadline and no shell. otherskill config: global
   `~/.config/otherskill/otherskill.config.json`, or a project `otherskill.config.json` or
   `.otherskill/config.json` in the cwd or an ancestor. A failure at runtime disables the
   adapter for 15 minutes; detection reruns every 10 minutes.
7. **otherskill call on the prompt path.** `otherskill select --json --agent <a> --prompt <text>
   --timeout-ms 600`, argv only, killed after 700 ms. Host mapping: Claude Code `claude`,
   Codex `codex`, agy `antigravity`, opencode `opencode`; other hosts get no otherskill
   candidates. Status `failed`, a non-zero exit, a timeout or bad JSON counts as a failure.
   Candidates are de-duplicated by skill name (first wins) and enter the shared budget as
   `skill:<name>` with text `<name>: <description>`.
8. **Ranking prior.** `tkm skills --json` prints
   `{"version":1,"source":"tokenimum","skills":[{"name","loaded","used"}]}`, built by
   `tkm_integrations::otherskill::ranking_prior`.
9. **Key reuse.** tokenimum reads otherskill's key file `~/.config/otherskill/credentials.json`
   only when `integrations.otherskill_key_reuse = true`; `tkm init` asks before setting it.
   The default is `false`.

## Consequences

- Adapter detection state is written to `<tokenimum data dir>/integrations/status.json` for status and doctor.

- No change to superorchestrator is needed for escalations. Spend in superorchestrator status
  needs a superorchestrator change (follow-up).
- Escalation reaches the orchestrator through its existing `wait_agent` updates.
- Tests use fake `superorchestrator` and `otherskill` scripts on `PATH`; the fake MCP server
  records the JSON-RPC calls it receives.
