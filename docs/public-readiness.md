# Public Readiness

Last updated: 2026-05-26

This repository is ready for public source review and local experimentation, but
it remains pre-1.0. The release gate is intentionally stricter than "tests pass"
because the project controls hidden Linux desktop workspaces and host-visible
viewer windows.

## Public Surface

- License is declared in `Cargo.toml` and tracked in `LICENSE`.
- Contribution, security, and CI entrypoints are present.
- Generated artifacts are ignored: `target/`, `.codex/`, local environment
  files, copied browser-profile manifests, and per-machine JSON examples.
- The internal GPUI implementation notes were replaced with the public
  [GPUI Viewer Direction](gpui-viewer-direction.md).
- The default MCP path remains host-controlled: starting `mcp` without
  `--permissions` does not invent a second permission ceiling.

## Validation Expectations

Run these before publishing a release candidate:

```bash
cargo fmt --check
cargo clippy --locked -- -D warnings
cargo test --locked
git diff --check
scripts/prod_readiness_smoke.sh
```

The broad smoke generates source-bound release reports under `target/`. Those
reports are local artifacts, not tracked documentation.

## Remaining Release Gates

The current automated gate has one manual blocker before a production tag:

- final human diff review marker generated after review

Use these scripts for the current source-bound answer:

```bash
scripts/release_gate_audit.py
scripts/final_review_bundle.py
scripts/objective_completion_audit.py
scripts/release_next_steps.py
```

Do not reuse timestamped paths, source hashes, or review-scope hashes from docs
after source or documentation edits. Regenerate the audit bundle instead.
