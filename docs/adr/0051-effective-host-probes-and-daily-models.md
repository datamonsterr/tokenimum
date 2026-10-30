# ADR 0051: Effective host levels, event probes, and daily free-model probes

Status: accepted, 2026-10-29.

Keep configured host levels separate from observed health. An additive store migration records a
per-host fold-only override. Doctor sets it when the installed hook/shim check fails, and removes
it after a successful check; disabled hosts and explicit fold-only configuration remain folded.
The daemon and installation planner resolve the configured level through this override.

`tkm doctor --probe <host>` installs recording hooks/shims in a disposable HOME and runs the
host's documented bounded headless command. Record each native event independently, including
missing events as idle-fold. Unsupported headless hook paths use previously recorded live events
and say so explicitly. Probe sessions never touch the person's host configuration. The daemon
loads this policy outside the tool hot path and keeps idle folding available for missing events.

A daemon maintenance worker checks a durable last-probed timestamp, at startup and periodically.
Once per 24 hours it invokes the decision client's shared free-model probe request, preserving
privacy, deadlines and budget accounting. Successful free slots move to the front of the same
client used for live decisions; failures leave paid fallbacks available. Persist probe results
so restart restores promotion without extra calls. Storage/network failures are logged and do
not interrupt hooks or folding. Tests use fake executables, clocks and HTTP transports.

Deliver each item as a red test commit followed by implementation; CI runs Rust checks. Local
validation is limited to formatting, static inspection and the Python conformance checker.
