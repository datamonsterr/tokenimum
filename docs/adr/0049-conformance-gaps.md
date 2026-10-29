# ADR 0049: Conformance audit gaps and outstanding verification

Status: recorded for follow-up, 2026-10-29. This record documents observed gaps; it does not
change SPEC.md or assert that unfinished work satisfies the spec. Fixes and measurement require
separate work.

1. The fold stores a zero `priced_cost`, despite pricing support in core; end-to-end session
   pricing and weekly price refresh are not wired. See `crates/tkmd/src/fold.rs` and
   `crates/tkm-core/src/ledger/pricing.rs`.
2. Skill usage reports reconstruct at most 30 recent Claude sessions and skip other hosts;
   loaded counts require reader catalog data. The store has no skill evidence table.
   See `crates/tkm-cli/src/cmd/skills.rs`.
3. M0 and real-session performance/savings/success targets cannot be established from synthetic
   fixtures or the deterministic CI benchmarks alone. A representative real-session evaluation
   and measurements are still needed; see ADR 0045 and ADR 0046.
4. The M1 live shadow end-to-end test runs through `tkm init` and test fixtures; it does not
   establish that real sessions produce shadow decisions before a person installs/configures the
   daemon. See `crates/tkm-cli/tests/e2e_shadow.rs`.
5. M4 real sessions for each hook-level host have not been demonstrated: Cursor, Kimi and
   Codewhale examples in the verification notes are synthetic or reported. See
   `docs/verified/e5b-hosts.md`.
6. E10.6 replay benchmark harness is being merged in parallel; evidence is pending and the
   controller will replace its placeholder.
7. The Pi shim applies prompt context and notices, but no `pi.sendMessage` steer path is present;
   the prompt path is available. See `shims/pi.ts`.
8. The automatic fallback after a failed `tkm doctor` probe is not established by the mode
   selection code: doctor reports missing events and fold-only is selected during init/config.
   See `crates/tkm-cli/src/cmd/doctor.rs` and `crates/tkm-hosts/src/fold_only/mod.rs`.
9. Codewhale's installer registers six events, excluding `session_idle`, and rejects project-scope
   installation. See `crates/tkm-hosts/src/codewhale/install.rs`; ADR 0020 explains user scope.
10. `tkm doctor --probe` inspects recorded hook events and probes models; it does not start a
    Cursor probe session or switch individual missing events to idle-fold. See
    `crates/tkm-cli/src/cmd/doctor.rs`.
11. Repetition of Cursor sessionStart context at postToolUse is not established by the
    pending-injection path: it retains context only when the original event could not inject.
    See `crates/tkmd/src/injection.rs` and ADR 0017.
12. The daemon fold always supplies `decisions: None` to fact lifecycle, so production folds do
    not ask the specified batched stable/safe or contradiction decisions, even though core can
    consume decision evidence. See `crates/tkmd/src/fold.rs` and `crates/tkm-core/src/facts/mod.rs`.
13. The calibration gate checks a stored valid calibration record; configuration lists 14 days
    and 200 turns, but the live decision path does not enforce those durations/counts independently
    of the calibration row. See `crates/tkmd/src/live.rs` and `crates/tkm-config/src/config.rs`.
14. Free-model re-probes are explicit through `tkm doctor --probe`; no daily scheduler was found
    that updates the daemon's live model chain. See `crates/tkm-cli/src/cmd/doctor/models.rs` and
    `crates/tkm-decide/src/client.rs`.
15. `facts.enabled`, `ledger.enabled` and `hosts.<host>.enabled`/`level` appear in the config
    registry but are not applied by daemon settings or host mode selection. In particular, fold
    always calls the fact and ledger updates. See `crates/tkmd/src/settings.rs`,
    `crates/tkmd/src/fold.rs` and `crates/tkm-hosts/src/fold_only/mod.rs`.
16. No committed golden replay expected-action JSON was identified for all eight hosts, and no
    executable comparison running TE as oracle on the same Claude and Codex fixture logs was
    identified. There are ported behavioral tests and a single golden loop test; see
    `crates/tkm-core/tests/facts_te.rs` and `crates/tkm-core/tests/loopguard_behavior.rs`.
17. A fresh-machine end-to-end install and measured precision on representative labelled real
    turns are not established by the existing fake-home and synthetic-labelled tests. See
    `crates/tkm-cli/tests/init_lifecycle.rs` and ADR 0045.
18. Gemini CLI hook applicability to agy has not been verified; SPEC 18 explicitly records it
    as open and the implementation does not assume the two products share hooks.
19. The fault tests cover several failure classes, but a single all-host matrix proving each
    listed failure yields both a no-op reply and an error line has not been identified. See
    `crates/tkmd/tests/faults.rs` and `crates/tkm-hook/tests/integration.rs`.
20. TUI Spend prints daily spend, caps and a textual seven-day series, but does not render the
    requested per-model share or sparkline. Session rows do not render the turn identifier, and
    redirect rows do not render triggering calls. See `crates/tkm-ui/src/tui.rs`.
