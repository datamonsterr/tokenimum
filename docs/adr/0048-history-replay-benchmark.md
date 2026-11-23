# ADR 0048: Private anchored history replay benchmark

Date: 2026-10-29. Status: accepted.

## Context

E10.6 measures the cost of repeating the real taphoa-mini work with tokenimum. The fixed design
is [the replay plan](../plan/replay-benchmark.md). E10.1's offline decision benchmark answers a
different question. Historical transcripts contain private material and live replay incurs cost.

## Decision

- Keep the implementation in the existing `tkm-cli` crate, in `replay_bench/`, with a hidden
  `tkm bench history` command. Add no workspace member. The Python standard-library driver
  coordinates isolated processes; transcript interpretation, accounting, anchoring, config
  validation, comparisons and report generation stay in Rust.
- Reuse `tkm-hosts::claude::ClaudeAdapter::read_session` for normalized timing and tool metrics.
  Add strict JSONL parsing and a terminal-turn check before accepting a historical transcript.
  Reuse its checked timestamp parser for promptless sidechain records, and count raw tool IDs
  when those records are absent from the normalized turn model.
  The last conversational record must be a completed assistant turn or a final result; trailing
  system bookkeeping does not make a complete session appear active. Malformed and unfinished
  history is listed in the private baseline rather than counted.
- Deduplicate assistant message IDs within each transcript. Repeated streaming snapshots use
  the maximum observed count in each of the four independent billing classes. Different agent
  transcripts may reuse message IDs. Require explicit historical parent links, reject orphans
  and cycles, and attribute descendants to the top-level spawning session. For a live replay,
  a fresh HOME contains exactly one initiated conversation, so all other generated session
  transcripts in that HOME belong to that conversation. Overlapping agent wall times are not
  summed.
- Clone A stays at latest main and supplies the original history's reference code. Reset B to
  the last commit at or before the first transcript timestamp, from that session's historical
  branch-specific committer-date log. Before the first commit, use a synthetic empty initial
  commit. Git logs cannot reconstruct historical branch switches: permit an explicit verified
  `start_commit` from the original worktree/reflog when necessary. Do not infer original end
  commits from timestamp proximity; require them in the bundle. Check all objects exist before
  spending model tokens. Free-running mode is explicit and labelled in the report.
- Pin one Claude release and use recorded model IDs, matched effort and permissions, reviewed
  global AI configuration and the repository configuration at the start commit. Compare full
  AI configuration snapshots before the first prompt. Only exact installed Claude hook groups,
  first-write backups, the exact generated tokenimum skill, and newly generated tokenimum
  config/service files may differ. A command containing the string `tkm-hook` is insufficient.
  Keep `claude-code` as an init-only CLI spelling; the hook protocol still uses `claude`.
- Use `claude -p --output-format stream-json --verbose --model <recorded model>` with stdin
  prompts. Assign a new UUID per historical session and resume that UUID for later prompts.
  Read usage from the persisted transcripts, not cumulative headless result totals. Do not use
  bare mode, which would disable the configuration and hooks being measured.
- Run ten distinct anchored no-tokenimum controls for every subset, including pilot (five
  treatment sessions plus ten controls). The noise floor is the **maximum absolute relative
  priced-cost deviation** between each control and its recorded baseline. This is an empirical
  conservative bound, not a statistical confidence interval. A saving must strictly exceed it.
  Failed controls invalidate the comparison. Require passing replay project tests and changed
  file Jaccard overlap of at least 0.5 against the original end commit. Failed outcomes are
  regressions even if fewer tokens were used; empty/empty changed-file sets have overlap 1.
- Historical per-model prices are explicit bundle inputs; do not invent rates for unavailable
  models. Charge exported decision-model cost against treatment and report its tokens separately
  because the current ledger exposes only total decision overhead tokens. The ordinary `tkm
  export` gains repository-scoped activity counts: actual `loop.redirect` events and sessions
  with completed folds. All private measurements remain outside the checkout.
- Run only through `workflow_dispatch` on a GitHub-hosted Linux runner, with the full-run guard
  in both workflow and CLI. Build the encrypted bundle only when the user invokes the builder.
  Download and decrypt into private runner storage. The sole artifact is aggregate Markdown:
  token classes, priced totals, model rows, size buckets, outcome counts and noise floor. Never
  upload baseline JSON, manifests, transcripts, prompts, diffs, exports, or model output.

## Consequences

The harness is reviewable and testable without private history or credentials. The live sample
run and its resulting report remain pending until the user supplies secrets and dispatches it.
Unit contracts use invented transcripts in `fixtures/replay-bench/`; no live model calls occur
in ordinary CI. Rust compile, test and lint validation runs in CI, per repository delivery rules.
The throwaway-home runner starts and stops its daemon directly; installer service-manager calls
are contained with temporary install-only executables to avoid modifying the runner's user
services. Only CI builds binaries. Official API evidence is in
[the verification record](../verified/e10-replay-benchmark.md).
