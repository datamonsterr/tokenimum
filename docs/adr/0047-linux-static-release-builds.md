# ADR 0047: Linux static release builds

- **Status:** Accepted
- **Date:** 2026-10-29

## Context

SPEC 13.2 requires reproducible static Linux musl releases. E10.4 needs x86_64 and aarch64
artifacts, a repeat-build hash gate, attached dependency unsafe-code report and install guidance.
The repository already uses a hand-authored release workflow and does not need cargo-dist's
publishing and installer pipeline for this milestone.

## Decision

Keep the workflow hand-authored and use pinned `cargo-zigbuild` 0.24.0 with Zig 0.17.0 to build
the workspace's three release executables for both Linux musl targets on the large Linux runner.
This is the equivalent distribution build path permitted by SPEC and E10.4. Keep publication
separate from building, gated by a version tag, explicit non-dry-run dispatch when applicable,
and `RELEASE_PUBLISH_ENABLED=true`.

Build each target twice using the same pinned toolchain, `SOURCE_DATE_EPOCH`, disabled Cargo
incremental compilation and remapped source paths. Compare SHA-256 for each executable and reject
dynamic binaries using `file` plus `ldd` on native x86_64. Publish per-target tar archives,
archive checksums, and the `cargo-geiger` JSON report as release assets. Exercise build and report
jobs in pull requests that modify release workflow/build inputs.

Do not add macOS targets: SPEC 13.2 specifies static release builds on musl/Linux only.

## Consequences

- Releases run on one Linux runner profile and produce x86_64 and aarch64 musl archives.
- Zig makes the aarch64 cross-link available without a dedicated ARM runner.
- The workflow owns reproducibility checks and release asset layout; cargo-dist's installers and
  generated release orchestration are not included.
- Any future macOS release requirement or installer ecosystem needs a separate decision.

## References

- [SPEC 13.2: Dependencies](../../SPEC.md#132-dependencies)
- [E10.4: Release](../plan/EPICS.md#e10-calibration-bench-performance-gates-and-release-spec-85-14-15-m0m8-132)
- [Release installation guide](../release.md)
- [cargo-zigbuild README](https://docs.rs/crate/cargo-zigbuild/0.24.0/source/README.md)
- [cargo-geiger README](https://github.com/geiger-rs/cargo-geiger#readme)
