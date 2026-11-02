---
name: release-management
description: Cut or publish a new tokenimum version, bump the version, tag a release, or prepare release notes or a changelog.
---

# Release management

Read `docs/release.md` and `.github/workflows/release.yml` before proceeding. Respect the user's authorization for publishing and do not change repository secrets or `RELEASE_PUBLISH_ENABLED` without authorization. Run Rust builds, tests, clippy, and benchmarks only in GitHub Actions.

1. Bump `[workspace.package]` version in `Cargo.toml` and hard-coded version strings such as `clientInfo.version` in `crates/tkm-integrations/src/superorchestrator.rs`. Update `Cargo.lock` to match. Prepare a changelog or release notes from the merged changes.
2. Open a PR, watch its CI and release dry-run builds, fix failures, and merge after required checks and approvals.
3. After merge, check out and update `main`, confirm the release commit and check `gh variable get RELEASE_PUBLISH_ENABLED`. The tag workflow publishes only when that value is `true`; do not silently change it. With authorization, create and push `vX.Y.Z` on the merged main commit (`git tag vX.Y.Z` and `git push origin vX.Y.Z`).
4. Watch the `Release` workflow with `gh run list --workflow release.yml` and `gh run watch RUN_ID`. It double-builds four target archives, produces checksums and geiger reports, then creates a release with generated notes. Review and update the release notes as needed. The workflow does not mark prereleases automatically: ensure the release is marked prerelease when applicable. GitHub `latest` selects stable releases only, so use an explicit `--version` for a prerelease.
5. Verify `tokenimum-<target>.tar.gz` and `.sha256` for `x86_64-unknown-linux-musl`, `aarch64-unknown-linux-musl`, `x86_64-apple-darwin`, and `aarch64-apple-darwin`, plus the geiger reports. Download and check each archive checksum and its contained `SHA256SUMS`. Test the README Linux, macOS, and Windows/WSL one-liners against the published release on appropriate machines, with private-repository authentication as documented.
6. If rollback is required and authorized, delete the GitHub release and remote tag with `gh release delete vX.Y.Z --yes --cleanup-tag`; delete any corresponding local tag with `git tag -d vX.Y.Z`. Correct the issue before publishing a replacement.
