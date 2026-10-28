# Release and benchmark workflows

The release workflow runs only for `v*` tag pushes or manual dispatch. It builds the three
x86_64 Linux musl binaries twice in separate clean target directories with the pinned Rust
toolchain, the commit timestamp as `SOURCE_DATE_EPOCH`, remapped Cargo and workspace paths,
and incremental compilation disabled. It checks static linking and compares each pair's SHA-256;
a mismatch fails the job. The artifact contains `tkm`, `tkm-hook`, `tkmd`, and `SHA256SUMS`.
The release profile uses thin LTO and one codegen unit for reproducibility, at the cost of longer
builds; panic unwinding remains enabled for the hook's eventual panic handler.

Publishing remains disabled until E10: the repository variable `RELEASE_PUBLISH_ENABLED` must
be set to `true` before a tag push or a manual run on a tag with `dry_run` set to false can
create a GitHub Release. Manual dispatch defaults to dry-run. Plain Cargo is used for this
skeleton while E10.4 evaluates cargo-dist and final distribution requirements.

The benchmark workflow compares the Criterion placeholder and hyperfine's mean hook startup
against the latest cached main-branch results. Main pushes save a new immutable cache; PRs read
but never save it, and fail when either result exceeds 120% of the corresponding baseline.
A PR with no baseline passes; the gate can only be exercised after a successful main run has
saved one. Cache eviction likewise leaves no baseline until main runs again. Shared runners are
noisy, especially for the current empty hook; E4.8 and E10.3 should replace the placeholder and
tune the gate and performance budgets. The hyperfine benchmark runs the hook with empty stdin;
it is not yet the daemon round trip or a p99 check.
