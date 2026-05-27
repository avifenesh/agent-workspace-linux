# Security Policy

`agent-workspace-linux` creates isolated Linux desktop workspaces for agents, so
security reports should focus on boundary escapes, unsafe defaults, credential
exposure, permission-ceiling bypasses, or real-world action approval failures.

## Supported Versions

| Version | Supported |
|---------|-----------|
| 0.1.x   | Yes       |

This project is pre-1.0. Security fixes target `main` until release branches
exist. Only the current `0.1.x` line receives fixes.

## Reporting

Please report vulnerabilities **privately** using
[GitHub private security advisories](https://github.com/agent-sh/agent-workspace-linux/security/advisories/new)
for this repository. Do not open a public issue for security reports.

Do not include live credentials, copied browser profiles, private logs, or raw
account page contents in any report.

Helpful reports include:

- the commit or release you tested
- Linux distribution and display session type (X11 or Wayland)
- the exact MCP or CLI command used
- whether the MCP was started with `--permissions`
- the output of `agent-workspace-linux doctor`, with local paths redacted if needed
- the smallest reproduction that shows the boundary failure

## Trust Model

- **Control socket**: the workspace control socket is a same-uid Unix socket
  with mode 0600. It provides no cross-user protection by design — any process
  running as the same UID can connect. Running the server as a dedicated
  isolated user is the recommended mitigation in multi-user environments.
- **Permission ceiling**: with a `--permissions PATH` file (or the
  `AGENT_WORKSPACE_PERMISSIONS` environment variable) the configured ceiling is
  the authoritative boundary for that MCP process, enforced at both the MCP
  front-end and the workspace daemon's IPC socket and unchangeable without
  restarting the process. Without it, the MCP adds no ceiling of its own; the
  host/client harness owns the session boundary.
- **Live viewer control**: the GPUI monitor allows a human operator to pause or
  stop agent actions. This is best-effort — it does not provide a hard
  cryptographic guarantee against a racing agent action before the control
  signal is received.
- **Workspace isolation**: workspace input, screenshots, windows, clipboard, and
  browser control should target the isolated workspace, not the user's host
  desktop or host Chrome. Leakage to the host display is a reportable boundary
  violation.
- **Browser profiles**: browser-session profiles are for explicitly
  user-approved browser data only. Use copied/disposable profiles for
  real-account dogfood whenever possible.
