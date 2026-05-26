# Security Policy

`agent-workspace-linux` creates isolated Linux desktop workspaces for agents, so
security reports should focus on boundary escapes, unsafe defaults, credential
exposure, permission-ceiling bypasses, or real-world action approval failures.

## Supported Versions

This project is pre-1.0. Security fixes target `main` until release branches
exist.

## Reporting

Please report vulnerabilities privately through the repository owner's preferred
GitHub security channel. Do not include live credentials, copied browser
profiles, private logs, or raw account page contents in public issues.

Helpful reports include:

- the commit or release you tested
- Linux distribution and display session type
- the exact MCP or CLI command used
- whether the MCP was started with `--permissions`
- the output of `agent-workspace-linux doctor`, with local paths redacted if
  needed
- the smallest reproduction that shows the boundary failure

## Boundary Expectations

- With no `--permissions` file, the MCP does not add its own permission ceiling;
  the host/client harness owns the session boundary.
- With `--permissions PATH`, the configured file is an immutable ceiling for
  that MCP process.
- Workspace input, screenshots, windows, clipboard, and browser control should
  target the isolated workspace, not the user's host desktop or host Chrome.
- Browser-session profiles are for explicitly user-approved browser data only.
  Use copied/disposable profiles for real-account dogfood whenever possible.
