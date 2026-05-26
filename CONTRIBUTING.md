# Contributing

Thanks for taking a look. This project is still pre-1.0, so small focused
changes with strong evidence are much easier to review than broad rewrites.

## Local Setup

On Debian/Ubuntu-like systems:

```bash
sudo apt install xvfb openbox xdotool xauth x11-utils imagemagick xclip bubblewrap pkg-config libxkbcommon-x11-dev
cargo build --locked
cargo run -- doctor
```

## Checks

Run the focused checks for ordinary changes:

```bash
cargo fmt --check
cargo clippy --locked -- -D warnings
cargo test --locked
git diff --check
```

For runtime, MCP, permission, viewer, or release-gate changes, also run:

```bash
scripts/prod_readiness_smoke.sh
```

When a real GPUI viewer is already open and you do not want the smoke to open a
second monitor:

```bash
AGENT_WORKSPACE_NO_NEW_VIEWER=1 scripts/prod_readiness_smoke.sh
```

## Public Hygiene

- Do not commit `target/`, `.codex/`, copied browser profiles, local MCP config,
  real account data, or generated release evidence.
- Keep browser/account dogfood reports metadata-only. Raw logged-in page text,
  links, headings, and account details do not belong in tracked files.
- Keep default MCP usage clean: no `--permissions` means no MCP-level ceiling;
  locked permission mode is opt-in through an explicit permissions file.
- Prefer repo-owned MCP/CLI evidence over host browser bridges or external
  automation when validating this runtime.
