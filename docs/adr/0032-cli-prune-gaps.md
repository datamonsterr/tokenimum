# ADR 0032: CLI pruning evidence gaps

Date: 2026-10-29. Status: accepted.

## Context

The CLI's pruning command consumes the existing evidence and host writer APIs. The configuration
registry and storage schema do not yet contain all of the data needed by SPEC 7.3.

## Decision

The CLI protects the built-in set and always protects superorchestrator tools. The configuration
registry does not expose `prune.protect`; adding that key remains follow-up work. The store retains
only a lifetime delegated-run count, not the time of the most recent run, so the 30-day delegation
rule remains a follow-up. The store also does not record when a candidate became ready, so the
24-hour cap cannot be calculated; readiness output identifies its unused-count basis. The CLI
checks the available repository idle timestamp before applying changes.

Only Claude currently has an ownership-aware deny writer. Other hosts remain proposal-only until
host-specific writers are available.
