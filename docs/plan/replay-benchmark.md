# Replay benchmark: taphoa-mini history with and without tokenimum

Status: harness implemented for E10 (task E10.6); live sample pending secrets. This adds an end-to-end token benchmark to the E10
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

## Running it

The harness is implemented in `crates/tkm-cli/src/replay_bench/` (ADR
[0048](../adr/0048-history-replay-benchmark.md)). The actual pilot/sample/full run is deferred
until you add the private inputs below. Nothing in ordinary PR CI calls live Claude models.

Prepare the bundle **on your own machine**, outside the tokenimum checkout. These commands are
instructions for the owner; implementation agents and CI must not inspect your real home.
The builder uses only the explicitly selected transcript files and a reviewed AI configuration
template. It never collects `.credentials.json`, account files, or your ambient Claude login.
Python 3.12+, Git and GnuPG are needed.

1. Make a private staging directory, for example `mkdir -m 700 /tmp/taphoa-replay-input`. Inventory the
   historical files, including worktree directories and nested agent JSONL files:

   ```bash
   python3 scripts/replay_bundle.py inventory \
     --source-root "$HOME/.claude/projects" \
     > /tmp/taphoa-replay-input/inventory.json
   ```

2. Write `metadata.json` in that staging directory. Describe **every** inventoried transcript
   exactly once. Give every `so/*` or other agent transcript its spawning session's `parent`;
   nested parents are supported. Do not guess agent ownership from timestamps. `path` is relative
   to `~/.claude/projects`. Top-level sessions also need their original `end_commit` and a
   branch-specific git log file. Capture each relevant branch log in staging:

   ```bash
   git -C /path/to/taphoa-mini log --format='%H%x09%ct' ORIGINAL_BRANCH \
     > /tmp/taphoa-replay-input/main.log
   ```

   The anchor is the last commit at or before the transcript's first timestamp, using committer
   dates. A log cannot recover a historical checkout/branch switch: provide a verified full
   `start_commit` from that original worktree's HEAD/reflog in those cases. Before the first
   commit, leave the override absent and supply a log whose first commit is later than the
   session; the harness uses an empty initial commit. End commits are explicit because a nearby
   timestamp is not reliable evidence of the original outcome. All referenced commits must be
   fetchable from the reference repository before dispatch.

   Schema example (replace all paths/IDs and fill **actual historical USD rates per million**;
   `null` is deliberately not an invented price):

   ```json
   {
     "claude_version": "2.1.310",
     "effort": "high",
     "permission_mode": "default",
     "tests": [["npm", "ci"], ["npm", "test"]],
     "prices": {
       "claude-opus-5-5": {
         "input": null, "cache_creation": null, "cache_read": null, "output": null
       }
     },
     "sessions": [
       {
         "id": "main-session-id",
         "path": "-home-dat-dev-taphoa-mini/main-session-id.jsonl",
         "parent": null,
         "git_log": "main.log",
         "end_commit": "FULL_ORIGINAL_END_COMMIT_HASH"
       },
       {
         "id": "so/agent-session-id",
         "path": "-home-dat-dev-taphoa-mini-agent/agent-session-id.jsonl",
         "parent": "main-session-id"
       }
     ]
   }
   ```

   Add prices for **every** recorded model, including sonnet/fable or other agent models.
   Unknown/invalid rates refuse accounting. Choose the effort and permission mode matching the
   history and the actual project's CI-equivalent setup/test commands; the example commands
   above are placeholders, not a claim about taphoa-mini's test runner. The common configuration
   must match the selected sessions; split incompatible configurations into separate bundles.

3. Create a reviewed, portable `home-template/` in staging. Copy the required `~/.claude`
   configuration into it deliberately: `.claude/CLAUDE.md`, `.claude/settings*.json`, skills,
   agents, commands, plugins and their pinned data. Include MCP configuration as needed. If using
   `.claude.json`, it may contain **only** a reviewed `mcpServers` object, never account/login
   state. Remove secrets and use environment references for required server credentials. Resolve
   symlinks and original-machine absolute paths into portable equivalents. Do not include
   projects, logs, caches containing conversations, `.credentials.json`, credential files, or an
   existing tokenimum installation. The builder refuses files outside its AI-config allowlist.
   Preserve foreign hook entries. Repository `CLAUDE.md`, `.claude/settings*.json`, skills and
   `.mcp.json` come from each session's start commit. Ensure any external plugin/MCP tools are
   available on the runner and supply their required credentials separately.

4. Encrypt the private bundle. Set a strong `REPLAY_BUNDLE_KEY` in your local environment or let
   the builder ask for it; never put the key in argv or a committed file:

   ```bash
   python3 scripts/replay_bundle.py build \
     --source-root "$HOME/.claude/projects" \
     --metadata /tmp/taphoa-replay-input/metadata.json \
     --config-template /tmp/taphoa-replay-input/home-template \
     --output /tmp/taphoa-replay-bundle.tar.gz.gpg
   ```

   Upload the encrypted file to private storage with an HTTPS download URL. Keep plaintext
   staging and the encrypted bundle outside git. Delete plaintext staging when finished. The
   script prints no transcript content; the only normal output is a completion message.

5. Add repository Actions secrets:

   | Secret | Value |
   |---|---|
   | `REPLAY_BUNDLE_URL` | Private HTTPS download URL for the encrypted bundle (a time-limited signed URL is supported). |
   | `REPLAY_BUNDLE_KEY` | The GnuPG symmetric passphrase used above. |
   | `ANTHROPIC_API_KEY` **or** `CLAUDE_CODE_OAUTH_TOKEN` | Claude Code headless authentication; configure one, not both. The official CI documentation supports both names. |
   | `OPENROUTER_API_KEY` | tokenimum's decision-model credentials; decision usage and USD cost count against the treatment arm. |
   | `REPLAY_REPO_TOKEN` | Read-only repository access token for the private `datamonsterr/taphoa-mini` clone. The token is not persisted into Git config. |

   Current official headless/CLI/authentication and Action references, with verification date,
   are recorded in [docs/verified/e10-replay-benchmark.md](../verified/e10-replay-benchmark.md).

6. Dispatch **Private history replay benchmark** with `subset=pilot`, then `sample` when ready.
   Pilot replays five treatment sessions **and ten controls**; sample replays 20 treatment
   sessions and ten controls. Selection is deterministic, oldest intact top-level sessions first.
   The job refuses fewer than ten eligible control sessions. Full additionally requires
   `confirm_full=true` in the dispatch inputs; the hidden CLI independently enforces the same
   guard. The job prints a historical-price replay/control estimate calculated from private
   `baseline.json` before its first live call; decision-model overhead is additional.

The workflow pins the bundle's Claude release, leaves clone A at latest main, and creates B and a
separate no-tokenimum control checkout. Every prompt uses `claude -p --output-format stream-json
--verbose --model <recorded-model>`; later prompts resume the newly assigned session UUID.
Configuration snapshots are checked before prompts are sent. Normal persisted JSONL transcripts,
including agents spawned in the isolated HOME, supply usage; cumulative stream result totals
are never added again. Tokenimum is installed using `tkm init --host claude-code --home
<throwaway-home>/.claude --yes`, and its daemon is managed directly inside the run.

The private working directory holds baseline, plan, measurements and skipped-session IDs/reasons.
A skipped descendant of a malformed parent is also excluded. A missing parent, model price,
configuration mismatch, incomplete measured transcript, or failed control outcome refuses the
comparison. Controls establish the maximum absolute relative cost deviation; a saving must
strictly exceed it. Replay outcomes require all configured project tests and changed-file Jaccard
overlap >=0.5 against the original end commit (both sets empty gives 1). An outcome failure is a
regression even if tokens fall. Decision cost is added once; its total token overhead is reported
separately from Claude's four classes.

Download the `replay-benchmark-<subset>` artifact. Its sole file is
`docs/reports/replay-benchmark.md`, containing only totals, model/size buckets, outcome counts and
control variance. Private inputs and model output are never artifacts; runner cleanup removes
private state even after failures. Review the aggregate report before committing it after the
future sample run. To do one explicit free-running pass, invoke the driver in the same prepared
CI environment with `--free-running`; its report is labelled and controls remain anchored.
For offline inspection, `tkm bench history --help` exposes `baseline`, `plan`, `config-check`, and
`report`; treat the plan as private because it contains extracted prompts.
