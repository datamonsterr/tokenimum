# ADR 0012: Configuration and repository identity

Date: 2026-10-29. Status: accepted.

Configuration uses seven layers in ascending priority: defaults, the selected preset, global,
repository, repository-local, environment, and invocation flags. Environment names match registered
keys after uppercasing and replacing dots with underscores; for example `loop.mode` becomes
`TKM_LOOP_MODE`. `TKM_CONFIG` chooses the global file and `TKM_DISABLE` disables the process tree.
Unknown `TKM_` names are reported.

Repository files are untrusted. They cannot set decision budgets, the API key path, or the
escalation command, and cannot set pruning to `apply`. If a repository file selects a preset,
its restricted preset values are also ignored and reported. A trusted preset can set them.

| Preset | Overrides |
|---|---|
| observe | `inject.enabled=false`, `loop.mode=shadow`, `prune.mode=report`, `decide.shadow=true` |
| balanced | No overrides; calibration gates redirect at runtime. |
| aggressive | `facts.promote_confirmations=2`, `loop.level=aggressive`, `prune.mode=apply`, `decide.agreement=false`, `inject.max_tokens=900` |
| offline | `facts.evidence=code`, `decide.enabled=false`, `prune.mode=propose`, `inject.memory=false` |

A repository key is the normalized remote when available, for example `github.com/owner/repo`.
Without a remote, it is `path:` followed by the real path of the main checkout. Outside a checkout
it is `dir:` followed by the real path of the starting directory. No hash is used. Linked
worktrees resolve their shared git directory before choosing a remote or main checkout.

The socket is `$XDG_RUNTIME_DIR/tokenimum/tkmd.sock` when a runtime directory is available;
otherwise it is `<data dir>/run/tkmd.sock`. Its directory is private and the socket is mode 0600
on Unix.

## Remote normalization follows TokenEconomizer

As in TokenEconomizer's `paths.py`, the entire normalized remote is lowercased. Repositories on
case-sensitive hosts whose paths differ only in case therefore share a key; this is accepted for
parity. SCP-style remotes with a path starting with a digit are treated as host:port, also for
TokenEconomizer parity.

Saving `.tokenimum.local.toml` adds `/.tokenimum.local.toml` to the shared Git `info/exclude`
file, including for linked worktrees.
