# ADR 0050: Conformance audit gaps and outstanding verification

Status: refreshed 2026-10-29 against `da6a44d` on `epic/e10` (CI green). This record documents
observed gaps; it does not change SPEC.md or assert that unfinished work satisfies the spec.
The row-level status is in [spec-conformance.md](../spec-conformance.md): 499 met, 46 deferred,
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
- Skill paths for hosts without a verified location (Codex, agy, Cursor, Codewhale): ADR 0026.
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
C5. Fact and relevance label contracts and `tkm calibrate --purpose facts|relevance` (SPEC 8.5,
    11, 15 M8). See `crates/tkm-cli/src/cmd/calibrate.rs` and `crates/tkm-decide/src/calibrate.rs`.
C6. Agreement mode: `decide_agreement` is never called and `decide.agreement` is only read (SPEC
    8.5, 10.4, 17 Q7). See `crates/tkm-decide/src/client.rs`, `crates/tkmd/src/decider.rs` and
    `crates/tkmd/src/settings.rs`.
C7. Session log discovery without a hook transcript path for hosts other than Claude (SPEC 6.12).
    See `crates/tkmd/src/fold.rs` and ADR 0018 item 5.
