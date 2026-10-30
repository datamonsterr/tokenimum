# Contributing

Tests, builds, Clippy, benchmarks and documentation builds run only in GitHub Actions. Never
run `cargo build`, `cargo check`, `cargo test`, `cargo nextest`, `cargo clippy`, `cargo bench`,
or `cargo doc` locally. Local commands include `cargo fmt`, `cargo metadata`,
`cargo generate-lockfile`, `cargo update -p`, `cargo search`, `git`, and `gh`.

For behavior changes, commit failing tests first, then the implementation in a separate commit.
Check current documentation before using any crate, action, CLI, or harness. Consult the
[delivery rules and global definition of done](docs/plan/EPICS.md) for the full process.

Every crate root forbids unsafe code and libraries deny missing documentation. Workspace lints
also forbid unsafe and deny unwrap and expect outside tests. Dependencies using unsafe require a
justification and review in [the allowlist](docs/unsafe-allowlist.md); CI compares the list with
its scan and checks advisories, licenses, and sources.

Before merging, all checks must pass on the PR head, all epic DoD items need evidence, review
must have no open correctness issues, and the branch must be rebased on the latest main with CI
passing on that head. Read results with `gh pr checks <pr>` and `gh run view <id> --log-failed`.
Clean built worktrees with `cargo clean`, remove completed worktrees, and delete merged remote
branches as described in the delivery rules.

## Workflows

The daily audit also runs when `Cargo.lock` changes. Benchmarks run on main, pull requests, and
manual dispatch; the 20% PR regression gate needs a successful main run to establish a cached
baseline. The release build runs on version tags, manual dispatch, and PRs that change release build
inputs. Automatic publication requires the `RELEASE_PUBLISH_ENABLED` repository variable.
See [release workflow details](docs/release.md).
