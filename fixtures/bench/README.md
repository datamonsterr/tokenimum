# Offline calibration fixtures

`cache.json` and `sessions/` contain the original 50 synthetic loop/comparison sessions.
`outcomes.json` adds 24 complete normalized Claude/Codex turns with an existing test-command
fact. Half exercise its working form successfully; half observe that form fail. Expected labels
are checked against the production outcome helper before fitting. Each turn carries synthetic
fact-filter, fact-contradiction and relevance answers for three dated model names, plus a tied
control model that cannot reach the precision target. These responses and model names are test
doubles, not live measurements.

`tkm calibrate --fixtures fixtures/bench` fits all purposes. `--purpose loop`, `--purpose facts`
and `--purpose relevance` select one family; facts fits filter and contradiction separately.
`fixture_precision_targets_gate_shadow_for_every_purpose_and_model` in
`crates/tkm-cli/tests/calibrate_ops.rs` verifies sample count, precision, recall, control rejection
and authorization revocation below the target. All Rust tests run in controller CI.
