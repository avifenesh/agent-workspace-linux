# Public Readiness

Last updated: 2026-05-26

This repository is ready for public source review and local experimentation, but
it remains pre-1.0. The contributor gates are intentionally stricter than "tests
pass" because the project controls hidden Linux desktop workspaces and
host-visible viewer windows.

## Public Surface

- License is declared in `Cargo.toml` and tracked in `LICENSE`.
- Contribution, security, and CI entrypoints are present.
- Generated artifacts are ignored: `target/`, `.codex/`, local environment
  files, copied browser-profile manifests, and per-machine JSON examples.
- The internal GPUI implementation notes were replaced with the public
  [GPUI Viewer Direction](gpui-viewer-direction.md).
- The default MCP path remains host-controlled: starting `mcp` without
  `--permissions` does not invent a second permission ceiling.

## Contributor Gates

Run these before opening a pull request:

```bash
cargo fmt --check
cargo clippy --locked -- -D warnings
cargo test --locked
git diff --check
scripts/integration_smoke.sh
```

`scripts/integration_smoke.sh` exercises the workspace lifecycle and MCP smokes
against a real hidden workspace; it skips the browser sections when
Chrome/Chromium is not installed.

## Releases

Releases are cut by the GitHub Actions workflows in `.github/workflows/`
(`release.yml` and `npm-release.yml`). Tagging a release runs those workflows;
there is no separate local release-gate step to run by hand.
