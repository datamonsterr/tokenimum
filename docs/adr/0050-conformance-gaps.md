# ADR 0050: Conformance audit gaps and outstanding verification

Status: refreshed 2026-10-30 for pruning changes based on `da6a44d` on `epic/e10` (baseline CI green). This record documents
observed gaps; it does not change SPEC.md or assert that unfinished work satisfies the spec.
The row-level status is in [spec-conformance.md](../spec-conformance.md): 508 met, 37 deferred,
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

The pruning gaps C1-C4 are implemented (SPEC 7.3; ten rows moved to `met`):

- C1: ownership-aware deny writers for Claude, Cursor, opencode and Pi, with backup, exclusion,
  dry-run, tracked/symlink safety, idempotent apply and selective/full restore. Cursor MCP rules use
  `Mcp(server:tool)`; opencode uses `server_tool`; Pi restores inherited `defaultTools` rather than
  leaving an empty array. Tests: `four_host_deny_roundtrip_preserves_foreign_bytes_and_backup`,
  `four_host_deny_refuses_tracked_and_escaping_targets`,
  `restore_removes_created_containers_and_preserves_later_foreign_changes`,
  `restoring_a_new_settings_file_returns_to_absence`, `prune_apply_one_and_restore_all_four_hosts`.
- C2: V009 adds nullable `ready_since`. Fold records first eligibility with configured thresholds;
  tool use clears it. CLI uses latest stored activity and the core 60-idle-minute/24-hour rule.
  Tests: `mocked_clock_applies_at_idle_boundary_or_ready_deadline`,
  `readiness_starts_once_at_threshold_and_resets_on_use_or_threshold_change`,
  `v008_upgrade_preserves_evidence_with_unknown_ready_time`.
- C3: layered, validated `prune.protect` string list reaches runtime and CLI; TE tools, native host
  essentials and headroom are protected, and superorchestrator protection follows adapter detection.
  Tests: `prune_protect_is_a_layered_string_list_and_rejects_wrong_types`,
  `prune_protect_reaches_effective_settings`, `all_protection_sources_and_host_names_are_respected`,
  `prune_honors_user_protection_and_disabled_orchestrator`.
- C4: V010 adds deduplicated delegation timestamps. The daemon records them through its retrying
  outbox; CLI queries the inclusive past 30 days and caps enabled-adapter pruning at propose.
  Untimestamped lifetime counts are preserved without inventing historical dates. Tests:
  `delegated_history_is_idempotent_scoped_and_queries_exact_thirty_day_window`,
  `delegated_session_is_recorded_once_with_daemon_clock`,
  `delegated_outbox_retries_without_double_counting`,
  `prune_recent_delegation_caps_apply_only_when_adapter_enabled`.

Each item has a separate red test commit followed by its implementation. The additional restore
and activity regressions also have a red/implementation pair. Official formats and test-file links
are recorded in [tool-pruning.md](../verified/tool-pruning.md). Rust suites and global CI gates
remain for controller CI; no local Rust build/test/check/clippy/bench was run.

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

C5. Fact and relevance label contracts and `tkm calibrate --purpose facts|relevance` (SPEC 8.5,
    11, 15 M8). See `crates/tkm-cli/src/cmd/calibrate.rs` and `crates/tkm-decide/src/calibrate.rs`.
C6. Agreement mode: `decide_agreement` is never called and `decide.agreement` is only read (SPEC
    8.5, 10.4, 17 Q7). See `crates/tkm-decide/src/client.rs`, `crates/tkmd/src/decider.rs` and
    `crates/tkmd/src/settings.rs`.
C7. Session log discovery without a hook transcript path for hosts other than Claude (SPEC 6.12).
    See `crates/tkmd/src/fold.rs` and ADR 0018 item 5.
