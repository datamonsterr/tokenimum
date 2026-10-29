# Spec conformance audit (incomplete)

This is a partial audit, not a conformance determination. Sections and requirements not listed
below have not yet been audited. The checker is deliberately not enabled in CI until the remaining
SPEC headings have tables and their requirements have been reviewed. Evidence links identify the
implementation or the documented limitation; the presence of a source file alone is not proof
that every requirement in its section is met.

## Gaps found

These requirements lack a documented deferral in the pre-existing ADRs:

- SPEC 7.5: “Per session: tokens by class (input, cache write, cache read, output) from the session log; priced with a local price table (`pricing.toml`, refreshed weekly from OpenRouter `/api/v1/models` and harness vendor prices).” The fold stores `priced_cost: 0.0` in [fold.rs](../crates/tkmd/src/fold.rs); pricing exists in core but is not connected there.
- SPEC 7.3: “count skills loaded (injected or listed) vs invoked, per repository.” [skills.rs](../crates/tkm-cli/src/cmd/skills.rs) reconstructs up to 30 recent sessions, only from Claude logs; loaded counts depend on host catalog data. This does not establish complete per-repository evidence across hosts.

## Pending

E10.6 replay benchmark harness is being merged in parallel. Its conformance evidence is pending;
the controller will replace the placeholder when that work lands.

### 6.4 Session log readers

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S6.4-1 | opencode legacy storage and export fallback | deferred | [ADR 0025](../docs/adr/0025-shim-protocol.md) explains missing legacy fixtures; [reader](../crates/tkm-hosts/src/opencode/reader.rs) rejects the legacy directory. |

### 6.8 Cursor (agent and CLI)

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S6.8-1 | Install Cursor hooks in versioned flat format | met | [Cursor installer](../crates/tkm-hosts/src/cursor/install.rs); [installer tests](../crates/tkm-hosts/tests/cursor_install.rs); [ADR 0020](../docs/adr/0020-e5b-host-adapters.md). |
| S6.8-2 | Read Cursor transcript format | deferred | [verified host notes](../docs/verified/e5b-hosts.md) explain that the format is forum-reported and the fixture is synthetic; [reader](../crates/tkm-hosts/src/cursor/reader.rs). |

### 6.10 DeepSeek harness

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S6.10-1 | Target Codewhale as the specified DeepSeek harness | met | [Codewhale adapter](../crates/tkm-hosts/src/codewhale/mod.rs); [contract tests](../crates/tkm-hosts/tests/codewhale_contract.rs). |
| S6.10-2 | Official DeepSeek Harness (`dsh`) is a separate future adapter | not applicable | The specification explicitly identifies `dsh` as a future, separate adapter; [verified host notes](../docs/verified/e5b-hosts.md). |

### 6.11 opencode and Pi (shim level)

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S6.11-1 | Pi session shutdown forwards a best-effort session end | met | [Pi shim](../shims/pi.ts); [Pi contract tests](../crates/tkm-hosts/tests/pi_contract.rs). |
| S6.11-2 | Confirm whether Pi awaits shutdown async work | deferred | [Pi verification](../docs/verified/opencode-pi.md) explains that upstream docs do not specify this and a runtime probe is needed. |

### 6.12 Fold-only fallback

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S6.12-1 | agy instructions-file injection | deferred | [ADR 0026](../docs/adr/0026-fold-only-mode.md) explains that its instructions-file path is unverified; [implementation](../crates/tkm-hosts/src/fold_only/instructions.rs). |
| S6.12-2 | Recover stored sessions without a live log path across hosts | deferred | [ADR 0018](../docs/adr/0018-fold-workers-and-idle-fold.md) records Claude-only discovery; [fold worker](../crates/tkmd/src/fold.rs). |

### 8.4 Request rules

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S8.4-1 | Preserve daily decision spend for cap checks after restart | deferred | [ADR 0015](../docs/adr/0015-daemon-architecture-and-flush.md) explains that earlier spend is not reloaded; [ledger](../crates/tkmd/src/decider.rs). |

### 8.5 Calibration

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S8.5-1 | Agreement mode asks two models and acts only on agreement | deferred | [ADR 0016](../docs/adr/0016-live-layer-and-decision-queue.md) records that daemon wiring is absent; [client](../crates/tkm-decide/src/client.rs) exposes the two-model primitive. |

### 9.3 superorchestrator adapter

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S9.3-1 | Decision spend visible to superorchestrator | deferred | [ADR 0040](../docs/adr/0040-e9-integrations.md) item 5 explains the missing ingestion API and follow-up. |

## 14. Performance budget

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S14-1 | Replay benchmark harness | deferred | [pending note](../docs/spec-conformance.md#pending): E10.6 is being merged in parallel; controller will supply evidence. |

## 15. Milestones

| ID | Requirement | Status | Evidence |
|---|---|---|---|
| S15-1 | M0: benchmark 50 recorded sessions and compare real decision models | deferred | [ADR 0045](../docs/adr/0045-offline-decision-bench.md) explains that sessions and cached model answers are synthetic. |
| S15-2 | M7: spend visible in superorchestrator status | deferred | [ADR 0040](../docs/adr/0040-e9-integrations.md) item 5 documents the external ingestion follow-up. |
