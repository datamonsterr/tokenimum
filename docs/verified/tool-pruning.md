# Tool-pruning configuration verification

Checked against official documentation on 2026-10-29, with the Pi configuration-path and Claude
permission references rechecked on 2026-10-30. This change adds settings writers and uses existing
hook observations; it introduces no new hook event or payload shape.

| Host | Verified project target and encoding | Official evidence |
|---|---|---|
| Claude Code | `.claude/settings.local.json`, `permissions.deny` string array, MCP names `mcp__server__tool` | [Settings scopes](https://code.claude.com/docs/en/settings), [permission rules](https://code.claude.com/docs/en/permissions) |
| Cursor | `.cursor/cli.json`, `permissions.deny` string array; MCP rules `Mcp(server:tool)` | [CLI permissions](https://cursor.com/docs/cli/reference/permissions) |
| opencode | `opencode.json`, `permission` object; exact tool key maps to `"deny"`; MCP keys `server_tool` | [Permissions](https://opencode.ai/docs/permissions/), [MCP naming verification](opencode-pi.md) |
| Pi | `.pi/settings.json`, `defaultTools` string array; `-name` removes a tool from the inherited selection | [Settings](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/settings.md), [project configuration](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/configuration.md) |

Pi's official settings reference distinguishes an absent field from `[]`: the empty array disables
built-in tools. Restore removes a field tokenimum created when its final owned exclusion is
removed, using the original backup as evidence. A settings file created by tokenimum returns to
absence when no foreign content remains. Foreign bytes changed after application are retained.

The shared [writer](../../crates/tkm-hosts/src/writer/deny.rs) retains a first-write `.tkm-bak`,
excludes settings and backup through git's resolved `info/exclude` (including worktrees), refuses
tracked settings/backups and escaping symlinks, and uses snapshot-checked atomic writes. Existing
foreign deny entries are never adopted. Existing opencode allow, ask or granular rules are not
replaced. Dry runs propose ownership and paths without writes.

V009 adds nullable `tool_evidence.ready_since`; old rows keep unknown readiness until next
observation. Fold records first eligibility using the configured thresholds and protection set;
use clears readiness. CLI apply uses an injected clock and the latest stored repository, session
or hook activity. Each candidate can write after the configured idle interval (60 minutes by
default), or once its first-ready time is 24 hours old.

V010 adds indexed, deduplicated `delegated_run_history`. The daemon queues delegated sessions
through the retrying outbox and increments the lifetime count once. CLI queries the inclusive
`now - 30 days <= ran_at <= now` window. With the adapter enabled, core `effective_mode` caps an
explicit apply at propose. The existing lifetime count is retained; no timestamps are fabricated
for historical counts. Adapter-off, stale history, future times and other repositories do not cap
application. CLI restore remains available during the cap.

The validated layered string-list key `prune.protect` reaches daemon settings and CLI. Protection
includes TE's work and interactive sets, native host essentials, headroom names in normalized and
host-native forms, and orchestrator names only when adapter detection enables that integration.

## Committed tests

| Item | Red commit | Implementation | Executable evidence |
|---|---|---|---|
| Four-host deny writers and CLI | `326ac5c` | `56bc0e1` | [deny_hosts.rs](../../crates/tkm-hosts/tests/deny_hosts.rs): roundtrip, foreign bytes/permissions, backup, idempotence, dry-run, exclusion, tracked/symlink refusal. [prune_ops.rs](../../crates/tkm-cli/tests/prune_ops.rs): `prune_apply_one_and_restore_all_four_hosts`. |
| Write timing | `238c816` | `45c0382` | [prune_clock.rs](../../crates/tkm-cli/tests/prune_clock.rs): `mocked_clock_applies_at_idle_boundary_or_ready_deadline`. [prune_timing.rs](../../crates/tkm-store/tests/prune_timing.rs): stable readiness and reset. [migrations.rs](../../crates/tkm-store/src/migrations.rs): V008 upgrade preservation. |
| Protection | `c100fa0` | `d32381d` | [config.rs](../../crates/tkm-config/tests/config.rs): schema/layers/type rejection. [e9_settings.rs](../../crates/tkmd/tests/e9_settings.rs): settings propagation. [prune_protection.rs](../../crates/tkm-core/tests/prune_protection.rs): protection sources and host names. [prune_ops.rs](../../crates/tkm-cli/tests/prune_ops.rs): custom protection and disabled orchestrator. |
| Recent delegation | `822aa7b` | `a1de12e` | [prune_delegation.rs (store)](../../crates/tkm-store/tests/prune_delegation.rs): exact 30-day window and idempotence/scoping. [persist.rs](../../crates/tkmd/src/persist.rs): `delegated_outbox_retries_without_double_counting`. [prune_delegation.rs (daemon)](../../crates/tkmd/tests/prune_delegation.rs): daemon observation. [prune_ops.rs](../../crates/tkm-cli/tests/prune_ops.rs): enabled/disabled-adapter cap. |
| Restore and activity regressions | `1a4e3d5` | `8dc2e2f` | [deny_hosts.rs](../../crates/tkm-hosts/tests/deny_hosts.rs): remove created containers, retain later foreign edits, restore fresh files to absence. [prune_clock.rs](../../crates/tkm-cli/tests/prune_clock.rs): recent hook activity affects the idle boundary. opencode MCP key assertions use `server_tool`. |

## Validation boundary

No local Cargo build, test, check, clippy or bench was run, as required by the task. Red commits
specify regressions or missing APIs; observed Rust red/green results and workspace CI gates must
come from the controller's CI. The tests above are executable evidence, not a claim of an observed
passing Rust run.

Permitted local checks: `cargo fmt --all -- --check`, `cargo metadata --no-deps --locked
--format-version 1`, `git diff --check`, and `python3 scripts/check_spec_conformance.py`. A Python
SQLite smoke check executed the embedded V001-V010 SQL, upgraded seeded V008 evidence, and checked
nullable readiness, preserved lifetime counts, duplicate delegation observations and inclusive
30-day/future bounds. Those SQL checks passed; they do not replace the Rust suites.
