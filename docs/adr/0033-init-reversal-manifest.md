# ADR 0033: Host-independent init reversal manifest

Date: 2026-10-29. Status: accepted.

## Context

The installs table identifies harness homes. Init also edits global configuration
and can create a key file. These effects are shared by every installed home and
must survive repeated init calls and host-filtered uninstall operations.

## Decision

Keep the existing installs schema and host manifests. Before applying global
changes, write a mode-0600 `init-reversal.json` sidecar in the application data
directory. Store the global config path, its original bytes and Unix mode (or
absence), directories created for global effects, and the path of a key file
only after init successfully creates it with `create_new`. Never store the key
value or copy an environment key. Repeated init preserves the first config
snapshot and extends ownership only for newly created effects.

Restore global effects after the final recorded home and owned service have
been removed successfully. Restore the exact original config bytes and mode, or
delete a config init created. Delete only a key whose creation was recorded;
reused keys remain in place. Remove recorded directories only when empty. Retain
the sidecar if restoration fails, including when no host rows remain, so another
uninstall can retry. Host-filtered uninstall keeps shared effects while other
homes remain installed. Dry-run reads this manifest without changing it.

Service metadata records successful detached fallback startup. A managed stop
failure returns failure and retains an installs recovery row. A detached daemon
has no pid file in tkmd; when its monitor reports running, uninstall reports
`pkill -x tkmd` and retains recovery state until the person stops it. It never
kills an arbitrary process. An absent detached daemon requires no manager call.

## Consequences

No public store contract or migration changes are needed. The sidecar stays in
the data directory, outside the config tree being restored. Global reversal
intentionally restores the pre-init file as a whole, including its formatting;
edits to that file after init are replaced during final uninstall. Restoration
refuses symlink targets and keeps recovery information on failure. Concurrent
init/uninstall invocations are not serialized by this private manifest format.
