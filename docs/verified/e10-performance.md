# Performance tooling verification

Checked 2026-10-29:

- [hyperfine 2.0 README](https://github.com/sharkdp/hyperfine/blob/v2.0.0/README.md): process benchmarking and JSON export.
- [github-action-benchmark](https://github.com/benchmark-action/github-action-benchmark): custom smaller-is-better JSON, external baseline and alert threshold.
- [Criterion.rs documentation](https://criterion-rs.github.io/book/): statistical Rust microbenchmarks.
- [Bun process spawning](https://bun.com/docs/runtime/child-process) and [Bun shell](https://bun.com/docs/runtime/shell): the test harness runs hook processes and the opencode shell callback.
- [setup-bun](https://github.com/oven-sh/setup-bun/blob/main/action.yml): pin a Bun version on the runner.
- [Criterion benchmark groups](https://docs.rs/criterion/latest/criterion/struct.Criterion.html#method.benchmark_group): grouped benchmark configuration.
- [OpenCode plugin documentation](https://opencode.ai/docs/plugins/) and [Pi extension documentation](https://github.com/earendil-works/pi/blob/main/packages/coding-agent/docs/extensions.md): shim event interfaces.
- [GitHub Actions cache reference](https://docs.github.com/en/actions/reference/workflows-and-actions/dependency-caching): pull requests can restore base-branch caches.
