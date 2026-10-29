# Replay benchmark: taphoa-mini history with and without tokenimum

Status: planned for E10 (task E10.6). This adds an end-to-end token benchmark to the E10
calibration bench (E10.1), which measures decisions only. It answers one question: does an agent
spend fewer tokens on the same real work when tokenimum is installed?

## Corpus

`taphoa-mini` (`git@github.com:datamonsterr/taphoa-mini.git`) has a complete Claude Code history
under `~/.claude/projects/-home-dat-dev-taphoa-mini*`. As of 2026-10-29 that history holds:

- 61 transcripts, from 2026-10-22T23:22Z to 2026-10-29T07:45Z.
- Main sessions, worktree sessions (`claude/*`) and superorchestrator agent sessions (`so/*`,
  with no typed prompts of their own).
- Models `claude-opus-5-5`, `claude-sonnet-5-5` and `claude-fable-5-1`, on Claude Code 2.1.304 to
  2.1.310.
- Recorded usage of about 72 k input, 3.96 G cache-read, 56.9 M cache-creation and 14.7 M output
  tokens.

The oldest intact session started before the first commit (`e28663f`, 2026-10-22T23:28Z), so the
oldest starting state is an empty repository.

## Two clones

| Clone | Checkout | Role |
|---|---|---|
| A, `taphoa-baseline` | latest `origin/main` at bench time | Reference end state. Its recorded sessions are the baseline. Nothing is replayed in A. |
| B, `taphoa-replay` | the state at the start of the oldest intact session (empty repo, initial commit only) | tokenimum installed with `tkm init --host claude-code`; every recorded session is replayed in order. |

Both clones run under the same throwaway `HOME` template, so their AI configuration matches:

- Claude Code version pinned to one release, and the same model per session as recorded.
- The same `CLAUDE.md`, `.claude/settings*.json`, skills, plugins and MCP servers as the original
  sessions. These are taken from the repository at each session's start commit and from a
  snapshot of `~/.claude`.
- The same effort and permission mode.

The configuration diff between A and B must contain only the tokenimum hook entries and files that
`tkm init` writes. The harness checks this and refuses to run if anything else differs.

## Baseline

1. Load every intact transcript of A in time order. A transcript counts as intact when it parses
   fully and ends on an assistant turn or a final result. Sessions still running are skipped and
   listed in the report.
2. For each assistant message, sum `usage.input_tokens`, `cache_creation_input_tokens`,
   `cache_read_input_tokens` and `output_tokens`. Group the sums by session, by model and in
   total. Agent sessions (`so/*`) are counted under the session that spawned them.
3. Also record turns, tool calls, tool errors and wall time per session.
4. Store the result as `baseline.json` next to the run, never in git, because transcripts carry
   private data.

## Replay in clone B

For each intact top-level session, oldest first:

1. Reset B's worktree to that session's start commit, the commit `HEAD` pointed to at its first
   timestamp. This is anchored mode, the default. Anchoring keeps each replay on the same code the
   original session saw, so drift from earlier replays does not compound. A free-running mode,
   where each replay continues on the previous replay's output, is available for one end-to-end
   pass.
2. Extract the session's prompts in order: user messages with string content or text blocks.
   Harness-injected messages are dropped, because the harness regenerates them during the
   replay. These include `<task-notification>`, `<system-reminder>`, hook feedback, tool results
   and interrupts.
3. Send the prompts to Claude Code headless with the recorded model, one turn per prompt, in one
   new session per recorded session. Each prompt waits for the previous turn to finish.
4. The replay writes a normal Claude Code transcript, so every replayed session and message is
   real session data. Agent sessions spawned during a replay are recorded and counted under that
   replayed session, the same way as in the baseline.
5. Collect the same per-session metrics as the baseline, plus tokenimum's own data from `tkm
   export`: decisions, redirects, folds and decision-model tokens and cost.

## Comparison and report

- Per session and in total: recorded baseline against tokenimum replay, for each token class, plus
  a cost-weighted total at the recorded model prices. Decision-model tokens count against
  tokenimum.
- Outcome check: the replay's final diff is compared with the original session's end commit, using
  the project's tests (CI-equivalent commands inside the clone) and changed-file overlap. A
  session with fewer tokens but a failed outcome is reported as a regression, not a saving.
- Noise control: the recorded history comes from different days and agent luck. A third arm, a
  replay of clone B without tokenimum, is run on a sample of at least 10 sessions to measure
  replay variance. A saving counts only when it exceeds that variance.
- The report goes to `docs/reports/replay-benchmark.md` and holds aggregates only: totals, per-model
  and per-session-size buckets, and no prompt text.

## Where it runs

The replay calls live models, so it never runs in PR CI. It is a `workflow_dispatch` job on
`namespace-profile-linux-xl`, which takes:

- the transcript bundle as an encrypted artifact or secret store,
- the Claude credentials as a repository secret,
- a session subset: `pilot` (5 sessions), `sample` (20 sessions) or `full`.

Unit tests for the extraction, anchoring, config-diff and accounting code run in normal CI on
synthetic transcripts.

## Cost guard

A full anchored replay costs roughly as much as the original history. That is about 4 G cache-read
and 15 M output tokens. The job prints an estimate from `baseline.json` and needs an explicit
`confirm_full: true` input for `full`.
