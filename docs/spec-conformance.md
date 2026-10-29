# Spec conformance audit

Status is `met` when linked code, test, or CI evidence supports the stated requirement;
`deferred` records a known gap, external limitation, or evidence still needed; and
`not applicable` identifies explanatory material outside the implementation contract. Links to
source show implementation, while tests and CI identify executable checks. An evidence link does
not imply that the test has been run locally. Historic host observations and model probes describe
what was known at the date in SPEC, not current upstream guarantees.

## Gaps found

The following exact SPEC text identifies gaps found in this audit. They are recorded for scheduling
in [ADR 0050](../docs/adr/0050-conformance-gaps.md); that record does not close them.

- SPEC 7.5: “Per session: tokens by class (input, cache write, cache read, output) from the session log; priced with a local price table (`pricing.toml`, refreshed weekly from OpenRouter `/api/v1/models` and harness vendor prices).” [fold.rs](../crates/tkmd/src/fold.rs) stores zero priced cost.
- SPEC 7.3: “count skills loaded (injected or listed) vs invoked, per repository.” [skills.rs](../crates/tkm-cli/src/cmd/skills.rs) derives a limited report from Claude logs only.
- SPEC 15 M4: “fold works on a real session of each”. [host verification](../docs/verified/e5b-hosts.md) identifies synthetic fixtures and forum-reported formats for hosts without local installs.
- SPEC 2 G1: “Lower mean cost per session in repeat repositories without lowering task success.” [ADR 0050](../docs/adr/0050-conformance-gaps.md) records the missing representative outcome evaluation.
- SPEC 2 G4: “total model spend under 1% of the agent spend it saves.” [ADR 0050](../docs/adr/0050-conformance-gaps.md) records the missing representative savings measurement.

- SPEC 6.3: “redirects via `pi.sendMessage(..., {deliverAs:"steer"})`”. [Pi shim](../shims/pi.ts) has no steer delivery path.
- SPEC 6.1: “if the hook or shim path is unavailable, disabled by the person, or fails `tkm doctor`, the adapter drops to `fold-only`”. [doctor.rs](../crates/tkm-cli/src/cmd/doctor.rs) reports state but automatic switching after a failed probe is not established.

- SPEC 6.10: “Events used: `session_start`, `message_submit`, `tool_call_before`, `tool_call_after`, `turn_end`, `session_end`, `session_idle`.” [Codewhale installer](../crates/tkm-hosts/src/codewhale/install.rs) does not register `session_idle`.
- SPEC 6.8: “`tkm doctor` runs a probe session and records which events arrived; missing events switch that event to idle-fold.” [doctor.rs](../crates/tkm-cli/src/cmd/doctor.rs) reads recorded events but does not run a host session or switch mode.

- SPEC 7.1: “Filter (decision, batched: all candidates of the session in one request).” [fold.rs](../crates/tkmd/src/fold.rs) passes no decision evidence to fact updates.
- SPEC 7.1: “Contradiction check (decision, batched): for each active fact the session exercised, noul `contradicts_<id>` against the session digest.” The same fold path supplies no model verdicts.
- SPEC 8.5: “Shadow mode (default for the first 14 days or 200 judged turns): decisions are made and stored, no action is taken; TE's rule verdicts run beside them.” [live.rs](../crates/tkmd/src/live.rs) gates on calibration, not independently on the stated duration/count.

- SPEC 8.2: “`tkm doctor` re-probes `:free` slugs daily and moves a free model to the front when it answers.” [doctor models](../crates/tkm-cli/src/cmd/doctor/models.rs) performs an explicit probe, with no daily scheduler established.

- SPEC 10.4: `[facts]    enabled = true`, `[ledger]   enabled = true`, and `[hosts.claude]   enabled = true  # one table per harness; level = "auto" | "hooks" | "shim" | "fold-only"`: [settings.rs](../crates/tkmd/src/settings.rs) does not apply these registered keys.

- SPEC 16: “Golden: replay fixtures per harness (all eight); expected actions committed as JSON.” A single loop golden exists, but no all-host expected-action JSON suite was identified.
- SPEC 16: “Parity: for Claude Code and Codex fixtures, tokenimum's fact formation and loop gate agree with TE run over the same logs (TE is the oracle in M2; differences documented).” Ported tests exist, but no same-log TE oracle comparison was identified.

- SPEC 6.8: “Blocks that matter are repeated on the next `postToolUse`.” [ADR 0050](../docs/adr/0050-conformance-gaps.md) item 11 records that this repetition is not established.
- SPEC 15 M1: “shadow decisions logged on real sessions”. The [end-to-end test](../crates/tkm-cli/tests/e2e_shadow.rs) uses an installed fake home and fixture events.
- SPEC 15 M3: “fresh machine to working install in one command; every action explainable”. [ADR 0050](../docs/adr/0050-conformance-gaps.md) items 15 and 17 record missing switch wiring and fresh-machine evidence.
- SPEC 16: “Fault injection: daemon down, socket slow, OpenRouter 404/500/timeout, DB locked, corrupt log, foreign SQLite locked, shim timeout. Each must yield the no-op reply and an error line, never a hook failure.” [ADR 0050](../docs/adr/0050-conformance-gaps.md) item 19 records incomplete matrix evidence.

- SPEC 11.3 Spend pane: “decision spend today vs caps; per-model share; sparkline over 7 days”. [tui.rs](../crates/tkm-ui/src/tui.rs) renders caps and a text series, but not model share or a sparkline.
- SPEC 11.3 Sessions pane: “active sessions: harness, repository, turn, tool calls this turn vs envelope, loop state”. [tui.rs](../crates/tkm-ui/src/tui.rs) omits the turn identifier.
- SPEC 11.3 Redirects pane: “redirects and escalations with the triggering calls”. [tui.rs](../crates/tkm-ui/src/tui.rs) displays redirect text but no triggering calls.

## Pending

E10.6 replay benchmark harness is being merged in parallel. Its evidence is pending; the controller
will replace the placeholder row after that work lands.


## 1. Summary

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S1-1 | Rust hook and daemon meet hook latency target | met | [0046-performance-gates.md](../docs/adr/0046-performance-gates.md) |
| S1-2 | All model judgments use Decisions with a priority chain | met | [client.rs](../crates/tkm-decide/src/client.rs) |
| S1-3 | Eight harness adapters support integration | met | [registry.rs](../crates/tkmd/src/registry.rs) |
| S1-4 | superorchestrator and otherskill are optional | met | [matrix.rs](../crates/tkm-integrations/tests/matrix.rs) |
| S1-5 | Install, status, TUI, dashboard, config, explanations | met | [cli.rs](../crates/tkm-cli/src/cli.rs) |
| S1-6 | Forbid unsafe in own crates and audit dependencies | met | [ci.yml](../.github/workflows/ci.yml) |

## 2. Goals and non-goals

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S2-1 | Goals and exclusions are detailed below | not applicable | Section introduction; see [SPEC.md](../SPEC.md). |

### Goals

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S2.G-1 | G1: reduce repeat-repo cost without loss of success | deferred | [0050-conformance-gaps.md](../docs/adr/0050-conformance-gaps.md) |
| S2.G-2 | G2: hook p50 &lt; 3 ms, p99 &lt; 10 ms | met | [0046-performance-gates.md](../docs/adr/0046-performance-gates.md) |
| S2.G-3 | G3: share neutral facts by repository identity | met | [repo_paths.rs](../crates/tkm-config/tests/repo_paths.rs) |
| S2.G-4 | G4: Decisions only; spend &lt; 1% of savings | deferred | [0050-conformance-gaps.md](../docs/adr/0050-conformance-gaps.md) |
| S2.G-5 | G5: failures leave the agent running | met | [integration.rs](../crates/tkm-hook/tests/integration.rs) |
| S2.G-6 | G6: actions visible, explainable, reversible | deferred | [0032-cli-prune-gaps.md](../docs/adr/0032-cli-prune-gaps.md) |
| S2.G-7 | G7: standalone, optional integrations | met | [matrix.rs](../crates/tkm-integrations/tests/matrix.rs) |
| S2.G-8 | G8: no unsafe in own crates | met | [ci.yml](../.github/workflows/ci.yml) |

### Non-goals

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S2.N-1 | Output compression is excluded | not applicable | Excluded scope, not a required feature; [SPEC.md](../SPEC.md). |
| S2.N-2 | Model/weights changes are excluded | not applicable | Excluded scope, not a required feature; [SPEC.md](../SPEC.md). |
| S2.N-3 | Copilot CLI is excluded | not applicable | Excluded scope, not a required feature; [SPEC.md](../SPEC.md). |
| S2.N-4 | Model-generated prose is excluded | not applicable | Excluded scope, not a required feature; [SPEC.md](../SPEC.md). |

## 3. Glossary

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S3-1 | Terms define vocabulary used by subsequent requirements | not applicable | Definitions only; [SPEC.md](../SPEC.md). |

## 4. How TokenEconomizer works, and what tokenimum keeps

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S4-1 | Introduction to replicated mechanisms | not applicable | Overview only; [SPEC.md](../SPEC.md). |

### 4.1 TE flow (replicated)

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S4.1-1 | SessionStart injects facts and notices | met | [inject.rs](../crates/tkmd/tests/inject.rs) |
| S4.1-2 | Prompt starts turn; tool events feed guard | met | [live.rs](../crates/tkmd/tests/live.rs) |
| S4.1-3 | Free gate judges asynchronously; code backstop redirects at 2.5 times envelope | met | [loopguard_behavior.rs](../crates/tkm-core/tests/loopguard_behavior.rs) |
| S4.1-4 | Second stuck segment escalates without stopping turn | met | [live.rs](../crates/tkmd/tests/live.rs) |
| S4.1-5 | Stop closes turn and good turns feed envelope | met | [live.rs](../crates/tkmd/tests/live.rs) |
| S4.1-6 | SessionEnd folds facts, pruning and cache-aware ledger | deferred | [0050-conformance-gaps.md](../docs/adr/0050-conformance-gaps.md) |

### 4.2 What tokenimum changes in that flow

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S4.2-1 | Rust hook and daemon communicate by Unix socket | met | [lib.rs](../crates/tkm-hook/src/lib.rs) |
| S4.2-2 | Turn state in memory, flush every 5 s and turn end | met | [0015-daemon-architecture-and-flush.md](../docs/adr/0015-daemon-architecture-and-flush.md) |
| S4.2-3 | Noul judge and choice-based templated redirect | met | [live.rs](../crates/tkmd/src/live.rs) |
| S4.2-4 | Code extracts facts, model filters; score relevance in shared budget | deferred | [ADR 0050](../docs/adr/0050-conformance-gaps.md) item 12 records missing fact decisions; [injection.rs](../crates/tkmd/src/injection.rs) scores relevance. |
| S4.2-5 | Headless escalation forwarded or stored | met | [0040-e9-integrations.md](../docs/adr/0040-e9-integrations.md) |
| S4.2-6 | Install into detected homes, optional account homes | met | [init_lifecycle.rs](../crates/tkm-cli/tests/init_lifecycle.rs) |
| S4.2-7 | SQLite ledger and optional superorchestrator reporting | deferred | [0040-e9-integrations.md](../docs/adr/0040-e9-integrations.md) |
| S4.2-8 | Status, top, dashboard and why monitors | met | [cli.rs](../crates/tkm-cli/src/cli.rs) |

### 4.3 TE parameters kept as defaults

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S4.3-1 | facts.promote_confirmations = 3 | met | [config.rs](../crates/tkm-config/src/config.rs) |
| S4.3-2 | facts.recency_decay = 0.8 | met | [config.rs](../crates/tkm-config/src/config.rs) |
| S4.3-3 | facts.contradiction_weight = 5.0 | met | [config.rs](../crates/tkm-config/src/config.rs) |
| S4.3-4 | facts.fire_gate = 0.40 | met | [config.rs](../crates/tkm-config/src/config.rs) |
| S4.3-5 | facts.fire_min_sessions = 4 | met | [config.rs](../crates/tkm-config/src/config.rs) |
| S4.3-6 | facts.max_facts = 4; facts.max_tokens = 150 | met | [config.rs](../crates/tkm-config/src/config.rs) |
| S4.3-7 | prune.evidence_prompts = 10 | met | [config.rs](../crates/tkm-config/src/config.rs) |
| S4.3-8 | prune.min_sessions = 2 | met | [config.rs](../crates/tkm-config/src/config.rs) |
| S4.3-9 | prune.sessions_any = 5 | met | [config.rs](../crates/tkm-config/src/config.rs) |
| S4.3-10 | prune.quiet_minutes = 60 | met | [config.rs](../crates/tkm-config/src/config.rs) |
| S4.3-11 | loop.min_turns = 20 | met | [config.rs](../crates/tkm-config/src/config.rs) |
| S4.3-12 | loop.gate_min_turns = 18 | met | [config.rs](../crates/tkm-config/src/config.rs) |
| S4.3-13 | loop.gate_error_outputs = 8 | met | [config.rs](../crates/tkm-config/src/config.rs) |
| S4.3-14 | loop.edit_churn = 4 | met | [config.rs](../crates/tkm-config/src/config.rs) |
| S4.3-15 | loop.stagnation = 0.20 | met | [config.rs](../crates/tkm-config/src/config.rs) |
| S4.3-16 | loop.envelope_pct = 90; loop.envelope_margin = 1.25 | met | [config.rs](../crates/tkm-config/src/config.rs) |
| S4.3-17 | loop.envelope_min_samples = 5 | met | [config.rs](../crates/tkm-config/src/config.rs) |
| S4.3-18 | loop.backstop_mult = 2.5 | met | [config.rs](../crates/tkm-config/src/config.rs) |
| S4.3-19 | loop.redirects_per_turn = 1 | met | [config.rs](../crates/tkm-config/src/config.rs) |
| S4.3-20 | loop.level = default; conservative and aggressive presets | met | [config.rs](../crates/tkm-config/src/config.rs) |

## 5. Architecture

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S5-1 | Hook, daemon, store, adapters and monitor composition | met | [main.rs](../crates/tkmd/src/main.rs) |

### 5.1 Components

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S5.1-1 | Hook reads stdin, forwards host/env over framed socket and fails open | met | [integration.rs](../crates/tkm-hook/tests/integration.rs) |
| S5.1-2 | Connect timeout 2 ms | deferred | [0014-hook-client.md](../docs/adr/0014-hook-client.md) sets 10 ms due to safe-Rust scheduling. |
| S5.1-3 | Event-specific read timeouts | met | [lib.rs](../crates/tkm-proto/src/lib.rs) |
| S5.1-4 | Daemon uses tokio and systemd/launchd service; hook starts detached on miss | met | [service.rs](../crates/tkmd/tests/service.rs); [integration.rs](../crates/tkm-hook/tests/integration.rs) |
| S5.1-5 | CLI command surface and read-only daemon-down fallback | met | [status_report.rs](../crates/tkm-cli/tests/status_report.rs) |
| S5.1-6 | opencode and Pi generated shims ≤40 lines forward events and apply replies | met | [shim.rs](../crates/tkm-hosts/tests/shim.rs) |

### 5.2 Crate layout

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S5.2-1 | Workspace and lint policy | met | [Cargo.toml](../Cargo.toml) |
| S5.2-2 | Dependency denial policy | met | [deny.toml](../deny.toml) |
| S5.2-3 | tkm-proto framing and contracts | met | [lib.rs](../crates/tkm-proto/src/lib.rs) |
| S5.2-4 | tkm-hook binary | met | [main.rs](../crates/tkm-hook/src/main.rs) |
| S5.2-5 | tkm-core pure mechanisms | met | [lib.rs](../crates/tkm-core/src/lib.rs) |
| S5.2-6 | tkm-hosts adapters | met | [lib.rs](../crates/tkm-hosts/src/lib.rs) |
| S5.2-7 | tkm-decide client | met | [lib.rs](../crates/tkm-decide/src/lib.rs) |
| S5.2-8 | tkm-store SQLite | met | [lib.rs](../crates/tkm-store/src/lib.rs) |
| S5.2-9 | tkm-integrations adapters | met | [lib.rs](../crates/tkm-integrations/src/lib.rs) |
| S5.2-10 | tkm-config layers | met | [lib.rs](../crates/tkm-config/src/lib.rs) |
| S5.2-11 | tkm-ui monitor and web | met | [lib.rs](../crates/tkm-ui/src/lib.rs) |
| S5.2-12 | tkmd daemon | met | [main.rs](../crates/tkmd/src/main.rs) |
| S5.2-13 | tkm-cli binary | met | [main.rs](../crates/tkm-cli/src/main.rs) |
| S5.2-14 | Shim templates | met | [README.md](../shims/README.md) |
| S5.2-15 | Host fixtures | met | [README.md](../fixtures/README.md) |
| S5.2-16 | Every crate forbids unsafe and inherits workspace lint | met | [check_forbid_unsafe.py](../scripts/check_forbid_unsafe.py) |
| S5.2-17 | Core has no I/O or network | met | [check-core-pure.sh](../scripts/check-core-pure.sh) |

### 5.3 Libraries (check current docs before coding; no API from memory)

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S5.3-1 | Workspace libraries and pinned versions | met | [Cargo.lock](../Cargo.lock); [Cargo.toml](../Cargo.toml) |
| S5.3-2 | Dependency policy enforced in CI | met | [ci.yml](../.github/workflows/ci.yml) |
| S5.3-3 | Use and pin tokio | met | [Cargo.lock](../Cargo.lock) |
| S5.3-4 | Use and pin serde and serde_json | met | [Cargo.lock](../Cargo.lock) |
| S5.3-5 | Use and pin rusqlite bundled with WAL | met | [Cargo.lock](../Cargo.lock) |
| S5.3-6 | Use and pin reqwest with rustls | met | [Cargo.lock](../Cargo.lock) |
| S5.3-7 | Use and pin tracing | met | [Cargo.lock](../Cargo.lock) |
| S5.3-8 | Use and pin clap | met | [Cargo.lock](../Cargo.lock) |
| S5.3-9 | Use and pin ratatui and crossterm | met | [Cargo.lock](../Cargo.lock) |
| S5.3-10 | Use and pin axum under web feature | met | [Cargo.lock](../Cargo.lock) |
| S5.3-11 | Use and pin regex | met | [Cargo.lock](../Cargo.lock) |
| S5.3-12 | Use and pin globset | met | [Cargo.lock](../Cargo.lock) |
| S5.3-13 | Use and pin toml_edit | met | [Cargo.lock](../Cargo.lock) |
| S5.3-14 | Use and pin directories | met | [Cargo.lock](../Cargo.lock) |

## 6. Harness support

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S6-1 | claude: adapter, log reader, installer and fixture tests | met | [claude_contract.rs](../crates/tkm-hosts/tests/claude_contract.rs); [claude_reader.rs](../crates/tkm-hosts/tests/claude_reader.rs); [claude_install.rs](../crates/tkm-hosts/tests/claude_install.rs) |
| S6-2 | codex: adapter, log reader, installer and fixture tests | met | [codex_contract.rs](../crates/tkm-hosts/tests/codex_contract.rs); [codex_reader.rs](../crates/tkm-hosts/tests/codex_reader.rs); [codex_install.rs](../crates/tkm-hosts/tests/codex_install.rs) |
| S6-3 | agy: adapter, log reader, installer and fixture tests | met | [agy_contract.rs](../crates/tkm-hosts/tests/agy_contract.rs); [agy_reader.rs](../crates/tkm-hosts/tests/agy_reader.rs); [agy_install.rs](../crates/tkm-hosts/tests/agy_install.rs) |
| S6-4 | cursor: adapter, log reader, installer and fixture tests | met | [cursor_contract.rs](../crates/tkm-hosts/tests/cursor_contract.rs); [cursor_reader.rs](../crates/tkm-hosts/tests/cursor_reader.rs); [cursor_install.rs](../crates/tkm-hosts/tests/cursor_install.rs) |
| S6-5 | kimi: adapter, log reader, installer and fixture tests | met | [kimi_contract.rs](../crates/tkm-hosts/tests/kimi_contract.rs); [kimi_reader.rs](../crates/tkm-hosts/tests/kimi_reader.rs); [kimi_install.rs](../crates/tkm-hosts/tests/kimi_install.rs) |
| S6-6 | codewhale: adapter, log reader, installer and fixture tests | met | [codewhale_contract.rs](../crates/tkm-hosts/tests/codewhale_contract.rs); [codewhale_reader.rs](../crates/tkm-hosts/tests/codewhale_reader.rs); [codewhale_install.rs](../crates/tkm-hosts/tests/codewhale_install.rs) |
| S6-7 | opencode: adapter, log reader, installer and fixture tests | met | [opencode_contract.rs](../crates/tkm-hosts/tests/opencode_contract.rs); [opencode_reader.rs](../crates/tkm-hosts/tests/opencode_reader.rs); [opencode_install.rs](../crates/tkm-hosts/tests/opencode_install.rs) |
| S6-8 | pi: adapter, log reader, installer and fixture tests | met | [pi_contract.rs](../crates/tkm-hosts/tests/pi_contract.rs); [pi_reader.rs](../crates/tkm-hosts/tests/pi_reader.rs); [pi_install.rs](../crates/tkm-hosts/tests/pi_install.rs) |

### 6.1 Integration level per harness

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S6.1-1 | Claude: hooks in settings | met | [install.rs](../crates/tkm-hosts/src/claude/install.rs) |
| S6.1-2 | Codex: hooks.json and per-hook trust | met | [install.rs](../crates/tkm-hosts/src/codex/install.rs) |
| S6.1-3 | agy: user and project hooks.json | met | [install.rs](../crates/tkm-hosts/src/agy/install.rs) |
| S6.1-4 | Cursor: user and project hooks.json | met | [install.rs](../crates/tkm-hosts/src/cursor/install.rs) |
| S6.1-5 | Kimi: [[hooks]] in config.toml | met | [install.rs](../crates/tkm-hosts/src/kimi/install.rs) |
| S6.1-6 | Codewhale: [hooks] in config.toml | met | [install.rs](../crates/tkm-hosts/src/codewhale/install.rs) |
| S6.1-7 | opencode: Bun TypeScript shim | met | [opencode.ts](../shims/opencode.ts) |
| S6.1-8 | Pi: TypeScript extension shim | met | [pi.ts](../shims/pi.ts) |
| S6.1-9 | Unavailable, disabled or failed doctor hook falls back to fold-only | deferred | [0050-conformance-gaps.md](../docs/adr/0050-conformance-gaps.md) |

### 6.2 Events tokenimum handles

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S6.2-1 | session_start read timeout 300 ms | met | [lib.rs](../crates/tkm-proto/src/lib.rs) |
| S6.2-2 | prompt read timeout 1,500 ms | met | [lib.rs](../crates/tkm-proto/src/lib.rs) |
| S6.2-3 | tool_done, turn_end, session_end read timeout 5 ms | met | [lib.rs](../crates/tkm-proto/src/lib.rs) |
| S6.2-4 | claude maps specified event classes and fallbacks | met | [claude_contract.rs](../crates/tkm-hosts/tests/claude_contract.rs) |
| S6.2-5 | codex maps specified event classes and fallbacks | met | [codex_contract.rs](../crates/tkm-hosts/tests/codex_contract.rs) |
| S6.2-6 | agy maps specified event classes and fallbacks | met | [agy_contract.rs](../crates/tkm-hosts/tests/agy_contract.rs) |
| S6.2-7 | cursor maps specified event classes and fallbacks | met | [cursor_contract.rs](../crates/tkm-hosts/tests/cursor_contract.rs) |
| S6.2-8 | kimi maps specified event classes and fallbacks | met | [kimi_contract.rs](../crates/tkm-hosts/tests/kimi_contract.rs) |
| S6.2-9 | codewhale maps specified event classes and fallbacks | met | [codewhale_contract.rs](../crates/tkm-hosts/tests/codewhale_contract.rs) |
| S6.2-10 | opencode maps specified event classes and fallbacks | met | [opencode_contract.rs](../crates/tkm-hosts/tests/opencode_contract.rs) |
| S6.2-11 | pi maps specified event classes and fallbacks | met | [pi_contract.rs](../crates/tkm-hosts/tests/pi_contract.rs) |
| S6.2-12 | No network wait on tool_done; later delivery or drop at turn end | met | [live.rs](../crates/tkmd/tests/live.rs) |
| S6.2-13 | Prompt timeout uses cached default block | met | [inject.rs](../crates/tkmd/tests/inject.rs) |
| S6.2-14 | Cursor and Codewhale defer prompt injection until injectable event | met | [0017-inject-layer.md](../docs/adr/0017-inject-layer.md) |
| S6.2-15 | Missing session_end: 30 minute idle or next session start | deferred | [0018-fold-workers-and-idle-fold.md](../docs/adr/0018-fold-workers-and-idle-fold.md) describes a five-minute grace at next start. |
| S6.2-16 | Host hook timeouts exceed internal read deadlines | met | [contracts.rs](../crates/tkm-hosts/tests/contracts.rs) |
| S6.2-17 | claude: `session_start` → `SessionStart` | met | [claude_contract.rs](../crates/tkm-hosts/tests/claude_contract.rs) |
| S6.2-18 | codex: `session_start` → `SessionStart` | met | [codex_contract.rs](../crates/tkm-hosts/tests/codex_contract.rs) |
| S6.2-19 | agy: `session_start` → `SessionStart` | met | [agy_contract.rs](../crates/tkm-hosts/tests/agy_contract.rs) |
| S6.2-20 | cursor: `session_start` → `sessionStart` | met | [cursor_contract.rs](../crates/tkm-hosts/tests/cursor_contract.rs) |
| S6.2-21 | kimi: `session_start` → `SessionStart` | met | [kimi_contract.rs](../crates/tkm-hosts/tests/kimi_contract.rs) |
| S6.2-22 | codewhale: `session_start` → `session_start` (env only, no stdin, cannot inject) | met | [codewhale_contract.rs](../crates/tkm-hosts/tests/codewhale_contract.rs) |
| S6.2-23 | opencode: `session_start` → `event` `session.created` | met | [opencode_contract.rs](../crates/tkm-hosts/tests/opencode_contract.rs) |
| S6.2-24 | pi: `session_start` → `session_start` | met | [pi_contract.rs](../crates/tkm-hosts/tests/pi_contract.rs) |
| S6.2-25 | claude: `prompt` → `UserPromptSubmit` | met | [claude_contract.rs](../crates/tkm-hosts/tests/claude_contract.rs) |
| S6.2-26 | codex: `prompt` → `UserPromptSubmit` | met | [codex_contract.rs](../crates/tkm-hosts/tests/codex_contract.rs) |
| S6.2-27 | agy: `prompt` → `PreInvocation` (`lastUserInput`, agy ≥ 1.4.7) | met | [agy_contract.rs](../crates/tkm-hosts/tests/agy_contract.rs) |
| S6.2-28 | cursor: `prompt` → `beforeSubmitPrompt` (cannot inject) | met | [cursor_contract.rs](../crates/tkm-hosts/tests/cursor_contract.rs) |
| S6.2-29 | kimi: `prompt` → `UserPromptSubmit` | met | [kimi_contract.rs](../crates/tkm-hosts/tests/kimi_contract.rs) |
| S6.2-30 | codewhale: `prompt` → `message_submit` | met | [codewhale_contract.rs](../crates/tkm-hosts/tests/codewhale_contract.rs) |
| S6.2-31 | opencode: `prompt` → `chat.message` | met | [opencode_contract.rs](../crates/tkm-hosts/tests/opencode_contract.rs) |
| S6.2-32 | pi: `prompt` → `before_agent_start` | met | [pi_contract.rs](../crates/tkm-hosts/tests/pi_contract.rs) |
| S6.2-33 | claude: `tool_done` → `PostToolUse`, `PostToolUseFailure` | met | [claude_contract.rs](../crates/tkm-hosts/tests/claude_contract.rs) |
| S6.2-34 | codex: `tool_done` → `PostToolUse` | met | [codex_contract.rs](../crates/tkm-hosts/tests/codex_contract.rs) |
| S6.2-35 | agy: `tool_done` → `PostToolUse` | met | [agy_contract.rs](../crates/tkm-hosts/tests/agy_contract.rs) |
| S6.2-36 | cursor: `tool_done` → `postToolUse`, `postToolUseFailure` | met | [cursor_contract.rs](../crates/tkm-hosts/tests/cursor_contract.rs) |
| S6.2-37 | kimi: `tool_done` → `PostToolUse`, `PostToolUseFailure` | met | [kimi_contract.rs](../crates/tkm-hosts/tests/kimi_contract.rs) |
| S6.2-38 | codewhale: `tool_done` → `tool_call_after` | met | [codewhale_contract.rs](../crates/tkm-hosts/tests/codewhale_contract.rs) |
| S6.2-39 | opencode: `tool_done` → `tool.execute.after` | met | [opencode_contract.rs](../crates/tkm-hosts/tests/opencode_contract.rs) |
| S6.2-40 | pi: `tool_done` → `tool_result` | met | [pi_contract.rs](../crates/tkm-hosts/tests/pi_contract.rs) |
| S6.2-41 | claude: `turn_end` → `Stop` | met | [claude_contract.rs](../crates/tkm-hosts/tests/claude_contract.rs) |
| S6.2-42 | codex: `turn_end` → `Stop` | met | [codex_contract.rs](../crates/tkm-hosts/tests/codex_contract.rs) |
| S6.2-43 | agy: `turn_end` → `Stop` | met | [agy_contract.rs](../crates/tkm-hosts/tests/agy_contract.rs) |
| S6.2-44 | cursor: `turn_end` → `stop` | met | [cursor_contract.rs](../crates/tkm-hosts/tests/cursor_contract.rs) |
| S6.2-45 | kimi: `turn_end` → `Stop` | met | [kimi_contract.rs](../crates/tkm-hosts/tests/kimi_contract.rs) |
| S6.2-46 | codewhale: `turn_end` → `turn_end` | met | [codewhale_contract.rs](../crates/tkm-hosts/tests/codewhale_contract.rs) |
| S6.2-47 | opencode: `turn_end` → `event` `session.idle` | met | [opencode_contract.rs](../crates/tkm-hosts/tests/opencode_contract.rs) |
| S6.2-48 | pi: `turn_end` → `agent_end` (not `turn_end`, ADR 0025) | met | [pi_contract.rs](../crates/tkm-hosts/tests/pi_contract.rs) |
| S6.2-49 | claude: `session_end` → `SessionEnd` | met | [claude_contract.rs](../crates/tkm-hosts/tests/claude_contract.rs) |
| S6.2-50 | codex: `session_end` → `SessionEnd` | met | [codex_contract.rs](../crates/tkm-hosts/tests/codex_contract.rs) |
| S6.2-51 | agy: `session_end` → **missing**: idle-fold | met | [0018-fold-workers-and-idle-fold.md](../docs/adr/0018-fold-workers-and-idle-fold.md) |
| S6.2-52 | cursor: `session_end` → `sessionEnd` | met | [cursor_contract.rs](../crates/tkm-hosts/tests/cursor_contract.rs) |
| S6.2-53 | kimi: `session_end` → `SessionEnd` | met | [kimi_contract.rs](../crates/tkm-hosts/tests/kimi_contract.rs) |
| S6.2-54 | codewhale: `session_end` → `session_end` (env only) | met | [codewhale_contract.rs](../crates/tkm-hosts/tests/codewhale_contract.rs) |
| S6.2-55 | opencode: `session_end` → `event` `session.deleted`; else idle-fold | met | [opencode_contract.rs](../crates/tkm-hosts/tests/opencode_contract.rs) |
| S6.2-56 | pi: `session_end` → `session_shutdown` | met | [pi_contract.rs](../crates/tkm-hosts/tests/pi_contract.rs) |

### 6.3 Reply shapes

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S6.3-1 | Claude additionalContext hookSpecificOutput; systemMessage notice; empty stdout no-op; 10,000-char cap and no Stop/SessionEnd context | met | [claude_contract.rs](../crates/tkm-hosts/tests/claude_contract.rs) |
| S6.3-2 | Codex Claude-shaped additionalContext on allowed events, systemMessage notice, empty no-op; additionalContextLimit | met | [codex_contract.rs](../crates/tkm-hosts/tests/codex_contract.rs) |
| S6.3-3 | agy injectSteps systemMessage/ephemeralMessage, no notice, {} no-op | met | [agy_contract.rs](../crates/tkm-hosts/tests/agy_contract.rs) |
| S6.3-4 | Cursor additional_context on sessionStart/postToolUse; no notice; {} no-op | met | [cursor_contract.rs](../crates/tkm-hosts/tests/cursor_contract.rs) |
| S6.3-5 | Kimi plain stdout on SessionStart/UserPromptSubmit; no notice, empty no-op | met | [kimi_contract.rs](../crates/tkm-hosts/tests/kimi_contract.rs) |
| S6.3-6 | Codewhale tool_call_before decision allow and additionalContext ≤2,000 chars; empty no-op | met | [codewhale_contract.rs](../crates/tkm-hosts/tests/codewhale_contract.rs) |
| S6.3-7 | opencode chat.message synthetic text part, toast notice, unmodified no-op | met | [opencode_contract.rs](../crates/tkm-hosts/tests/opencode_contract.rs) |
| S6.3-8 | Pi before_agent_start custom message, ui.notify notice, undefined no-op | met | [pi_contract.rs](../crates/tkm-hosts/tests/pi_contract.rs) |
| S6.3-10 | Pi redirects via pi.sendMessage steer | deferred | [0050-conformance-gaps.md](../docs/adr/0050-conformance-gaps.md) |
| S6.3-11 | Never use blocking replies or stop a turn | met | [contracts.rs](../crates/tkm-hosts/tests/contracts.rs) |

### 6.4 Session log readers

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S6.4-20 | claude log path and normalized reader | met | [claude_reader.rs](../crates/tkm-hosts/tests/claude_reader.rs) |
| S6.4-2 | codex log path and normalized reader | met | [codex_reader.rs](../crates/tkm-hosts/tests/codex_reader.rs) |
| S6.4-3 | agy log path and normalized reader | met | [agy_reader.rs](../crates/tkm-hosts/tests/agy_reader.rs) |
| S6.4-4 | cursor log path and normalized reader | met | [cursor_reader.rs](../crates/tkm-hosts/tests/cursor_reader.rs) |
| S6.4-5 | kimi log path and normalized reader | met | [kimi_reader.rs](../crates/tkm-hosts/tests/kimi_reader.rs) |
| S6.4-6 | codewhale log path and normalized reader | met | [codewhale_reader.rs](../crates/tkm-hosts/tests/codewhale_reader.rs) |
| S6.4-7 | opencode log path and normalized reader | met | [opencode_reader.rs](../crates/tkm-hosts/tests/opencode_reader.rs) |
| S6.4-8 | pi log path and normalized reader | met | [pi_reader.rs](../crates/tkm-hosts/tests/pi_reader.rs) |
| S6.4-1 | Legacy opencode storage and export fallback | deferred | [0025-shim-protocol.md](../docs/adr/0025-shim-protocol.md) |
| S6.4-10 | Foreign SQLite opened read only | met | [reader.rs](../crates/tkm-hosts/src/opencode/reader.rs) |
| S6.4-11 | Unknown records counted; &gt;20% skipped yields partial ledger-only fold | met | [fold.rs](../crates/tkmd/tests/fold.rs) |
| S6.4-12 | Claude Code reader source: `<home>/projects/<cwd-slug>/<session>.jsonl` | met | [claude_reader.rs](../crates/tkm-hosts/tests/claude_reader.rs) |
| S6.4-13 | Codex reader source: `<CODEX_HOME>/sessions/YYYY/MM/DD/rollout-<ts>-<uuid>.jsonl`; older files `.jsonl.zst` | met | [codex_reader.rs](../crates/tkm-hosts/tests/codex_reader.rs) |
| S6.4-14 | agy reader source: hook `transcriptPath` = `<home>/.gemini/antigravity-cli/brain/<id>/.system_generated/logs/transcript_full.jsonl` (one JSON step per line, with token counts) | met | [agy_reader.rs](../crates/tkm-hosts/tests/agy_reader.rs) |
| S6.4-15 | Cursor reader source: `transcript_path` in hook payload (also env `CURSOR_TRANSCRIPT_PATH`) | deferred | [cursor_reader.rs](../crates/tkm-hosts/tests/cursor_reader.rs); [e5b-hosts.md](../docs/verified/e5b-hosts.md) |
| S6.4-16 | Kimi Code reader source: `~/.kimi/sessions/<md5(workdir)>/<session-id>/{context.jsonl,wire.jsonl,state.json}` | met | [kimi_reader.rs](../crates/tkm-hosts/tests/kimi_reader.rs) |
| S6.4-17 | DeepSeek harness (Codewhale) reader source: `$CODEWHALE_HOME/sessions/<uuid>.json` | met | [codewhale_reader.rs](../crates/tkm-hosts/tests/codewhale_reader.rs) |
| S6.4-18 | opencode reader source: SQLite `~/.local/share/opencode/opencode.db` (tables `session`, `message`, `part`, ...; path from `opencode db path`); legacy `~/.local/share/opencode/storage/` | deferred | [opencode_reader.rs](../crates/tkm-hosts/tests/opencode_reader.rs); [0025-shim-protocol.md](../docs/adr/0025-shim-protocol.md) |
| S6.4-19 | Pi reader source: `~/.pi/agent/sessions/--<cwd-encoded>--/<timestamp>_<session-id>.jsonl`, header `{type:"session",version:3,...}` | met | [pi_reader.rs](../crates/tkm-hosts/tests/pi_reader.rs) |

### 6.5 Claude Code

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S6.5-1 | User settings.json and project settings.local.json hook installation | met | [claude_install.rs](../crates/tkm-hosts/tests/claude_install.rs) |
| S6.5-2 | Exec-form tkm-hook registration with args and timeouts | met | [install.rs](../crates/tkm-hosts/src/claude/install.rs) |
| S6.5-3 | Map session, prompt, tool, timing and subagent fields | met | [claude_contract.rs](../crates/tkm-hosts/tests/claude_contract.rs) |
| S6.5-4 | SessionEnd queues detached fold | met | [0018-fold-workers-and-idle-fold.md](../docs/adr/0018-fold-workers-and-idle-fold.md) |
| S6.5-5 | Prune via .claude/settings.local.json permissions.deny | met | [claude_install.rs](../crates/tkm-hosts/tests/claude_install.rs); [deny.rs](../crates/tkm-hosts/src/writer/deny.rs) |
| S6.5-6 | Trust and Stop/SessionEnd behavior researched | met | [claude-codex.md](../docs/verified/claude-codex.md) |

### 6.6 Codex

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S6.6-1 | hooks.json settings and shell-string handlers | met | [codex_install.rs](../crates/tkm-hosts/tests/codex_install.rs) |
| S6.6-2 | Event payload and reply mapping | met | [codex_contract.rs](../crates/tkm-hosts/tests/codex_contract.rs) |
| S6.6-3 | Print /hooks trust step; never write trusted_hash or bypass flag | met | [install.rs](../crates/tkm-hosts/src/codex/install.rs) |
| S6.6-4 | Trusted hooks under codex exec; untrusted fallback | deferred | [claude-codex.md](../docs/verified/claude-codex.md) verifies host behavior; [0050-conformance-gaps.md](../docs/adr/0050-conformance-gaps.md) notes missing automatic fallback. |
| S6.6-5 | Built-in tool pruning report only | met | [mod.rs](../crates/tkm-hosts/src/codex/mod.rs) |

### 6.7 Antigravity (`agy`)

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S6.7-1 | Global ~/.gemini/config/hooks.json and project .agents/hooks.json, own flat tokenimum namespace | met | [agy_install.rs](../crates/tkm-hosts/tests/agy_install.rs) |
| S6.7-2 | Map PreInvocation first invocation, PostToolUse and lifecycle; no session_end | met | [agy_contract.rs](../crates/tkm-hosts/tests/agy_contract.rs) |
| S6.7-3 | Payload fields and optional tool events under agy -p | met | [e5b-hosts.md](../docs/verified/e5b-hosts.md) |
| S6.7-4 | One-second hook timeouts, three seconds for prompt | met | [install.rs](../crates/tkm-hosts/src/agy/install.rs) |
| S6.7-5 | Per-repository pruning report only | met | [mod.rs](../crates/tkm-hosts/src/agy/mod.rs) |
| S6.7-6 | Gemini CLI hooks not assumed to apply | not applicable | Separate product, not the agy adapter; [e5b-hosts.md](../docs/verified/e5b-hosts.md). |

### 6.8 Cursor (agent and CLI)

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S6.8-1 | User and project hooks.json versioned flat format | met | [cursor_install.rs](../crates/tkm-hosts/tests/cursor_install.rs) |
| S6.8-10 | Map six Cursor lifecycle and tool events | met | [cursor_contract.rs](../crates/tkm-hosts/tests/cursor_contract.rs) |
| S6.8-3 | beforeSubmitPrompt records prompt; defer injection to postToolUse | met | [inject.rs](../crates/tkmd/tests/inject.rs) |
| S6.8-4 | sessionStart content repeated on next postToolUse | deferred | [0050-conformance-gaps.md](../docs/adr/0050-conformance-gaps.md); deferred-injection behavior is not proof of repetition. |
| S6.8-5 | Probe Cursor session and switch missing events to idle-fold | deferred | [0050-conformance-gaps.md](../docs/adr/0050-conformance-gaps.md) |
| S6.8-6 | Explicit hook timeout and trust instructions | met | [cursor_install.rs](../crates/tkm-hosts/tests/cursor_install.rs) |
| S6.8-7 | Cursor deny path .cursor/cli.json permissions.deny | deferred | [0032-cli-prune-gaps.md](../docs/adr/0032-cli-prune-gaps.md) records proposal-only non-Claude writers. |
| S6.8-8 | Detect Claude hook duplicates and warn | met | [cursor_install.rs](../crates/tkm-hosts/tests/cursor_install.rs); [server.rs](../crates/tkmd/tests/server.rs) |
| S6.8-2 | Transcript format remains forum-reported | deferred | [e5b-hosts.md](../docs/verified/e5b-hosts.md) |

### 6.9 Kimi Code

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S6.9-1 | User [[hooks]] TOML, event, command and timeout | met | [kimi_install.rs](../crates/tkm-hosts/tests/kimi_install.rs) |
| S6.9-2 | Map all five event types and payload | met | [kimi_contract.rs](../crates/tkm-hosts/tests/kimi_contract.rs) |
| S6.9-3 | Plain stdout injection never exit 2; source currently ignores stdout | met | [e5b-hosts.md](../docs/verified/e5b-hosts.md); [kimi_contract.rs](../crates/tkm-hosts/tests/kimi_contract.rs) |
| S6.9-4 | No project/plugin hooks, pruning report only | met | [0020-e5b-host-adapters.md](../docs/adr/0020-e5b-host-adapters.md) |

### 6.10 DeepSeek harness

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S6.10-1 | Codewhale (codewhale / legacy deepseek) target; dsh future adapter | met | [e5b-hosts.md](../docs/verified/e5b-hosts.md); [mod.rs](../crates/tkm-hosts/src/codewhale/mod.rs) |
| S6.10-8 | User config [[hooks.hooks]], project hook path | deferred | [0020-e5b-host-adapters.md](../docs/adr/0020-e5b-host-adapters.md) records user-only installer. |
| S6.10-3 | Six lifecycle/tool events plus session_idle | deferred | [0050-conformance-gaps.md](../docs/adr/0050-conformance-gaps.md) |
| S6.10-4 | Map DEEPSEEK env markers and stdin | met | [codewhale_contract.rs](../crates/tkm-hosts/tests/codewhale_contract.rs) |
| S6.10-5 | Only tool_call_before additionalContext, caps and no blocking | met | [codewhale_contract.rs](../crates/tkm-hosts/tests/codewhale_contract.rs) |
| S6.10-6 | continue_on_error true and explicit timeouts | met | [install.rs](../crates/tkm-hosts/src/codewhale/install.rs) |
| S6.10-7 | Pruning report only | met | [mod.rs](../crates/tkm-hosts/src/codewhale/mod.rs) |

| S6.10-2 | Official DeepSeek Harness (dsh) is a separate future adapter | not applicable | Separate future adapter, outside the specified Codewhale target; [verified hosts](../docs/verified/e5b-hosts.md). |

### 6.11 opencode and Pi (shim level)

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S6.11-8 | --no-shims uses fold-only and optional instruction/skill injection | met | [fold_only_mode.rs](../crates/tkm-hosts/tests/fold_only_mode.rs) |
| S6.11-10 | opencode shim generated ≤40 lines, no runtime dependencies, forwards with Bun and timeouts | met | [shim.rs](../crates/tkm-hosts/tests/shim.rs) |
| S6.11-3 | Pi shim generated ≤40 lines, no runtime dependencies, forwards via pi.exec | met | [shim.rs](../crates/tkm-hosts/tests/shim.rs) |
| S6.11-4 | Shim header, hash, doctor verification and owned uninstall | met | [shim.rs](../crates/tkm-hosts/tests/shim.rs); [doctor_checks.rs](../crates/tkm-cli/tests/doctor_checks.rs) |
| S6.11-5 | opencode and Pi user and project install paths and trust | met | [opencode_install.rs](../crates/tkm-hosts/tests/opencode_install.rs); [pi_install.rs](../crates/tkm-hosts/tests/pi_install.rs) |
| S6.11-6 | opencode session.created, chat.message, tool.execute.after, session.idle, session.deleted | met | [opencode_contract.rs](../crates/tkm-hosts/tests/opencode_contract.rs) |
| S6.11-7 | Pi session_start, before_agent_start, tool_result, agent_end, session_shutdown | met | [pi_contract.rs](../crates/tkm-hosts/tests/pi_contract.rs) |
| S6.11-2 | Pi async session_shutdown awaited by host | deferred | [opencode-pi.md](../docs/verified/opencode-pi.md) documents missing runtime probe. |
| S6.11-9 | opencode permission and Pi defaultTools deny targets | deferred | [0032-cli-prune-gaps.md](../docs/adr/0032-cli-prune-gaps.md) |

| S6.11-1 | Pi session shutdown forwards a best-effort session end | met | [Pi shim](../shims/pi.ts); [Pi contract](../crates/tkm-hosts/tests/pi_contract.rs). |

### 6.12 Fold-only fallback

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S6.12-8 | Watch logs; fold after 30 minutes idle; scan every 60 seconds if watch unavailable | met | [0018-fold-workers-and-idle-fold.md](../docs/adr/0018-fold-workers-and-idle-fold.md) |
| S6.12-9 | Optional marked instructions block, regeneration and git exclude | met | [fold_only_instructions.rs](../crates/tkm-hosts/tests/fold_only_instructions.rs) |
| S6.12-1 | agy instructions path | deferred | [0026-fold-only-mode.md](../docs/adr/0026-fold-only-mode.md) records no verified agy path. |
| S6.12-4 | Generated tokenimum skill for each harness | deferred | [0026-fold-only-mode.md](../docs/adr/0026-fold-only-mode.md) lists hosts without verified skill paths. |
| S6.12-5 | tkm run passes session/host env and folds at exit | met | [run_wrapper.rs](../crates/tkm-cli/tests/run_wrapper.rs) |
| S6.12-6 | No live loop guard in fold-only | met | [fold_only_mode.rs](../crates/tkm-hosts/tests/fold_only_mode.rs) |
| S6.12-2 | Discover stored sessions across hosts without transcript path | deferred | [0018-fold-workers-and-idle-fold.md](../docs/adr/0018-fold-workers-and-idle-fold.md) documents Claude-only discovery. |

### 6.13 Settings writers and install steps per harness

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S6.13-1 | claude target, ownership-aware writer and trust instructions | met | [claude_install.rs](../crates/tkm-hosts/tests/claude_install.rs) |
| S6.13-2 | codex target, ownership-aware writer and trust instructions | met | [codex_install.rs](../crates/tkm-hosts/tests/codex_install.rs) |
| S6.13-3 | agy target, ownership-aware writer and trust instructions | met | [agy_install.rs](../crates/tkm-hosts/tests/agy_install.rs) |
| S6.13-4 | cursor target, ownership-aware writer and trust instructions | met | [cursor_install.rs](../crates/tkm-hosts/tests/cursor_install.rs) |
| S6.13-5 | kimi target, ownership-aware writer and trust instructions | met | [kimi_install.rs](../crates/tkm-hosts/tests/kimi_install.rs) |
| S6.13-6 | codewhale target, ownership-aware writer and trust instructions | met | [codewhale_install.rs](../crates/tkm-hosts/tests/codewhale_install.rs) |
| S6.13-7 | opencode target, ownership-aware writer and trust instructions | met | [opencode_install.rs](../crates/tkm-hosts/tests/opencode_install.rs) |
| S6.13-8 | pi target, ownership-aware writer and trust instructions | met | [pi_install.rs](../crates/tkm-hosts/tests/pi_install.rs) |
| S6.13-9 | Preserve foreign entries, first-write backup, refuse escaping symlinks, idempotence | met | [writer.rs](../crates/tkm-hosts/tests/writer.rs); [writer_text.rs](../crates/tkm-hosts/tests/writer_text.rs) |
| S6.13-10 | Claude Code install: `<home>/settings.json` `hooks`; JSON merge; none (user settings) | met | [claude_install.rs](../crates/tkm-hosts/tests/claude_install.rs) |
| S6.13-11 | Codex install: `<CODEX_HOME>/hooks.json`; JSON merge; run `codex`, then `/hooks`, trust tokenimum entries | met | [codex_install.rs](../crates/tkm-hosts/tests/codex_install.rs) |
| S6.13-12 | agy install: `<home>/.gemini/config/hooks.json`, top-level `tokenimum`; JSON merge; `/hooks` lists them; trust rule **to verify** | met | [agy_install.rs](../crates/tkm-hosts/tests/agy_install.rs) |
| S6.13-13 | Cursor install: `~/.cursor/hooks.json`; JSON merge; restart Cursor/agent; project hooks need workspace trust | met | [cursor_install.rs](../crates/tkm-hosts/tests/cursor_install.rs) |
| S6.13-14 | Kimi Code install: `~/.kimi/config.toml` `[[hooks]]`; `toml_edit`, comments kept; none documented | met | [kimi_install.rs](../crates/tkm-hosts/tests/kimi_install.rs) |
| S6.13-15 | DeepSeek harness install: `~/.codewhale/config.toml` `[[hooks.hooks]]`; `toml_edit`; project hooks: `/hooks approve` | met | [codewhale_install.rs](../crates/tkm-hosts/tests/codewhale_install.rs) |
| S6.13-16 | opencode install: `~/.config/opencode/plugins/tokenimum.ts`; generated shim; none (plugins auto-load) | met | [opencode_install.rs](../crates/tkm-hosts/tests/opencode_install.rs) |
| S6.13-17 | Pi install: `~/.pi/agent/extensions/tokenimum.ts`; generated shim; `/reload` in Pi; project scope needs project trust | met | [pi_install.rs](../crates/tkm-hosts/tests/pi_install.rs) |

## 7. Mechanisms

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S7-1 | Facts, injection, pruning, loop guard and ledger | met | [lib.rs](../crates/tkm-core/src/lib.rs) |

### 7.1 Repository facts

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S7.1-1 | Observe commands, classify intent/form, normalize rtk and outcome | met | [facts_te.rs](../crates/tkm-core/tests/facts_te.rs) |
| S7.1-2 | Extract form repair candidate family | met | [facts_te.rs](../crates/tkm-core/tests/facts_te.rs) |
| S7.1-3 | Extract boundary candidate family | met | [facts_te.rs](../crates/tkm-core/tests/facts_te.rs) |
| S7.1-4 | Extract empty probe candidate family | met | [facts_te.rs](../crates/tkm-core/tests/facts_te.rs) |
| S7.1-5 | Extract argument repair candidate family | met | [facts_te.rs](../crates/tkm-core/tests/facts_te.rs) |
| S7.1-6 | Extract env value candidate family | met | [facts_te.rs](../crates/tkm-core/tests/facts_te.rs) |
| S7.1-7 | Batch stable_&lt;id&gt; and safe_&lt;id&gt; noul filters on session candidates | deferred | [0050-conformance-gaps.md](../docs/adr/0050-conformance-gaps.md) |
| S7.1-8 | Batch contradicts_&lt;id&gt; for exercised facts; retain code contradictions | deferred | [0050-conformance-gaps.md](../docs/adr/0050-conformance-gaps.md) |
| S7.1-9 | Promote at 3, decay 0.8, contradiction weight 5, high-stakes kill, fire gate 40% of 4 | met | [facts_te.rs](../crates/tkm-core/tests/facts_te.rs) |
| S7.1-10 | Neutral scope shared; host scope limited to harness | met | [facts_review.rs](../crates/tkm-core/tests/facts_review.rs) |
| S7.1-11 | Render templates without model-generated text | met | [facts_safety.rs](../crates/tkm-core/tests/facts_safety.rs) |
| S7.1-12 | Offline/budget code-only facts need extra confirmation | met | [facts_te.rs](../crates/tkm-core/tests/facts_te.rs) |

### 7.2 Context budget and relevance (not in TE)

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S7.2-1 | Candidates: active host-scoped facts, selected skills, optional MEMORY.md pointers | met | [0017-inject-layer.md](../docs/adr/0017-inject-layer.md) |
| S7.2-2 | Dedupe normalized hash and token Jaccard ≥ 0.8 | met | [inject_assembly.rs](../crates/tkm-core/tests/inject_assembly.rs) |
| S7.2-3 | One rel_&lt;id&gt; score request using four relevance criteria | met | [injection.rs](../crates/tkmd/src/injection.rs) |
| S7.2-4 | Score threshold 1.5, 1.0 for fire-gated facts; default max 600 tokens | met | [inject_assembly.rs](../crates/tkm-core/tests/inject_assembly.rs) |
| S7.2-5 | Cache by prompt and candidates for 10 minutes | met | [inject.rs](../crates/tkm-core/src/inject.rs) |
| S7.2-6 | Without otherskill facts and memory only; never drop existing context | met | [inject.rs](../crates/tkmd/tests/inject.rs) |

### 7.3 Tool and skill pruning

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S7.3-1 | Count per-repository and harness loaded/used by prompt and session | met | [prune_rules.rs](../crates/tkm-core/tests/prune_rules.rs) |
| S7.3-2 | Candidate after 10 prompts across 2 sessions or 5 sessions | met | [prune_rules.rs](../crates/tkm-core/tests/prune_rules.rs) |
| S7.3-3 | Write after 60 idle minutes or 24-hour latest deadline | deferred | [0032-cli-prune-gaps.md](../docs/adr/0032-cli-prune-gaps.md) |
| S7.3-4 | off, report, propose, apply modes | met | [mod.rs](../crates/tkm-core/src/prune/mod.rs) |
| S7.3-5 | Claude .claude/settings.local.json permissions.deny | met | [deny.rs](../crates/tkm-hosts/src/writer/deny.rs) |
| S7.3-6 | Cursor .cursor/cli.json permissions.deny | deferred | [0032-cli-prune-gaps.md](../docs/adr/0032-cli-prune-gaps.md) |
| S7.3-7 | opencode opencode.json permission.&lt;tool&gt; | deferred | [0032-cli-prune-gaps.md](../docs/adr/0032-cli-prune-gaps.md) |
| S7.3-8 | Pi project defaultTools -name | deferred | [0032-cli-prune-gaps.md](../docs/adr/0032-cli-prune-gaps.md) |
| S7.3-9 | Codex, agy, Kimi, Codewhale report-only pruning | met | [contracts.rs](../crates/tkm-hosts/tests/contracts.rs) |
| S7.3-10 | Own entries only; preserve foreign rules, git exclude, refuse tracked and unsafe symlinks | met | [writer.rs](../crates/tkm-hosts/tests/writer.rs) |
| S7.3-11 | Protect TE and host tools, headroom, prune.protect and conditional orchestrator tools | deferred | [0032-cli-prune-gaps.md](../docs/adr/0032-cli-prune-gaps.md) |
| S7.3-12 | Delegated run in last 30 days caps pruning at propose | deferred | [0032-cli-prune-gaps.md](../docs/adr/0032-cli-prune-gaps.md) |
| S7.3-13 | Per-repository skills loaded versus invoked, export ranking prior | deferred | [0050-conformance-gaps.md](../docs/adr/0050-conformance-gaps.md) |
| S7.3-14 | Never delete or hide a skill | met | [skills.rs](../crates/tkm-core/src/prune/skills.rs) |

### 7.4 Loop guard

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S7.4-1 | Track calls, errors, file edits, verification and repeat failures including shell edits | met | [loopguard_behavior.rs](../crates/tkm-core/tests/loopguard_behavior.rs) |
| S7.4-2 | Gate after 18 calls on repeat or 8 errors, with 5-sample envelope | met | [loopguard_behavior.rs](../crates/tkm-core/tests/loopguard_behavior.rs) |
| S7.4-3 | Async judge one in flight, at most one per five calls | met | [0016-live-layer-and-decision-queue.md](../docs/adr/0016-live-layer-and-decision-queue.md) |
| S7.4-4 | Batch stuck/progress noul and redirect choice | met | [live.rs](../crates/tkmd/src/live.rs) |
| S7.4-5 | Apply t_stuck, t_progress and t_low verdict thresholds | met | [turn.rs](../crates/tkm-core/src/loopguard/turn.rs) |
| S7.4-6 | Code backstop when rule-stuck and &gt;2.5 times envelope | met | [loopguard_behavior.rs](../crates/tkm-core/tests/loopguard_behavior.rs) |
| S7.4-7 | Deliver templated redirect at next injectable event | met | [live.rs](../crates/tkmd/tests/live.rs) |
| S7.4-8 | Escalate further stuck segments via notice, orchestrator or stored event/command | met | [0016-live-layer-and-decision-queue.md](../docs/adr/0016-live-layer-and-decision-queue.md) |
| S7.4-9 | Good turns feed last 500 samples; p90 × 1.25 | met | [envelope.rs](../crates/tkm-core/src/loopguard/envelope.rs) |
| S7.4-10 | Never stop a turn | met | [contracts.rs](../crates/tkm-hosts/tests/contracts.rs) |

### 7.5 Ledger and savings

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S7.5-1 | Per-session token classes and pricing.toml, weekly refreshed from OpenRouter/vendor | deferred | [0050-conformance-gaps.md](../docs/adr/0050-conformance-gaps.md) |
| S7.5-2 | Record per-decision tokens, cost, model, latency and questions | met | [live.rs](../crates/tkmd/src/live.rs) |
| S7.5-3 | Per-session decision-cost overhead | met | [e9_spend.rs](../crates/tkmd/tests/e9_spend.rs) |
| S7.5-4 | Savings by repo and host against shadow/pre-action baseline; samples and confidence band | deferred | [0050-conformance-gaps.md](../docs/adr/0050-conformance-gaps.md) explains zero priced session costs. |
| S7.5-5 | Flag shadow sessions for comparisons | met | [live.rs](../crates/tkmd/tests/live.rs) |

## 8. Decision client (`tkm-decide`)

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S8-1 | Decision client contract | met | [client.rs](../crates/tkm-decide/src/client.rs) |

### 8.1 Endpoint

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S8.1-1 | POST /api/alpha/decisions request and response including usage | met | [api.rs](../crates/tkm-decide/tests/api.rs) |
| S8.1-2 | noul, choice and score typed criteria and answers | met | [api.rs](../crates/tkm-decide/tests/api.rs) |
| S8.1-3 | Implement fallback chain locally | met | [client.rs](../crates/tkm-decide/tests/client.rs) |

### 8.2 Model chain

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S8.2-1 | Configurable free Mercury probe slot, paid dated Mercury, pplx flash and Jev | met | [chain.rs](../crates/tkm-decide/tests/chain.rs) |
| S8.2-2 | Pin dated versions, reject latest aliases | met | [chain.rs](../crates/tkm-decide/tests/chain.rs) |
| S8.2-3 | Daily free model re-probe and promotion | deferred | [0050-conformance-gaps.md](../docs/adr/0050-conformance-gaps.md) |
| S8.2-4 | 404/5xx/1,500 ms timeout/invalid answer fallback; three failures in five minutes skip 15 | met | [client.rs](../crates/tkm-decide/tests/client.rs); [chain.rs](../crates/tkm-decide/tests/chain.rs) |
| S8.2-5 | No calibration: record answer, take no action | met | [live.rs](../crates/tkmd/tests/live.rs) |
| S8.2-6 | decide.enabled=false or offline preset uses code-only behavior | met | [e9_settings.rs](../crates/tkmd/tests/e9_settings.rs) |

### 8.3 Probe result (2026-10-29)

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S8.3-1 | Historical model response/latency/cost sample | not applicable | Historical observation rather than a product obligation; [e3-openrouter-decisions.md](../docs/verified/e3-openrouter-decisions.md). |

### 8.4 Request rules

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S8.4-10 | Code-built digest, prompt first 500 chars, last 12 calls, arguments ≤120, hard cap 16 KB | met | [digest.rs](../crates/tkm-decide/tests/digest.rs) |
| S8.4-2 | Redact env values, keys, dotenv, outside paths before request | met | [redact.rs](../crates/tkm-decide/tests/redact.rs) |
| S8.4-3 | Batch purpose questions; deny data collection and allow fallbacks | met | [client.rs](../crates/tkm-decide/tests/client.rs) |
| S8.4-4 | Cache model/state/questions: relevance 10 min, fact filter 24 h | met | [cache.rs](../crates/tkm-decide/tests/cache.rs) |
| S8.4-5 | Daily $0.50 and session $0.02 budget; code-only over cap | met | [budget.rs](../crates/tkm-decide/tests/budget.rs); [client.rs](../crates/tkm-decide/tests/client.rs) |
| S8.4-1 | Retain cap spend across daemon restart | deferred | [0015-daemon-architecture-and-flush.md](../docs/adr/0015-daemon-architecture-and-flush.md) |
| S8.4-7 | Top shows daily and session caps | met | [tui.rs](../crates/tkm-ui/src/tui.rs) |
| S8.4-8 | OPENROUTER_API_KEY or mode-600 openrouter.env | met | [key.rs](../crates/tkm-decide/tests/key.rs) |
| S8.4-9 | Optional key reuse only after consent, without copying | met | [integrations_wiring.rs](../crates/tkm-cli/tests/integrations_wiring.rs) |

### 8.5 Calibration

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S8.5-7 | 14-day or 200-turn initial shadow period | deferred | [0050-conformance-gaps.md](../docs/adr/0050-conformance-gaps.md) |
| S8.5-2 | Label loop outcomes and fact confirmation/contradiction | deferred | [0045-offline-decision-bench.md](../docs/adr/0045-offline-decision-bench.md) notes fact/relevance label contracts are missing. |
| S8.5-3 | Fit per-model threshold with ≥0.9 stuck precision and minimum recall | met | [calibrate_ops.rs](../crates/tkm-cli/tests/calibrate_ops.rs) |
| S8.5-1 | Agreement: two models in parallel, act only on consensus | deferred | [0016-live-layer-and-decision-queue.md](../docs/adr/0016-live-layer-and-decision-queue.md) |
| S8.5-5 | Offline cached fixture replay and model comparison | met | [replay_ops.rs](../crates/tkm-cli/tests/replay_ops.rs) |
| S8.5-6 | Defaults t_stuck .85, t_progress .30, t_low .30, t_high .80 | met | [calibrate.rs](../crates/tkm-decide/tests/calibrate.rs) |

## 9. Optional integrations

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S9-1 | Standalone operation with two optional adapters | met | [matrix.rs](../crates/tkm-integrations/tests/matrix.rs) |

### 9.1 Detection

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S9.1-1 | Detect superorchestrator PATH, config directory, version ≤1 s | met | [e9.rs](../crates/tkm-integrations/tests/e9.rs) |
| S9.1-2 | Detect otherskill PATH, global/project config, help ≤1 s | met | [e9.rs](../crates/tkm-integrations/tests/e9.rs) |
| S9.1-3 | integrations.superorchestrator and integrations.otherskill: auto/on/off | met | [config.rs](../crates/tkm-config/src/config.rs) |
| S9.1-4 | Detect at daemon start/init and every 10 minutes | met | [runtime.rs](../crates/tkm-integrations/src/runtime.rs) |
| S9.1-5 | Disable failing adapter for 15 minutes; log and keep standalone | met | [e9.rs](../crates/tkm-integrations/tests/e9.rs) |
| S9.1-6 | Status and doctor show adapter state and reason | met | [integrations_wiring.rs](../crates/tkm-cli/tests/integrations_wiring.rs) |

### 9.2 Behaviour in the four combinations

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S9.2-1 | Standalone: all detected homes, facts/memory, escalation, local spend, propose pruning | met | [matrix.rs](../crates/tkm-integrations/tests/matrix.rs) |
| S9.2-2 | Standalone plus otherskill: select, dedupe, shared budget, ranking prior, consented key reuse | deferred | [0050-conformance-gaps.md](../docs/adr/0050-conformance-gaps.md) notes incomplete skill evidence. |
| S9.2-3 | Orchestrated: account homes, delegated detection, escalation and spend report | deferred | [0040-e9-integrations.md](../docs/adr/0040-e9-integrations.md) item 5 documents missing spend ingestion. |
| S9.2-4 | Both adapters enabled, fall back on removal | met | [matrix.rs](../crates/tkm-integrations/tests/matrix.rs) |

### 9.3 superorchestrator adapter

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S9.3-6 | --superorchestrator reads accounts/*.toml and installs known account homes idempotently | met | [integrations_wiring.rs](../crates/tkm-cli/tests/integrations_wiring.rs) |
| S9.3-2 | Detect delegated runs by env run ID or cached run worktree | met | [e9.rs](../crates/tkm-integrations/tests/e9.rs) |
| S9.3-3 | Send structured escalation via integration; never cancel run | met | [0040-e9-integrations.md](../docs/adr/0040-e9-integrations.md) |
| S9.3-4 | Choice suggests continue/redirect/cancel/reroute from digest and routing level | met | [live.rs](../crates/tkmd/src/live.rs) |
| S9.3-1 | Report each decision cost under tokenimum-decisions so routing sees spend | deferred | [0040-e9-integrations.md](../docs/adr/0040-e9-integrations.md) item 5: superorchestrator ingestion needed. |

### 9.4 otherskill adapter

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S9.4-1 | Call otherskill select with literal agent/prompt arguments | deferred | [0040-e9-integrations.md](../docs/adr/0040-e9-integrations.md) item 7 limits selection to four supported host names. |
| S9.4-2 | Ask whether tokenimum owns merged block, dedupe by skill name; otherwise facts only | met | [integrations_wiring.rs](../crates/tkm-cli/tests/integrations_wiring.rs) |
| S9.4-3 | Export loaded-vs-used evidence as ranking prior | deferred | [0050-conformance-gaps.md](../docs/adr/0050-conformance-gaps.md) |
| S9.4-4 | Separate model chain from otherskill free models | met | [0040-e9-integrations.md](../docs/adr/0040-e9-integrations.md) |

### 9.5 Repository identity across worktrees

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S9.5-1 | Normalize origin URL; share facts/envelopes/evidence between worktrees | met | [repo_paths.rs](../crates/tkm-config/tests/repo_paths.rs) |
| S9.5-2 | Keep deny lists per checkout | met | [migrations.rs](../crates/tkm-store/src/migrations.rs) |
| S9.5-3 | Parse .git/config and gitdir, honor GIT_CEILING_DIRECTORIES and stop at HOME | met | [repo_paths.rs](../crates/tkm-config/tests/repo_paths.rs) |

## 10. Storage and configuration

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S10-1 | Storage and layered configuration architecture | met | [lib.rs](../crates/tkm-config/src/lib.rs); [lib.rs](../crates/tkm-store/src/lib.rs) |

### 10.1 Storage

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S10.1-1 | One WAL SQLite database at ~/.local/share/tokenimum/tokenimum.db | met | [paths.rs](../crates/tkm-config/src/paths.rs); [lib.rs](../crates/tkm-store/src/lib.rs) |
| S10.1-2 | repos table stores repository identity | met | [schema.sql](../crates/tkm-store/tests/snapshots/schema.sql) |
| S10.1-3 | sessions table stores host/home/session and fold status | met | [schema.sql](../crates/tkm-store/tests/snapshots/schema.sql) |
| S10.1-4 | turns table stores turn calls and outcome | met | [schema.sql](../crates/tkm-store/tests/snapshots/schema.sql) |
| S10.1-5 | facts table stores fact lifecycle and evidence | met | [schema.sql](../crates/tkm-store/tests/snapshots/schema.sql) |
| S10.1-6 | tool_evidence table stores tool usage counts | met | [schema.sql](../crates/tkm-store/tests/snapshots/schema.sql) |
| S10.1-7 | denylist_applied table stores checkout deny writes | met | [schema.sql](../crates/tkm-store/tests/snapshots/schema.sql) |
| S10.1-8 | envelopes table stores good-turn lengths | met | [schema.sql](../crates/tkm-store/tests/snapshots/schema.sql) |
| S10.1-9 | decisions table stores model, questions, answers, cost and labels | met | [schema.sql](../crates/tkm-store/tests/snapshots/schema.sql) |
| S10.1-10 | calibration table stores model/purpose thresholds and quality | met | [schema.sql](../crates/tkm-store/tests/snapshots/schema.sql) |
| S10.1-11 | events table stores action explanations and evidence | met | [schema.sql](../crates/tkm-store/tests/snapshots/schema.sql) |
| S10.1-12 | ledger table stores usage, priced cost and overhead | met | [schema.sql](../crates/tkm-store/tests/snapshots/schema.sql) |
| S10.1-13 | installs table stores owned files and hashes | met | [schema.sql](../crates/tkm-store/tests/snapshots/schema.sql) |
| S10.1-14 | Versioned migrations and stable table/column compatibility surface | met | [migrations.rs](../crates/tkm-store/tests/migrations.rs) |
| S10.1-15 | errors.log rotation at 5 MB | met | [logging.rs](../crates/tkmd/src/logging.rs) |
| S10.1-16 | $XDG_RUNTIME_DIR/tokenimum/tkmd.sock mode 600 | met | [paths.rs](../crates/tkm-config/src/paths.rs); [server.rs](../crates/tkmd/tests/server.rs) |

### 10.2 Layered configuration

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S10.2-1 | Precedence defaults, preset, global, repo, repo-local, environment, CLI | met | [config.rs](../crates/tkm-config/tests/config.rs) |
| S10.2-2 | Global ~/.config/tokenimum/config.toml | met | [paths.rs](../crates/tkm-config/src/paths.rs) |
| S10.2-3 | Repository .tokenimum.toml and .tokenimum.local.toml; local excluded | met | [entries_cli.rs](../crates/tkm-config/tests/entries_cli.rs) |
| S10.2-4 | TKM_&lt;SECTION&gt;_&lt;NAME&gt; environment overrides | met | [config.rs](../crates/tkm-config/tests/config.rs) |
| S10.2-5 | TKM_DISABLE=1 hook no-op without contacting daemon | met | [integration.rs](../crates/tkm-hook/tests/integration.rs) |
| S10.2-6 | TKM_CONFIG selects global config path | met | [config.rs](../crates/tkm-config/tests/config.rs) |
| S10.2-7 | Repo config cannot raise budgets, change API key path, enable apply or set escalate command | met | [config.rs](../crates/tkm-config/tests/config.rs) |

### 10.3 Presets

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S10.3-1 | observe: facts learned, injection off, loop shadow, prune report, decisions shadow | met | [config.rs](../crates/tkm-config/tests/config.rs) |
| S10.3-2 | balanced: default, facts on, calibrated redirects, prune propose, injection on | met | [config.rs](../crates/tkm-config/tests/config.rs) |
| S10.3-3 | aggressive: 2 confirmations, aggressive redirect, apply, agreement off, 900 tokens | met | [config.rs](../crates/tkm-config/tests/config.rs) |
| S10.3-4 | offline: code-only facts, rule guard, propose, decisions off, facts-only injection | met | [config.rs](../crates/tkm-config/tests/config.rs) |

### 10.4 Per-mechanism switches

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S10.4-1 | facts.enabled | deferred | [ADR 0050](../docs/adr/0050-conformance-gaps.md) |
| S10.4-2 | inject.enabled | met | [config.rs](../crates/tkm-config/src/config.rs) |
| S10.4-3 | inject.max_tokens | met | [config.rs](../crates/tkm-config/src/config.rs) |
| S10.4-4 | inject.memory | met | [config.rs](../crates/tkm-config/src/config.rs) |
| S10.4-5 | inject.instructions_file | met | [config.rs](../crates/tkm-config/src/config.rs) |
| S10.4-6 | loop.mode: off/shadow/redirect | met | [config.rs](../crates/tkm-config/src/config.rs) |
| S10.4-7 | prune.mode: off/report/propose/apply | met | [config.rs](../crates/tkm-config/src/config.rs) |
| S10.4-8 | ledger.enabled | deferred | [ADR 0050](../docs/adr/0050-conformance-gaps.md) |
| S10.4-9 | decide.enabled | met | [config.rs](../crates/tkm-config/src/config.rs) |
| S10.4-10 | decide.chain and budget settings | met | [config.rs](../crates/tkm-config/src/config.rs) |
| S10.4-11 | hosts.claude.enabled and per-host level auto/hooks/shim/fold-only | deferred | [ADR 0050](../docs/adr/0050-conformance-gaps.md) |
| S10.4-12 | integrations.superorchestrator and integrations.otherskill | met | [config.rs](../crates/tkm-config/src/config.rs) |
| S10.4-13 | ui.web.enabled and ui.web.bind | met | [config.rs](../crates/tkm-config/src/config.rs) |
| S10.4-14 | decide.agreement | deferred | [0016-live-layer-and-decision-queue.md](../docs/adr/0016-live-layer-and-decision-queue.md) |
| S10.4-15 | tkm config show [--effective] | met | [config_ops.rs](../crates/tkm-cli/tests/config_ops.rs) |
| S10.4-16 | tkm config get, set, unset [--global/--repo/--local] | met | [config_ops.rs](../crates/tkm-cli/tests/config_ops.rs) |
| S10.4-17 | tkm config preset &lt;name&gt; [--repo] | met | [config_ops.rs](../crates/tkm-cli/tests/config_ops.rs) |
| S10.4-18 | tkm config explain &lt;key&gt; | met | [config_ops.rs](../crates/tkm-cli/tests/config_ops.rs) |
| S10.4-19 | tkm config edit [--global/--repo] | met | [config_ops.rs](../crates/tkm-cli/tests/config_ops.rs) |
| S10.4-20 | tkm config diff | met | [config_ops.rs](../crates/tkm-cli/tests/config_ops.rs) |
| S10.4-21 | hosts.claude.enabled and hosts.claude.level | deferred | [0050-conformance-gaps.md](../docs/adr/0050-conformance-gaps.md) |
| S10.4-22 | hosts.codex.enabled and hosts.codex.level | deferred | [0050-conformance-gaps.md](../docs/adr/0050-conformance-gaps.md) |
| S10.4-23 | hosts.agy.enabled and hosts.agy.level | deferred | [0050-conformance-gaps.md](../docs/adr/0050-conformance-gaps.md) |
| S10.4-24 | hosts.cursor.enabled and hosts.cursor.level | deferred | [0050-conformance-gaps.md](../docs/adr/0050-conformance-gaps.md) |
| S10.4-25 | hosts.kimi.enabled and hosts.kimi.level | deferred | [0050-conformance-gaps.md](../docs/adr/0050-conformance-gaps.md) |
| S10.4-26 | hosts.deepseek.enabled and hosts.deepseek.level | deferred | [0050-conformance-gaps.md](../docs/adr/0050-conformance-gaps.md) |
| S10.4-27 | hosts.opencode.enabled and hosts.opencode.level | deferred | [0050-conformance-gaps.md](../docs/adr/0050-conformance-gaps.md) |
| S10.4-28 | hosts.pi.enabled and hosts.pi.level | deferred | [0050-conformance-gaps.md](../docs/adr/0050-conformance-gaps.md) |
| S10.4-29 | decide.budget_daily_usd = 0.50 | met | [config.rs](../crates/tkm-config/src/config.rs) |
| S10.4-30 | decide.budget_session_usd = 0.02 | met | [config.rs](../crates/tkm-config/src/config.rs) |

## 11. Usability, monitoring and explanations

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S11-1 | CLI, monitor and dashboard usability | met | [cli.rs](../crates/tkm-cli/src/cli.rs) |

### 11.1 One-command install: `tkm init`

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S11.1-1 | Detect binaries and homes for all eight harnesses | met | [init_lifecycle.rs](../crates/tkm-cli/tests/init_lifecycle.rs) |
| S11.1-2 | Detect integrations and show plan before writes | met | [integrations_wiring.rs](../crates/tkm-cli/tests/integrations_wiring.rs) |
| S11.1-3 | Ask once for key, key reuse, or offline | met | [init_lifecycle.rs](../crates/tkm-cli/tests/init_lifecycle.rs) |
| S11.1-4 | Install hooks/shims and service, start daemon and run doctor | met | [init_lifecycle.rs](../crates/tkm-cli/tests/init_lifecycle.rs) |
| S11.1-5 | Print Codex, Codewhale and Pi trust steps | met | [init_lifecycle.rs](../crates/tkm-cli/tests/init_lifecycle.rs) |
| S11.1-6 | init flag --yes | met | [cli.rs](../crates/tkm-cli/src/cli.rs) |
| S11.1-7 | init flag --dry-run | met | [cli.rs](../crates/tkm-cli/src/cli.rs) |
| S11.1-8 | init flag --only | met | [cli.rs](../crates/tkm-cli/src/cli.rs) |
| S11.1-9 | init flag --home --host | met | [cli.rs](../crates/tkm-cli/src/cli.rs) |
| S11.1-10 | init flag --superorchestrator | met | [cli.rs](../crates/tkm-cli/src/cli.rs) |
| S11.1-11 | init flag --no-shims | met | [cli.rs](../crates/tkm-cli/src/cli.rs) |
| S11.1-12 | init flag --preset | met | [cli.rs](../crates/tkm-cli/src/cli.rs) |
| S11.1-13 | Uninstall reverses recorded owned changes | met | [uninstall_lifecycle.rs](../crates/tkm-cli/tests/uninstall_lifecycle.rs); [0033-init-reversal-manifest.md](../docs/adr/0033-init-reversal-manifest.md) |

### 11.2 `tkm status`

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S11.2-1 | Current repository or --all; --json equivalent | met | [status_report.rs](../crates/tkm-cli/tests/status_report.rs) |
| S11.2-2 | Daemon version, uptime and adapter state/reasons | met | [status_report.rs](../crates/tkm-cli/tests/status_report.rs) |
| S11.2-3 | Harness level, last event and missing events | met | [status_report.rs](../crates/tkm-cli/tests/status_report.rs) |
| S11.2-4 | Facts active, candidates and recent retired | met | [status_report.rs](../crates/tkm-cli/tests/status_report.rs) |
| S11.2-5 | Loop mode, envelope, 7-day redirects/escalations | met | [status_report.rs](../crates/tkm-cli/tests/status_report.rs) |
| S11.2-6 | Pruning mode and pending proposals | met | [status_report.rs](../crates/tkm-cli/tests/status_report.rs) |
| S11.2-7 | Decision spend and savings sample sizes | met | [status_report.rs](../crates/tkm-cli/tests/status_report.rs) |
| S11.2-8 | Last five action explanations and IDs | met | [status_report.rs](../crates/tkm-cli/tests/status_report.rs) |

### 11.3 Live monitor: `tkm top`

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S11.3-1 | Sessions pane | deferred | [ADR 0050](../docs/adr/0050-conformance-gaps.md) item 20. |
| S11.3-2 | Decisions pane | met | [monitor.rs](../crates/tkm-ui/tests/monitor.rs); [tui.rs](../crates/tkm-ui/src/tui.rs) |
| S11.3-3 | Spend pane | deferred | [ADR 0050](../docs/adr/0050-conformance-gaps.md) item 20. |
| S11.3-4 | Savings pane | met | [monitor.rs](../crates/tkm-ui/tests/monitor.rs); [tui.rs](../crates/tkm-ui/src/tui.rs) |
| S11.3-5 | Facts pane | met | [monitor.rs](../crates/tkm-ui/tests/monitor.rs); [tui.rs](../crates/tkm-ui/src/tui.rs) |
| S11.3-6 | Redirects pane | deferred | [ADR 0050](../docs/adr/0050-conformance-gaps.md) item 20. |
| S11.3-7 | Tab switches panes, ? help, Enter opens why | met | [tui.rs](../crates/tkm-ui/src/tui.rs) |
| S11.3-8 | Push stream; read-only DB fallback when daemon down | met | [0035-monitor-ui-architecture.md](../docs/adr/0035-monitor-ui-architecture.md) |

### 11.4 Optional web dashboard: `tkm dashboard`

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S11.4-1 | web feature off by default, axum server | met | [Cargo.toml](../crates/tkm-ui/Cargo.toml); [web.rs](../crates/tkmd/tests/web.rs) |
| S11.4-2 | 127.0.0.1-only, random per-start token in URL fragment | met | [0036-web-dashboard-security.md](../docs/adr/0036-web-dashboard-security.md) |
| S11.4-3 | Top panes, history charts, fact and pruning evidence | met | [web.rs](../crates/tkm-ui/tests/web.rs) |
| S11.4-4 | Only forget, restore, preset actions with confirmation | met | [web.rs](../crates/tkmd/tests/web.rs) |
| S11.4-5 | Embedded assets, no CDN or external requests | met | [web.rs](../crates/tkm-ui/tests/web.rs) |

### 11.5 Plain-language explanations: `tkm why <event>`

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S11.5-1 | Store action evidence IDs and explanation template; why event/last/session | met | [why_report.rs](../crates/tkm-cli/tests/why_report.rs) |
| S11.5-2 | Templates for fact injection/promotion/retirement, redirect, escalation, proposal, write, restore, budget and adapter disabled | met | [explain_templates.rs](../crates/tkm-core/tests/explain_templates.rs) |
| S11.5-3 | Templates are code, never model output | met | [explain.rs](../crates/tkm-core/src/explain.rs) |

## 12. CLI reference

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S12-1 | tkm init [flags] | met | [init_lifecycle.rs](../crates/tkm-cli/tests/init_lifecycle.rs) |
| S12-2 | tkm status [--all] [--json] | met | [status_report.rs](../crates/tkm-cli/tests/status_report.rs) |
| S12-3 | tkm top | met | [ui_commands.rs](../crates/tkm-cli/tests/ui_commands.rs) |
| S12-4 | tkm dashboard | met | [ui_commands.rs](../crates/tkm-cli/tests/ui_commands.rs) |
| S12-5 | tkm why &lt;event/last&gt; [--session id] | met | [why_report.rs](../crates/tkm-cli/tests/why_report.rs) |
| S12-6 | tkm facts [--all/--brief] and facts forget &lt;id&gt; | met | [facts_report.rs](../crates/tkm-cli/tests/facts_report.rs) |
| S12-7 | tkm prune [apply/restore [tool]] | deferred | [prune_ops.rs](../crates/tkm-cli/tests/prune_ops.rs); [0032-cli-prune-gaps.md](../docs/adr/0032-cli-prune-gaps.md) |
| S12-8 | tkm skills [--json] | deferred | [skills_wiring.rs](../crates/tkm-cli/tests/skills_wiring.rs); [0050-conformance-gaps.md](../docs/adr/0050-conformance-gaps.md) |
| S12-9 | tkm config ... | met | [config_ops.rs](../crates/tkm-cli/tests/config_ops.rs) |
| S12-10 | tkm models [probe] | met | [models_ops.rs](../crates/tkm-cli/tests/models_ops.rs) |
| S12-11 | tkm calibrate [--purpose loop/facts/relevance] | deferred | [calibrate_ops.rs](../crates/tkm-cli/tests/calibrate_ops.rs); [0032-cli-prune-gaps.md](../docs/adr/0032-cli-prune-gaps.md) |
| S12-12 | tkm replay &lt;session-or-fixture&gt; | met | [replay_ops.rs](../crates/tkm-cli/tests/replay_ops.rs) |
| S12-13 | tkm run &lt;harness&gt; [args...] | met | [run_wrapper.rs](../crates/tkm-cli/tests/run_wrapper.rs) |
| S12-14 | tkm doctor [--probe] | deferred | [doctor_checks.rs](../crates/tkm-cli/tests/doctor_checks.rs); [0050-conformance-gaps.md](../docs/adr/0050-conformance-gaps.md) |
| S12-15 | tkm export [--show-digests] | met | [export_report.rs](../crates/tkm-cli/tests/export_report.rs) |
| S12-16 | tkm uninstall [--host h] | met | [uninstall_lifecycle.rs](../crates/tkm-cli/tests/uninstall_lifecycle.rs) |
| S12-17 | tokenimum read-only skill for each harness, no effect claim | deferred | [0026-fold-only-mode.md](../docs/adr/0026-fold-only-mode.md) lists hosts without verified skill directories. |

## 13. Language and safety policy: entirely safe Rust

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S13-1 | Own Rust code forbids unsafe; dependencies audited | met | [ci.yml](../.github/workflows/ci.yml) |

### 13.1 Own code

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S13.1-1 | Every crate root forbids unsafe; workspace lint forbid | met | [check_forbid_unsafe.py](../scripts/check_forbid_unsafe.py); [Cargo.toml](../Cargo.toml) |
| S13.1-2 | CI checks crate attributes and builds | met | [ci.yml](../.github/workflows/ci.yml) |
| S13.1-3 | No own build.rs compiles C | met | [Cargo.toml](../Cargo.toml) |
| S13.1-4 | Pedantic warnings reviewed; unwrap and expect denied; missing docs denied in libraries | met | [Cargo.toml](../Cargo.toml); [lib.rs](../crates/tkm-core/src/lib.rs) |
| S13.1-5 | Hook catches panics and returns no-op | met | [unit.rs](../crates/tkm-hook/tests/unit.rs) |

### 13.2 Dependencies

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S13.2-1 | cargo-deny advisories, allowed licenses, bans and crates.io sources | met | [deny.toml](../deny.toml); [ci.yml](../.github/workflows/ci.yml) |
| S13.2-2 | cargo-geiger report for dependency changes, allowlist unsafe and fail unjustified diff | met | [geiger_diff.py](../scripts/geiger_diff.py); [ci.yml](../.github/workflows/ci.yml) |
| S13.2-3 | Prefer crates forbidding unsafe; reject unsafe crates without 12 months maintenance | deferred | [unsafe-allowlist.md](../docs/unsafe-allowlist.md) states the initial review is not a complete maintenance audit. |
| S13.2-4 | Daily cargo audit; Cargo.lock committed | met | [audit.yml](../.github/workflows/audit.yml); [Cargo.lock](../Cargo.lock) |
| S13.2-5 | Static musl release with repeat-build reproducibility check | met | [0047-linux-static-release-builds.md](../docs/adr/0047-linux-static-release-builds.md); [release.yml](../.github/workflows/release.yml) |

### 13.3 The TypeScript exception

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S13.3-1 | Only generated opencode and Pi shims, ≤40 lines, no dependencies/logic; doctor hash, uninstall, --no-shims | met | [shim.rs](../crates/tkm-hosts/tests/shim.rs); [0025-shim-protocol.md](../docs/adr/0025-shim-protocol.md) |

### 13.4 Runtime safety and privacy

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S13.4-1 | Fail open and log errors | met | [faults.rs](../crates/tkmd/tests/faults.rs); [integration.rs](../crates/tkm-hook/tests/integration.rs) |
| S13.4-2 | Never stop turn, cancel run or delete skill | met | [contracts.rs](../crates/tkm-hosts/tests/contracts.rs); [0040-e9-integrations.md](../docs/adr/0040-e9-integrations.md) |
| S13.4-3 | Preserve foreign settings and back up before writing | met | [writer.rs](../crates/tkm-hosts/tests/writer.rs) |
| S13.4-4 | Redact digests and export with --show-digests | met | [export_report.rs](../crates/tkm-cli/tests/export_report.rs) |
| S13.4-5 | Only templates and validated facts; reject completion/verification advice and outside paths | met | [facts_safety.rs](../crates/tkm-core/tests/facts_safety.rs) |
| S13.4-6 | Validate command, env name, host and test-label parameters; never inject free output | met | [facts_redaction.rs](../crates/tkm-core/tests/facts_redaction.rs) |
| S13.4-7 | Treat repository config as untrusted | met | [config.rs](../crates/tkm-config/tests/config.rs) |
| S13.4-8 | Local socket and token-protected local dashboard | met | [web.rs](../crates/tkmd/tests/web.rs); [server.rs](../crates/tkmd/tests/server.rs) |

## 14. Performance budget

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S14-12 | tool_done hook p50 &lt;3 ms, p99 &lt;10 ms | met | [0046-performance-gates.md](../docs/adr/0046-performance-gates.md) |
| S14-2 | daemon tool_done &lt;200 µs | met | [0046-performance-gates.md](../docs/adr/0046-performance-gates.md) |
| S14-3 | prompt scored &lt;1,500 ms, cached &lt;5 ms | met | [0046-performance-gates.md](../docs/adr/0046-performance-gates.md) |
| S14-4 | opencode/Pi shim overhead p50 &lt;15 ms, p99 &lt;40 ms | met | [0046-performance-gates.md](../docs/adr/0046-performance-gates.md) |
| S14-5 | tool_done shim spawn without await when no reply pending | deferred | [0025-shim-protocol.md](../docs/adr/0025-shim-protocol.md) states tool_done is awaited. |
| S14-6 | Session fold &lt;500 ms CPU excluding decisions | met | [0046-performance-gates.md](../docs/adr/0046-performance-gates.md) |
| S14-7 | Idle memory &lt;30 MB, &lt;40 MB with web | met | [0046-performance-gates.md](../docs/adr/0046-performance-gates.md) |
| S14-8 | Idle CPU ~0; event driven and 60s fallback scan | met | [0046-performance-gates.md](../docs/adr/0046-performance-gates.md) |
| S14-9 | tkm top refresh &lt;50 ms/frame | met | [0046-performance-gates.md](../docs/adr/0046-performance-gates.md) |
| S14-10 | Criterion/hyperfine CI fixtures and &gt;20% regression gate | met | [0046-performance-gates.md](../docs/adr/0046-performance-gates.md); [bench.yml](../.github/workflows/bench.yml) |
| S14-1 | E10.6 replay benchmark harness pending controller merge | deferred | [0050-conformance-gaps.md](../docs/adr/0050-conformance-gaps.md) item 6; controller will replace placeholder. |

## 15. Milestones

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S15-1 | M0: 50 recorded Claude and Codex sessions, real model comparisons and report | deferred | [0045-offline-decision-bench.md](../docs/adr/0045-offline-decision-bench.md) says sessions and cached answers are synthetic. |
| S15-18 | M1: hook, daemon, store, config, Claude and shadow loop | met | [e2e_shadow.rs](../crates/tkm-cli/tests/e2e_shadow.rs) |
| S15-3 | M1: p99 hook &lt;10 ms and safe-Rust CI policy | met | [0046-performance-gates.md](../docs/adr/0046-performance-gates.md); [ci.yml](../.github/workflows/ci.yml) |
| S15-4 | M1: shadow decisions logged on real sessions | deferred | [0050-conformance-gaps.md](../docs/adr/0050-conformance-gaps.md) item 4 documents fixture-based test. |
| S15-5 | M2: facts and ledger for Claude and Codex | deferred | [0050-conformance-gaps.md](../docs/adr/0050-conformance-gaps.md) items 1 and 12. |
| S15-6 | M2: facts match TE fixtures; ledger within 2% of cost | deferred | [0050-conformance-gaps.md](../docs/adr/0050-conformance-gaps.md) items 1 and 16. |
| S15-7 | M3: init, status, top, why, presets and switches | deferred | [0050-conformance-gaps.md](../docs/adr/0050-conformance-gaps.md) item 15: switches not applied. |
| S15-8 | M3: fresh machine one-command install and every action explainable | deferred | [0050-conformance-gaps.md](../docs/adr/0050-conformance-gaps.md) item 17. |
| S15-9 | M4: agy, Cursor, Kimi, Codewhale hooks and idle-fold | met | [0020-e5b-host-adapters.md](../docs/adr/0020-e5b-host-adapters.md) |
| S15-10 | M4: fold a real session of each; doctor probe reports events; verifications closed or documented | deferred | [0050-conformance-gaps.md](../docs/adr/0050-conformance-gaps.md) items 5 and 10. |
| S15-11 | M5: opencode/Pi shims ≤40 lines, hash-checked and fold-only with --no-shims | met | [shim.rs](../crates/tkm-hosts/tests/shim.rs); [fold_only_e2e.rs](../crates/tkm-hosts/tests/fold_only_e2e.rs) |
| S15-12 | M6: duplicate-free injection under 600 tokens, protected tools and four integration combinations | met | [inject_assembly.rs](../crates/tkm-core/tests/inject_assembly.rs); [matrix.rs](../crates/tkm-integrations/tests/matrix.rs); [protected.rs](../crates/tkm-integrations/tests/protected.rs) |
| S15-13 | M6: otherskill loaded-vs-used evidence | deferred | [0050-conformance-gaps.md](../docs/adr/0050-conformance-gaps.md) item 2. |
| S15-14 | M7: account-home installation and delegated escalation | met | [e9.rs](../crates/tkm-integrations/tests/e9.rs) |
| S15-2 | M7: spend visible in superorchestrator status | deferred | [0040-e9-integrations.md](../docs/adr/0040-e9-integrations.md) item 5. |
| S15-16 | M8: per-purpose calibrated decisions and stuck precision ≥0.9 on labelled turns | deferred | [0045-offline-decision-bench.md](../docs/adr/0045-offline-decision-bench.md) describes synthetic labels and missing fact/relevance contracts. |
| S15-17 | M8: dashboard local-only with token | met | [web.rs](../crates/tkmd/tests/web.rs) |

## 16. Testing

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S16-1 | Table-driven core unit tests and ported TE behavioral tests | met | [facts_te.rs](../crates/tkm-core/tests/facts_te.rs); [ledger_te.rs](../crates/tkm-core/tests/ledger_te.rs) |
| S16-2 | Golden replay per eight hosts with expected actions in JSON | deferred | [0050-conformance-gaps.md](../docs/adr/0050-conformance-gaps.md) item 16. |
| S16-3 | Recorded payload-to-reply contract tests for each host | met | [contracts.rs](../crates/tkm-hosts/tests/contracts.rs); [pi_contract.rs](../crates/tkm-hosts/tests/pi_contract.rs) |
| S16-4 | Installer tests preserve foreign entries byte-for-byte and restore | met | [writer.rs](../crates/tkm-hosts/tests/writer.rs); [cursor_install.rs](../crates/tkm-hosts/tests/cursor_install.rs) |
| S16-5 | Claude and Codex parity against TE on same logs | deferred | [0050-conformance-gaps.md](../docs/adr/0050-conformance-gaps.md) item 16. |
| S16-6 | Integration matrix using fake binaries for all four combinations | met | [matrix.rs](../crates/tkm-integrations/tests/matrix.rs) |
| S16-7 | Recorded decision responses; opt-in live model tests TKM_LIVE=1 | met | [live.rs](../crates/tkm-decide/tests/live.rs); [client.rs](../crates/tkm-decide/tests/client.rs) |
| S16-8 | Fault-inject daemon down, slow socket, 404/500/timeout, locked DB, corrupt log, foreign SQLite lock and shim timeout; each noop and log | deferred | [0050-conformance-gaps.md](../docs/adr/0050-conformance-gaps.md) item 19. |
| S16-9 | CI forbid-unsafe check, cargo-deny and cargo-geiger diff | met | [ci.yml](../.github/workflows/ci.yml) |

## 17. Open questions

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S17-1 | 1: TypeScript shim exception and default mode | met | [0001-open-question-defaults.md](../docs/adr/0001-open-question-defaults.md); [0025-shim-protocol.md](../docs/adr/0025-shim-protocol.md) |
| S17-2 | 2: DeepSeek harness identity | met | [0001-open-question-defaults.md](../docs/adr/0001-open-question-defaults.md); [e5b-hosts.md](../docs/verified/e5b-hosts.md) |
| S17-3 | 3: SQLite/TLS unsafe dependency policy | met | [0001-open-question-defaults.md](../docs/adr/0001-open-question-defaults.md); [unsafe-allowlist.md](../docs/unsafe-allowlist.md) |
| S17-4 | 4: otherskill selection hand-off and one injector | met | [0001-open-question-defaults.md](../docs/adr/0001-open-question-defaults.md); [0040-e9-integrations.md](../docs/adr/0040-e9-integrations.md) |
| S17-5 | 5: orchestrator event/spend API and daemon supervisor | deferred | [0001-open-question-defaults.md](../docs/adr/0001-open-question-defaults.md); [0040-e9-integrations.md](../docs/adr/0040-e9-integrations.md) notes missing spend ingestion. |
| S17-6 | 6: port TE logic with attribution, reimplement I/O | met | [0001-open-question-defaults.md](../docs/adr/0001-open-question-defaults.md); [NOTICE](../NOTICE) |
| S17-7 | 7: agreement mode default and availability | deferred | [0001-open-question-defaults.md](../docs/adr/0001-open-question-defaults.md); [0016-live-layer-and-decision-queue.md](../docs/adr/0016-live-layer-and-decision-queue.md) records missing daemon wiring. |
| S17-8 | 8: Cursor, agy, Kimi, Codex behavior verification | deferred | [0001-open-question-defaults.md](../docs/adr/0001-open-question-defaults.md); [e5b-hosts.md](../docs/verified/e5b-hosts.md) records unobserved agy tool events. |
| S17-9 | 9: committed instruction-file injection opt-in | met | [0001-open-question-defaults.md](../docs/adr/0001-open-question-defaults.md); [fold_only_instructions.rs](../crates/tkm-hosts/tests/fold_only_instructions.rs) |

## 18. To-verify register

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S18-1 | Claude trust, Stop/SessionEnd fields and permissions syntax | met | [claude-codex.md](../docs/verified/claude-codex.md) |
| S18-2 | Codex exec trust and deny keys | met | [claude-codex.md](../docs/verified/claude-codex.md) |
| S18-3 | agy reply, trust, permissions, transcript and DB schema | met | [e5b-hosts.md](../docs/verified/e5b-hosts.md) |
| S18-4 | agy PreToolUse/PostToolUse under agy -p | deferred | [e5b-hosts.md](../docs/verified/e5b-hosts.md) records events unobserved in probes. |
| S18-5 | Cursor CLI coverage, timeout, injection and third-party hooks | met | [e5b-hosts.md](../docs/verified/e5b-hosts.md) |
| S18-6 | Cursor transcript format | deferred | [e5b-hosts.md](../docs/verified/e5b-hosts.md) records forum-only evidence. |
| S18-7 | Kimi project hooks, plugins and stdout injection | met | [e5b-hosts.md](../docs/verified/e5b-hosts.md) |
| S18-8 | Codewhale identity and session format | met | [e5b-hosts.md](../docs/verified/e5b-hosts.md) |
| S18-9 | opencode timeouts, toast, session.created source and permission keys | met | [opencode-pi.md](../docs/verified/opencode-pi.md) |
| S18-10 | Pi notify API and handler timeouts | met | [opencode-pi.md](../docs/verified/opencode-pi.md) |
| S18-11 | Pi async session_shutdown runtime behavior | deferred | [opencode-pi.md](../docs/verified/opencode-pi.md) records that runtime probe is needed. |
| S18-12 | Gemini CLI hooks apply to agy | deferred | [0050-conformance-gaps.md](../docs/adr/0050-conformance-gaps.md) item 18. |

## 19. References

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S19-1 | External references list | not applicable | Bibliography is informational rather than an implementation requirement; [SPEC.md](../SPEC.md). |
