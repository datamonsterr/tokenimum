# tokenimum: project context

Glossary: see SPEC.md section 3. Terms below are settled for implementation.

- **Epic / task / DoD:** units of the implementation plan in `docs/plan/EPICS.md`.
- **Wave:** a set of epics that can run in parallel once earlier waves are merged.
- **CI-only tests:** tests, clippy and benches run in GitHub Actions only, never on the
  developer machine (see `docs/plan/EPICS.md`, "How every epic is delivered").
- **Shared contracts:** `tkm-proto` types, `tkm-core::model` normalized session shape, and the
  `tkm-hosts::HostAdapter` trait, frozen in epic E0 in
  [ADR 0010](docs/adr/0010-shared-contracts.md); later extensions are additive and breaking
  changes need a new ADR.

Decisions: `docs/adr/`.
