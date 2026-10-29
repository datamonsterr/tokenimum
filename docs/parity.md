# Same-log TokenEconomizer parity

The oracle is the TE rules at commit `d0675a3`, identified in [NOTICE](../NOTICE),
ported in [facts_te.rs](../crates/tkm-core/tests/facts_te.rs) and
[loopguard_behavior.rs](../crates/tkm-core/tests/loopguard_behavior.rs).
[te_parity_oracle.py](../scripts/te_parity_oracle.py) independently decodes the committed
native Claude and Codex logs and evaluates the relevant ported rules. It neither imports
Rust nor consumes tokenimum output. Regenerate with `python3 scripts/te_parity_oracle.py`;
verify with `python3 scripts/te_parity_oracle.py --check`. This is a scoped reference
implementation of the ported rules, not a claim that an upstream TE executable was run.

The committed [Claude oracle](../fixtures/golden/parity_claude_oracle.json) and
[Codex oracle](../fixtures/golden/parity_codex_oracle.json) cover both recorded, redacted
sessions and synthetic native logs under each host's `parity/` directory. The recorded
sessions have two turns each and yield no environmental fact or loop request. The WIP's
Codex one-turn expectation was incorrect. The positive logs include a missing `pytest`
command repaired with `python -m pytest`, plus the ported 21-call loop trajectory encoded
as native events. The free gate opens at call 18; with an envelope of 8, the strict
backstop fires at call 21. All turns and call counts are compared unconditionally by
[parity_te.rs](../crates/tkmd/tests/parity_te.rs); there is no agreement flag to skip checks.

| Difference | TE reference | tokenimum | Evidence |
|---|---|---|---|
| Promotion without a model filter | Accepted stable/safe filter promotes after three distinct confirming sessions | Daemon replay fold supplies no decision evidence, so SPEC's code-only path requires four | The oracle commits both trajectories; the Rust test asserts both, including the session-three disagreement |
| Native tools in the synthetic loop | Claude has Read calls; Codex encodes reads as successful `cat` commands and edits as shell commands | Host adapters preserve these native names; the loop signals agree | Both oracles assert judge call 18 and backstop call 21 |

There are no other fact-formation or loop-gate differences for these logs. Candidate IDs
and active IDs are checked, including a positive candidate and both sides of promotion.
The four-session lifecycle exercise uses distinct synthetic session identities with the
same call sequence; it does not pretend that replaying one identity confirms a fact.
The oracle's command vocabulary is intentionally limited to these fixtures; general
classification, contradiction, and segment-reset behavior remains in the ported TE tests.

Golden hook replay is separate: [golden_replay.rs](../crates/tkmd/tests/golden_replay.rs)
discovers every `fixtures/<host>/hooks/*.json`, maps it and runs the daemon plus pure fold,
then maps each host's synthetic `replay/loop.json` through the real guard with a mocked
calibrated stuck answer. Expected reply actions and exact stdout are in
`fixtures/golden/<host>.json`. In CI or another permitted Rust execution environment,
`TKM_BLESS=1 cargo test -p tkmd --test golden_replay` regenerates these eight snapshots,
even when a snapshot is absent. Oracle regeneration always uses the independent Python
script; `TKM_BLESS` cannot rewrite the TE oracle.
