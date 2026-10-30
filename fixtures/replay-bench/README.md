# Synthetic replay benchmark corpus

Every message, model ID, session ID, token count, tool result, commit ID and configuration here
is invented. No developer history or credentials were used. `malformed.jsonl` is deliberately
invalid; `running.jsonl` and `tool-pending.jsonl` deliberately do not finish a turn.

`main.jsonl` includes string/text prompts, host injections, interrupts, duplicate snapshots of
one assistant message, and a failed tool call. `agent.jsonl` reuses an assistant message ID in
a separate session to test scoped deduplication. `promptless-agent.jsonl` contains sidechain
records and equivalent UTC/offset timestamps; `git-log.tsv` straddles its start timestamp.
The config trees distinguish exact tokenimum hooks from permission changes and spoofed shell
commands. The tests also compare against actual installer-generated hooks and the skill.

Core contracts: `crates/tkm-cli/tests/replay_bench.rs`. Repository export aggregation:
`crates/tkm-cli/tests/replay_bench_export.rs`. All are offline and run in normal workspace CI.
