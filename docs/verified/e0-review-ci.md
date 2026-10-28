# E0 review CI configuration verification

Checked 2026-10-29:

- [Clippy `assert_is_empty`](https://rust-lang.github.io/rust-clippy/master/index.html#assert_is_empty) covers positive and negated checks on supported types.
- [cargo-deny advisories](https://embarkstudios.github.io/cargo-deny/checks/advisories/cfg.html) defines `unsound = "all"` to fail on any matching crate.
- [Cargo metadata](https://doc.rust-lang.org/cargo/commands/cargo-metadata.html) provides workspace members and target `src_path`, including custom builds.
- [benchmark action](https://github.com/benchmark-action/github-action-benchmark#github-token-optional) accepts `github-token`; `auto-push` defaults to false.
- [upload-artifact](https://github.com/actions/upload-artifact) documents `actions/upload-artifact@v7` and multiple paths.
- [cargo-geiger 0.14.0 source](https://docs.rs/crate/cargo-geiger/0.14.0/source/) defines package IDs and the used counters.
