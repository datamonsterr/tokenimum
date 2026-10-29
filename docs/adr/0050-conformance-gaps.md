# ADR 0050: Conformance audit gaps and outstanding verification

Status: refreshed 2026-10-30 after decision-calibration and agreement implementation, based on
`da6a44d` on `epic/e10` (baseline CI green; new Rust checks pending controller CI). This record documents
observed gaps; it does not change SPEC.md or assert that unfinished work satisfies the spec.
The row-level status is in [spec-conformance.md](../spec-conformance.md): 504 met, 41 deferred,
11 not applicable. Each deferred row carries one category: (a) needs live or real-world data or
user action, (b) intentionally out of scope or limited per an ADR, (c) not implemented.

## Closed since the first audit

The original items 1, 2, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17 (fresh-install part), 19 and 20 are
fixed and now `met` in the conformance table, with tests named there:

- Priced ledger, weekly pricing refresh, nullable costs and restart spend, and skill loaded-vs-used
  storage: ADR 0049.
- Pi steer delivery, Codewhale `session_idle`, Cursor repeat of blocks on `postToolUse`.
- Doctor fold-only fallback, `doctor --probe` with per-event idle-fold, and the daily free-model
  scheduler: ADR 0051.
- Batched fact filter and contradiction decisions in fold, the 14-day/200-turn shadow window, and
  the `[facts]`, `[ledger]` and `[hosts.*]` switches.
- Monitor panes (model share, sparkline, turn id, triggering calls).
- Golden replay for eight hosts, TE parity on Claude and Codex logs, the fault matrix, and the
  fresh-install CI job.

## Decision-calibration and agreement closures (C5, C6)

S8.5-2, S12-11 and S15-16 now have outcome-label and per-purpose calibration contracts.
Loop labels retain interruption/backstop = `stuck`, good completion after further calls =
`progress`, and unknown outcomes remain unlabelled. Fact batches use a JSON object in the
existing `decisions.outcome_label` column, keyed by question name. Later exercised sessions
confirm a held fact; code failures and later calibrated contradiction verdicts contradict it.
`contradicts_<id>` reverses validity-label polarity. A fact prediction cannot label itself.
Relevance uses the same exercised-item outcomes within its owning session, including explicit
skill invocations and successful/failed reads of a memory pointer. Merely injecting an item
supplies no label. Batched items retain separate labels. No schema migration is needed.

`tkm calibrate --purpose loop|facts|relevance` groups by response model version and purpose.
Facts calibrate `fact_filter` and `fact_contradiction` separately. Loop stuck/progress and
fact stability/safety fit the complete threshold pair. Relevance normalizes the 0..3 score,
measures both admission paths, and never lowers the SPEC 7.2 cutoffs. Every fit needs at least
20 labelled items, precision >=0.9 and recall >=0.5. SPEC states 0.9 for stuck and gives no
separate numeric precision target for facts/relevance, so this implementation conservatively
uses 0.9 for those too. The daemon requires the measured quality for the exact model/purpose;
unscored puts revoke authorization, and another purpose cannot satisfy its shadow window.
The existing minimum elapsed days and judged-turn requirements remain in effect.

Evidence: `crates/tkm-decide/tests/labels.rs`, the durable label test in
`crates/tkmd/src/fold.rs`, `crates/tkm-cli/tests/calibrate_ops.rs`, and
`crates/tkmd/tests/purpose_calibration.rs`. `fixtures/bench/outcomes.json` adds 24 complete,
labelled synthetic Claude/Codex turns, three qualifying test-double model versions and a tied
control model that cannot meet the target. The CI regression fits each purpose, verifies the
quality bound, rejects the control, and checks that lowering stored precision revokes action.
These are plumbing and policy checks; representative live-model quality remains unmeasured.

S8.5-1, S10.4-14 and S17-7 now use live parallel agreement. `Decider::decide_agreement` reaches
the budgeted client's concurrent primary + Jev requests (a custom chain without Jev uses its
first two slots). Both actual model versions need qualifying loop calibration, matching
threshold verdicts, and, for a redirect, the same valid redirect choice. Disagreement, either
failure, duplicate response versions, ambiguity or missing calibration yields no model action;
the deterministic guard remains available. Both successful answers are stored, a
`decide.agreement` event records consensus or its absence, and each transport attempt is
charged to both session and daily spend through the existing ledger. Agreement is off by
default, per ADR 0001 item 7, and is enabled by `decide.agreement`.

Evidence: `crates/tkmd/tests/agreement.rs` uses a transport barrier so sequential requests
cannot pass; it covers consensus, redirect/verdict disagreement, one transport failure and
missing peer calibration. `agreement_prefers_jev_over_the_fallback_and_charges_both_costs` in
`crates/tkm-decide/tests/client.rs` checks selection, both costs and the subsequent budget gate.
The settings regression checks the default and explicit enablement. Rust execution is reserved
for controller CI; locally, formatting, workspace metadata and the conformance checker are run.

## Open (a): needs live or real-world data or user action

1. Representative repeat-repository outcome and savings/spend measurement (SPEC 2 G1, G4); see ADR
   0045 and ADR 0046. Synthetic fixtures and CI benchmarks cannot establish it.
2. M0 over 50 real sessions (SPEC 15 M0) and M1 shadow decisions on real sessions (M1); the
   end-to-end test uses `tkm init` and fixtures (`crates/tkm-cli/tests/e2e_shadow.rs`).
3. M4 real sessions for Cursor, Kimi and Codewhale, and the open items in
   `docs/verified/e5b-hosts.md`: Cursor transcript format, agy `-p` tool events (SPEC 18).
4. Pi async `session_shutdown` runtime behavior (`docs/verified/opencode-pi.md`).
5. Gemini CLI hook applicability to agy (SPEC 18); the implementation assumes the products do not
   share hooks.
6. Ledger within 2% of billed cost and stuck precision on real labelled turns (SPEC 15 M2, M8).
7. E10.6 replay benchmark: harness merged (ADR 0048); the sample run needs private real history.
8. SPEC 13.2 dependency maintenance audit by a person.

## Open (b): intentionally out of scope per ADR

- Superorchestrator spend ingestion (SPEC 4.2, 9.2, 9.3, 15 M7, 17 Q5): ADR 0040 item 5. tokenimum
  writes the JSONL sink; ingestion needs an upstream change.
- Connect timeout 10 ms instead of 2 ms: ADR 0014.
- Legacy opencode `storage/` tree and awaited shim `tool_done`: ADR 0025.
- Kimi and Codewhale user-scope-only installers: ADR 0020.
- Instructions file and skill path for hosts without a verified location (agy, Codex, Cursor,
  Codewhale skills): ADR 0026.
- otherskill selection limited to four host names: ADR 0040 item 7.

## Open (c): not implemented

C1. Deny writers other than Claude: Cursor CLI config, opencode `permission.<tool>`, Pi
    `defaultTools` (SPEC 6.8, 6.11, 7.3, 11 `tkm prune apply/restore`, G6). See
    `crates/tkm-hosts/src/writer/deny.rs` and ADR 0032.
C2. 24-hour latest deadline for writing the deny list: the store records no candidate-ready time
    (SPEC 7.3). See `crates/tkm-cli/src/cmd/prune.rs` and `crates/tkm-store/src/migrations.rs`.
C3. `prune.protect` config key (SPEC 7.3). See `crates/tkm-config/src/config.rs`.
C4. 30-day delegated-run cap on pruning: the store keeps a lifetime count only (SPEC 7.3). See
    `crates/tkm-store/src/migrations.rs`.
C7. Session log discovery without a hook transcript path for hosts other than Claude (SPEC 6.12).
    See `crates/tkmd/src/fold.rs` and ADR 0018 item 5.
