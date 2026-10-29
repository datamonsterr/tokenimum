# tokenimum implementation plan: epics and tasks

Source of truth for scope: [`SPEC.md`](../../SPEC.md) (draft 2). This file splits the spec into
epics, each with tasks and a Definition of Done (DoD). Decisions on the spec's open questions are in
[`docs/adr/`](../adr/). Terms are in [`CONTEXT.md`](../../CONTEXT.md).

## How every epic is delivered (applies to all epics)

1. **One Claude Code session per epic.** The session is the controller: it plans, briefs,
   reviews and merges. Implementation is delegated through superorchestrator (load the
   `use-superorchestrator-mcp` skill before the first delegation). Native subagents are the
   fallback when superorchestrator is unavailable.
2. **Branch from current `origin/main`:** `git fetch origin && git checkout -B epic/<id>-<slug> origin/main`.
3. **TDD.** For every task: commit failing tests first (red), then the implementation (green),
   then refactor. Test commits and implementation commits are separate so CI shows red then green.
4. **CI runs the tests, not this machine.** Do not run `cargo build`, `cargo test`, `cargo clippy`,
   `cargo bench` or `cargo nextest` locally. Allowed locally: `cargo fmt`, `cargo metadata`,
   `cargo generate-lockfile` / `cargo update -p <crate>` (lockfile only, no compile), `git`, `gh`.
   Push the branch, open a draft PR early, and read results with `gh pr checks <pr>` /
   `gh run view <id> --log-failed`.
5. **Check current docs before coding** against any crate, GitHub Action, CLI or harness. No API
   from memory. Close "to verify" items with the doc link and date in `docs/verified/`.
6. **Merge rule.** Mark the PR ready and squash-merge only when (a) every CI job on the PR head is
   green, (b) every DoD item of the epic is checked in the PR description with evidence (test name,
   CI job, file), (c) a review pass (`/code-review` or a reviewer subagent) found no open
   correctness issue, and (d) the branch is rebased on the latest `origin/main` and CI is green on
   that head. Large epics may merge in several PRs; each PR obeys the same rule.
7. **Clean up after each task and at the end:** `cargo clean` in every worktree that built
   anything, remove finished superorchestrator worktrees, delete the merged remote branch.
8. **Report** at the end: PR links, merged commit, DoD checklist with evidence, any item deferred
   (with the reason and a follow-up issue).

### Global DoD (every epic, in addition to its own)

- `#![forbid(unsafe_code)]` at every crate root; workspace lints apply; `clippy::unwrap_used` and
  `clippy::expect_used` denied outside tests; `#![deny(missing_docs)]` on library crates.
- CI green: fmt, clippy (`-D warnings`), tests (nextest), forbid-unsafe check, cargo-deny,
  cargo-geiger diff, docs build.
- New public behaviour has tests; fail-open paths have fault-injection tests.
- No secrets, no real user data in fixtures (redacted per SPEC 8.4).
- Ported TokenEconomizer logic keeps attribution (file header + `NOTICE`).

## Waves and dependencies

| Wave | Epics (parallel inside a wave) | Needs merged |
|---|---|---|
| 1 | E0 | — |
| 2 | E1, E2, E3, E5A | E0 |
| 3 | E4, E5B, E6 | E1, E2, E3, E5A |
| 4 | E7, E8, E9 | E4 (E9 also E5B/E6 for installers) |
| 5 | E10 | all |

Shared contracts are frozen in E0 (`tkm-proto` types, `tkm-core::model` normalized session shape,
the `HostAdapter` trait signature in `tkm-hosts`). Later epics may extend them additively; a
breaking change needs an ADR.

---

## E0. Foundation, contracts and CI/CD (SPEC 5, 13, 14, 16)

Tasks:

- E0.1 Cargo workspace with all 11 crates of SPEC 5.2 (`tkm-proto`, `tkm-hook`, `tkm-core`,
  `tkm-hosts`, `tkm-decide`, `tkm-store`, `tkm-integrations`, `tkm-config`, `tkm-ui`, `tkmd`,
  `tkm-cli`), `[workspace.lints]` (`unsafe_code = "forbid"`, clippy pedantic warn, unwrap/expect
  deny), `[workspace.dependencies]` for every library in SPEC 5.3 at current versions, committed
  `Cargo.lock`, `rust-toolchain.toml`, `shims/`, `fixtures/` directories.
- E0.2 `tkm-proto`: request/reply types (host name, event kind, raw payload, env markers;
  reply = inject / notice / nothing), length-prefixed JSON framing with size limit, round-trip and
  malformed-frame tests.
- E0.3 Shared contracts: `tkm-core::model` (`Session`, `Turn`, `Call`, `FileEdit`, `Usage`,
  `StopReason`, `ToolDef`, `Host`, `RepoKey`) per SPEC 6.4; `tkm-hosts::HostAdapter` trait
  (map payload to event, render reply, read session log, install/uninstall, prune target) with a
  no-op test implementation.
- E0.4 CI workflow (GitHub Actions) on push and PR: fmt, clippy, nextest, doc, forbid-unsafe grep
  check (`scripts/check-forbid-unsafe.sh`), cargo-deny (`deny.toml` per SPEC 13.2), cargo-geiger
  diff against `docs/unsafe-allowlist.md`, MSRV/toolchain pin; cargo cache (e.g.
  `Swatinem/rust-cache`); concurrency group cancelling superseded runs.
- E0.5 Scheduled workflow: daily `cargo audit`; bench workflow skeleton (criterion + hyperfine,
  stores baseline on `main`, fails on > 20% regression) wired to a placeholder bench.
- E0.6 Release workflow skeleton: static musl build of `tkm`, `tkm-hook`, `tkmd` (cargo-dist or
  equivalent), reproducible-build check (build twice, compare hashes).
- E0.7 `docs/unsafe-allowlist.md`, `CONTRIBUTING.md` (the CI-only test rule, TDD, merge rule),
  README status update.

DoD:

- [ ] `cargo metadata` lists the 11 crates; every crate root has `#![forbid(unsafe_code)]` and
      `[lints] workspace = true`.
- [ ] CI workflow green on the E0 PR, with all jobs of E0.4 present and each one proven to fail on
      a deliberate violation (commit + revert evidence for: missing forbid attribute, unwrap in
      library code, disallowed license).
- [ ] `tkm-proto` framing tests pass (round trip, oversize, truncated, garbage).
- [ ] Shared contracts compile and are documented; `docs/adr/0010-shared-contracts.md` records them.
- [ ] Audit, bench and release workflows exist and pass a manual `workflow_dispatch` run (or the
      release job is documented as dry-run until E10).

## E1. Store, configuration and repository identity (SPEC 9.5, 10)

Tasks:

- E1.1 `tkm-store`: SQLite (bundled, WAL) schema for all 12 tables of SPEC 10.1, versioned
  migrations, typed queries, `open_read_only` helper for foreign SQLite (`SQLITE_OPEN_READ_ONLY`),
  busy timeout, DB-locked handling.
- E1.2 Store query API used by later epics: upsert repo/session/turn, facts CRUD with state
  transitions, tool evidence counters, deny-list records, envelope ring (last 500), decisions,
  calibration, events (with explanation template id + params), ledger, installs.
- E1.3 `tkm-config`: schema with defaults (SPEC 4.3, 8.5, 10.4), presets `observe`, `balanced`,
  `aggressive`, `offline` (SPEC 10.3), seven layers with provenance (SPEC 10.2), `TKM_<SECTION>_<NAME>`
  env overrides, `TKM_CONFIG`, `TKM_DISABLE`, untrusted-repo key filter (budgets, key path,
  `prune.mode = "apply"`, `loop.escalate_command`).
- E1.4 Config editing: get / set / unset per layer with `toml_edit` (comments kept), validation,
  explain text per key, diff from preset.
- E1.5 Repository key: port TE `paths.py` (parse `.git/config`, `gitdir:` files,
  `GIT_CEILING_DIRECTORIES`, stop at `$HOME`), URL normalization so worktrees share a key.
- E1.6 Paths: data dir, config dir, runtime socket path (mode 600), `errors.log` rotation (5 MB).

DoD:

- [ ] Migration test: empty DB to latest, and re-run is a no-op; schema snapshot test.
- [ ] Every table has insert/read round-trip tests; read-only open refuses writes (test).
- [ ] Layer precedence test covers all 7 layers; provenance reported per key.
- [ ] Repository file cannot set any of the four restricted keys (test per key).
- [ ] Repo key identical for main checkout and two worktrees of the same origin (test with temp
      repos); no-remote fallback to main checkout path.
- [ ] Preset tables match SPEC 10.3 (table-driven test).

## E2. Core mechanisms, pure (SPEC 7, `tkm-core`)

All pure functions `(state, event) -> (state, actions)`; no I/O. Port TE logic with attribution.

Tasks:

- E2.1 Loop guard: `TurnGuard`, `SpiralState`, shell-edit detection (`sed -i`, `perl -pi`,
  redirects into source files, `tee`, inline scripts, `patch`, `git apply`, `apply_patch`),
  free gate, backstop, verdict combiner (thresholds in), redirect segment reset, escalation state,
  presets `conservative`/`default`/`aggressive`.
- E2.2 Envelope: good-turn classifier (stopped on its own, not escalated, no failing check after
  last edit), ring of 500, p90 × margin, min samples 5.
- E2.3 Facts observe: command segmentation, intent classes, form, outcome, `GENERIC` vocabulary,
  rtk normalization (`rtk <cmd>` to `<cmd>`).
- E2.4 Facts extract: five envfacts families (form repair, boundary, empty probe, argument repair,
  env value) with parameter regex validation (command forms, env var names, host names, test
  labels only).
- E2.5 Facts lifecycle: promote after N confirmations, recency decay, contradiction weight,
  hard kill, fire gate, code-only mode (+1 confirmation), scope `neutral` / `host:<name>`,
  lint rejecting completion/verification advice and out-of-repo paths, templates for rendering.
- E2.6 Prune: per repo/host tool evidence, candidate rule (10 prompts over ≥ 2 sessions, or 5
  sessions), quiet window (60 min, max 24 h), modes, protected set (TE lists + per-harness
  equivalents + `mcp__headroom__*` + `prune.protect` + `mcp__superorchestrator__*` when adapter
  on), delegation rule (cap at `propose`), skill usage evidence.
- E2.7 Ledger: token classes, cache-aware pricing from a `pricing.toml` table, decision overhead,
  savings estimate vs baseline (shadow sessions + pre-first-action sessions) with sample count and
  confidence band.
- E2.8 Injection assembly: exact + near-duplicate removal (normalized hash, token Jaccard ≥ 0.8),
  selection given scores (≥ 1.5, fire-gate facts ≥ 1.0) under `max_tokens`, cached default block.
- E2.9 Redirect library and explanation templates for every action kind of SPEC 11.5.

DoD:

- [ ] Table-driven unit tests per module; ported TE test cases (`test_envfacts.py`,
      `test_injection.py`, `test_ledger.py`) as Rust tests with the same expectations.
- [ ] Golden test: synthetic loop fixture (pytest ImportError ×4, two `sed -i`) wakes the gate and
      the backstop at the documented call counts.
- [ ] Property test: no protected tool ever appears in a deny-list candidate set.
- [ ] Fact lint rejects completion advice and out-of-repo paths (tests); injected text only from
      templates (test).
- [ ] Every action kind has an explanation template with a rendering test.
- [ ] Crate has no I/O dependencies (CI check: `tkm-core` deps exclude tokio, reqwest, rusqlite,
      std::fs usage grep).

## E3. Decision client (SPEC 8, `tkm-decide`)

Tasks:

- E3.1 Typed request/response for `POST /api/alpha/decisions` (noul, choice, score), validation
  of answers (keys present, probability in 0–1) — check the live API reference first.
- E3.2 Model chain from config, pinned dated versions, failover on 404/5xx/timeout 1,500 ms/invalid
  answer, skip a model 15 min after 3 failures in 5 min, `:free` re-probe support.
- E3.3 Digest builder (prompt ≤ 500 chars, last 12 calls, counts, tests; 16 KB cap) and redaction
  (env values, secret patterns for AWS, GitHub, OpenRouter, Anthropic, OpenAI, Moonshot, DeepSeek,
  Cursor, JWT, private keys; `.env`; paths outside repo; home to `~`).
- E3.4 Cache (hash of model, state, questions; TTL 10 min relevance, 24 h fact filters), batching
  per purpose, provider prefs (`data_collection: "deny"`, `allow_fallbacks: true`).
- E3.5 Budget: daily ($0.50) and per-session ($0.02) caps, code-only fallback signal, spend ledger
  rows.
- E3.6 Key loading: env `OPENROUTER_API_KEY` or `~/.config/tokenimum/openrouter.env` (mode 600
  enforced), optional key path reuse (never copy).
- E3.7 Calibration: thresholds per model version and purpose, defaults (0.85/0.30/0.30/0.80),
  uncalibrated model = log only, fit (best precision at min recall), agreement mode, shadow-mode
  window (14 days or 200 judged turns).
- E3.8 Pricing refresh from `/api/v1/models` into `pricing.toml`.

DoD:

- [ ] All tests use recorded responses (no network); one live test per chain model behind
      `TKM_LIVE=1`, not run in default CI.
- [ ] Fault tests: 404, 500, timeout, invalid answer each fall through the chain; all fail means
      "no decision", never an error to the caller.
- [ ] Redaction test corpus: every listed secret kind removed; digest never exceeds 16 KB.
- [ ] Budget test: over cap returns code-only signal without a request.
- [ ] Calibration fit test on synthetic labels reaches the configured precision target.

## E4. Hook client and daemon (SPEC 5.1, 6.2 rules, 11.3 stream, 14)

Tasks:

- E4.1 `tkm-hook`: no async runtime; read stdin + env markers; 2 ms connect, per-event read
  timeout (300 ms / 1,500 ms / 5 ms); `TKM_DISABLE`; panic catch at top level; any failure prints
  the host's no-op reply and exits 0; on connect miss spawn `tkmd` detached without waiting.
- E4.2 `tkmd` server: tokio Unix socket (mode 600), framing from `tkm-proto`, router to host
  adapters, per-session/turn state in memory, flush to SQLite every 5 s and at turn end.
- E4.3 Live layer: TurnGuard per active turn, gate, async decision queue (one in flight per turn,
  one per 5 calls), deliver verdict on a later `tool_done` or next injectable event, drop at turn
  end; escalation path.
- E4.4 Inject layer: candidates (facts, memory pointers, skill candidates via integration hook),
  relevance scoring within the prompt timeout, cached default block on timeout, move block to the
  next injectable event where `prompt` cannot inject.
- E4.5 Fold workers: session end marker, detached fold (observe, facts lifecycle, prune evidence,
  ledger), idle-fold watcher (`notify` crate, 60 s scan fallback, 30 min idle, fold at next
  session start in same repo), partial-session rule (> 20% skipped records).
- E4.6 Monitor API: local socket push stream (sessions, decisions, spend, facts, redirects) for
  `tkm top` / CLI; read-only DB fallback contract.
- E4.7 Service units: systemd user unit and launchd agent templates; `errors.log` wiring.
- E4.8 Benchmarks: criterion for `tkmd` `tool_done` (< 200 µs); hyperfine for `tkm-hook` round
  trip (p50 < 3 ms, p99 < 10 ms); wired into the E0 bench gate.

DoD:

- [ ] Fault injection tests: daemon down, socket slow, DB locked, OpenRouter failure, corrupt log,
      foreign SQLite locked; each yields the no-op reply + an `errors.log` line, exit 0.
- [ ] End-to-end test (in CI): spawn `tkmd`, drive Claude Code fixture payloads through `tkm-hook`,
      assert reply bytes and stored rows; shadow decisions logged.
- [ ] `tool_done` path makes no network call and no disk write (test with instrumentation).
- [ ] Bench job reports hook p99 < 10 ms and handler < 200 µs on CI hardware.
- [ ] M1 exit criteria met (SPEC 15) for Claude Code.

## E5A. Host adapters: Claude Code and Codex (SPEC 6.2–6.6, 6.13, 7.3 targets)

Tasks:

- E5A.1 Claude Code adapter: payload mapping (incl. `PostToolUseFailure`), reply shapes (10,000
  char cap), stop hint, session log reader (`projects/<slug>/<session>.jsonl`, async lag),
  settings writer (JSON merge, `args` exec form, timeouts, backup `.tkm-bak`, symlink refusal,
  idempotent, uninstall), prune target `settings.local.json` `permissions.deny` + `.git/info/exclude`.
- E5A.2 Codex adapter: mapping, reply rules (no inject on `Stop`/`SessionEnd`,
  `additionalContextLimit`), reader (`sessions/YYYY/MM/DD/rollout-*.jsonl`, `.jsonl.zst` via a
  pure-Rust zstd decoder if available, `session_meta`), `hooks.json` writer, printed trust step,
  never writes `trusted_hash`; prune report-only until deny key verified.
- E5A.3 Redacted fixtures from local Claude Code and Codex sessions; contract tests (payload in,
  reply bytes out) and installer tests against copies of real configs with foreign entries
  (otherskill, rtk) surviving byte-for-byte; uninstall restores original.
- E5A.4 Close to-verify items for Claude Code and Codex (SPEC 18) in `docs/verified/`.

DoD:

- [ ] Contract tests for every event of table 6.2 for both hosts.
- [ ] Reader tests parse fixtures into `Session` with correct turns/calls/usage; > 20% unknown
      records marks `partial`.
- [ ] Installer tests: foreign entries byte-identical; second run no diff; uninstall restores
      backup; symlink out of home refused.
- [ ] Every Claude Code / Codex to-verify item closed or documented with link and date.

## E5B. Host adapters: agy, Cursor, Kimi Code, Codewhale (SPEC 6.7–6.10, M4)

Tasks:

- E5B.1 agy: `hooks.json` with top-level `tokenimum` name, `injectSteps` replies, reader for
  conversation SQLite (read-only) or `transcriptPath`, no session end (idle-fold), prune report-only.
- E5B.2 Cursor: `hooks.json` v1, `beforeSubmitPrompt` opens turn without inject, inject moved to
  next `postToolUse`/`sessionStart`, `transcript_path` reader, prune target `.cursor/cli.json`,
  double-registration check with Claude Code hooks.
- E5B.3 Kimi Code: `[[hooks]]` via `toml_edit` with comment marker, plain-stdout reply, reader
  for `~/.kimi/sessions/<md5(workdir)>/...` (`KIMI_SHARE_DIR`), prune report-only.
- E5B.4 Codewhale: `[[hooks.hooks]]` writer, env + stdin input, inject only via
  `tool_call_before` `additionalContext` (2,000/8,000 caps), reader for sessions dir, report-only.
- E5B.5 Doctor probe metadata per host (expected events) for `tkm doctor --probe`.
- E5B.6 Close the to-verify items for these four (SPEC 18), with local dumps where installed (agy).

DoD:

- [ ] Contract, reader and installer tests per host as in E5A.
- [ ] agy real local session folds (fixture from this machine, redacted).
- [ ] Each to-verify item closed or documented in `docs/verified/` with link and date; behaviour
      for still-unverified items is the safe fallback (report-only / idle-fold).

## E6. Shim harnesses and fold-only mode (SPEC 6.11, 6.12, 13.3, M5)

Tasks:

- E6.1 Shim templates in `shims/` for opencode (Bun `$`) and Pi (`pi.exec`), ≤ 40 lines, no deps,
  header with version + content hash; generator in `tkm-hosts`.
- E6.2 opencode adapter: event mapping, reply application, reader for `opencode.db` (read-only;
  legacy storage dir; `opencode export` fallback), prune target `opencode.json` `permission`.
- E6.3 Pi adapter: mapping, reply (`customType:"tokenimum"`, steer), reader for session JSONL v3,
  prune target `defaultTools` `-name`.
- E6.4 Fold-only: instructions-file block between markers (`inject.instructions_file`, untracked
  files only by default, `.git/info/exclude`), `tokenimum` skill generator per harness (read-only
  commands, no effect claims), `tkm run` wrapper library (env `TKM_SESSION_ID`, `TKM_HOST`,
  immediate fold on exit).
- E6.5 Close opencode/Pi to-verify items.

DoD:

- [ ] Test: generated shims ≤ 40 lines, hash verifies, uninstall removes only matching header.
- [ ] Reader tests on opencode (local DB copy, redacted) and Pi fixtures.
- [ ] Fold-only with `--no-shims` produces facts from fixtures and a regenerated marker block.

## E7. CLI (SPEC 11.1, 11.2, 11.5, 12)

Tasks:

- E7.1 `tkm` clap skeleton for every command of SPEC 12; daemon client with read-only DB fallback.
- E7.2 `tkm init`: detection (binaries + agent homes), plan display, key prompt / reuse /
  offline, writes via adapters, shims, service unit, start daemon, doctor, trust steps; flags
  `--yes --dry-run --only --home --host --superorchestrator --no-shims --preset`; `installs` rows.
- E7.3 `tkm uninstall` exact reversal from `installs`.
- E7.4 `tkm status [--all] [--json]`, `tkm why <event|last> [--session]`, `tkm facts`,
  `tkm facts forget`, `tkm prune [apply|restore]`, `tkm skills [--json]`, `tkm config ...`,
  `tkm models [probe]`, `tkm calibrate`, `tkm replay`, `tkm run`, `tkm doctor [--probe]`,
  `tkm export [--show-digests]`.
- E7.5 `tokenimum` skill wrapper text per harness.

DoD:

- [ ] Integration test: `tkm init --yes --dry-run` on a fake HOME with all eight agent homes
      prints the plan and writes nothing; `--yes` then `uninstall` leaves HOME byte-identical.
- [ ] Snapshot tests for `status`, `status --json`, `why` (the SPEC 11.5 example renders).
- [ ] Every command of SPEC 12 exists with `--help` and at least one test.
- [ ] M3 exit criteria met (SPEC 15).

## E8. Monitor UI: `tkm top` and web dashboard (SPEC 11.3, 11.4)

Tasks:

- E8.1 ratatui app over the monitor stream: six panes, tab switching, `?` help, Enter opens why,
  read-only DB mode when daemon down; render with `TestBackend`.
- E8.2 Web dashboard (feature `web`, axum): bind 127.0.0.1 only, random token per daemon start
  (URL fragment), embedded static assets (no CDN), same panes + history charts + evidence pages,
  three confirmed actions (forget fact, restore tool, switch preset).
- E8.3 `tkm dashboard` command.

DoD:

- [ ] TestBackend snapshot test per pane; frame render < 50 ms bench.
- [ ] Web tests: request without token 401; non-loopback bind refused; no external URLs in
      assets (test); each mutating action needs confirmation.
- [ ] `tkmd` idle memory budget measured in CI (< 30 MB, < 40 MB with web).

## E9. Optional integrations (SPEC 9, M6–M7)

Tasks:

- E9.1 Detection for superorchestrator and otherskill (PATH + config + version probe within 1 s),
  `auto|on|off`, 15-min disable on failure, 10-min re-detect, state in status/doctor.
- E9.2 otherskill adapter: `otherskill select --json --agent <h> --prompt <text>` without a shell,
  candidates into shared budget, de-dup by skill name, `tkm skills --json` ranking prior,
  key-path reuse with consent.
- E9.3 superorchestrator adapter: account-home install mode from `accounts/*.toml`, delegated-run
  detection (`SUPERORCHESTRATOR_RUN_ID`, fallback cwd match against `superorchestrator runs --json`),
  escalation event delivery (channel decided against its current CLI/MCP; ADR), spend report under
  `tokenimum-decisions`, protected `mcp__superorchestrator__*`, delegation rule.
- E9.4 Integration matrix: four combinations of SPEC 9.2 with fake binaries on `PATH`.

DoD:

- [ ] Matrix test green for all four combinations; removing a tool falls back to standalone.
- [ ] Prompt passed as a literal argument (test with shell metacharacters).
- [ ] Delegated-run escalation reaches a fake orchestrator; spend report observed.
- [ ] No protected tool ever in a deny list with the adapter on (test).

## E10. Calibration bench, performance gates and release (SPEC 8.5, 14, 15 M0/M8, 13.2)

Tasks:

- E10.1 `tkm replay` bench over recorded Claude Code and Codex sessions (redacted fixtures; target
  50); report agreement, latency, cost per model (offline cached responses in CI).
- E10.2 Calibration loop: labels from outcomes, `tkm calibrate`, leave shadow per purpose when
  precision ≥ 0.9 for `stuck`.
- E10.3 Performance gates final: all SPEC 14 budgets measured in CI with 20% regression gate.
- E10.4 Release: musl static binaries via cargo-dist (or equivalent), reproducible-build check,
  `cargo-geiger` report attached, install docs.
- E10.5 Spec conformance audit: walk SPEC.md section by section, map each requirement to a test or
  document a deferral; write `docs/spec-conformance.md`.
- E10.6 Replay benchmark over the taphoa-mini Claude Code history, per
  `docs/plan/replay-benchmark.md`:
  - clone A at latest `main` gives the recorded baseline;
  - clone B starts at the oldest intact session's start state, has tokenimum installed, and
    replays every session's prompts in anchored mode with a matched AI configuration;
  - a no-tokenimum control arm measures replay variance.

DoD:

- [ ] Bench report committed (`docs/reports/m0-decision-bench.md`).
- [ ] Calibration test meets the precision target on labelled fixture turns.
- [ ] Release workflow builds static binaries on a tag (dry-run tag acceptable) and the
      reproducibility check passes.
- [ ] `docs/spec-conformance.md` maps every SPEC requirement to evidence; no unmapped item.
- [ ] Replay benchmark:
      - extraction, anchoring, config-diff and accounting code is unit-tested on synthetic
        transcripts in CI;
      - the `workflow_dispatch` job ran the `sample` subset (at least 20 sessions);
      - `docs/reports/replay-benchmark.md` reports baseline against tokenimum tokens per token
        class, with the outcome check and the control-arm variance; it holds no prompt text.
