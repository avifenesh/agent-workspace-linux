# Permission Boundary Roadmap

This project is currently in developer-open dogfood mode. The existing
acknowledgement parameters and approval bundles are useful API shape and audit
metadata, but they are not a final human permission boundary while the MCP tools
are directly available to an agent.

Hard permission enforcement should wait until the core workspace flows are
validated end to end. Until then, keep development easy and preserve the
approval-bundle contract so the final boundary can be added without reshaping
the product.

## Target Authority Model

There are two permission owners.

### UI-Owned Mode

When the MCP is spawned without permission fields, or a permission dimension is
empty, that dimension is open at the MCP layer. Codex for Linux owns the user
approval flow, profile editing UX, and per-session decisions.

This mode is intended for the Codex app integration and for developer dogfood.
The app can request workspace starts, mounts, network modes, setup commands, and
apps through its own UI approval flow.

### MCP-Locked Mode

When the MCP is spawned with permission fields through
`agent-workspace-linux mcp --permissions PATH`, those fields become a hard
ceiling for the lifetime of that MCP server process. Codex for Linux may show
the policy, request narrower access, and operate inside it, but it must not
broaden or rewrite it.

This mode supports non-Codex hosts such as Claude Code, auto-looping agents, and
headless workflows where the user preconfigures permissions in MCP config.

Example shape:

```json
{
  "network": {
    "mode": "local_only",
    "allow_hosts": ["localhost:3000"]
  },
  "mounts": [
    {
      "host_path": "/home/me/project",
      "workspace_path": "/workspace/project",
      "mode": "read_write"
    }
  ],
  "apps": {
    "allow": ["/usr/bin/firefox", "/usr/bin/xterm", "/usr/bin/npm"]
  }
}
```

Possible MCP config shape:

```json
{
  "mcpServers": {
    "agent-workspace-linux": {
      "command": "/home/me/.local/bin/agent-workspace-linux",
      "args": [
        "mcp",
        "--permissions",
        "/home/me/.config/agent-workspace-linux/permissions.json"
      ]
    }
  }
}
```

Rules:

- Missing or empty permission fields mean no MCP-level ceiling for that
  dimension.
- Prefilled `network` is the maximum network access available to all workspace
  starts and launches.
- Prefilled `mounts` are the maximum file access. A UI may narrow mounts or
  downgrade read-write to read-only, but cannot add broader paths or upgrade
  access.
- Prefilled app allowlists limit launchable commands. The UI may show friendlier
  app pickers, but launches outside the ceiling are rejected. The allowlist
  matches the launched program, not its arguments; allowing shells, package
  managers, or browsers delegates follow-on behavior to that program inside the
  workspace policy.
- Spawn-time MCP permissions are immutable. Changing them requires restarting
  the MCP server with new config.
- The active ceiling is visible through the read-only `mcp_permissions` tool.
- Enforcement currently covers MCP profile template/check/validate/put/import,
  workspace start/open-profile, direct launch/run, and profile setup/startup
  launches. The standalone CLI remains a developer/admin surface.

## Gates Before Hard Enforcement

Current gate status on 2026-05-24:

- A is mostly validated for the current X11/bubblewrap runtime. Real MCP dogfood
  has covered Chrome, local-dev browser QA, Codex desktop feature tests, disabled
  networking, local-only networking, read-only/read-write mounts, setup/startup
  commands, screenshots, window targeting, input, clipboard, app logs, events,
  manifests, stop, stale cleanup, and daemon-crash recovery.
- A still has known product gaps: host-localhost bridging for `local_only`,
  network allowlist enforcement, and more varied real-project coverage.
- B has a first Codex for Linux slice: the conversation surface can show a live
  active-workspace panel with screenshot, profile/policy/app metadata, Stop, and
  Revoke. The side-by-side dev app has been dogfooded inside a hidden workspace,
  including a live panel Stop action. It still needs main-app install QA and more
  repeated live app runs before it should become a hard trust boundary.
- C is partially covered. Desktop QA, local-dev browser QA, arbitrary startup
  app configuration, and recovery/inspection flows work at the primitive level.
  MCP-locked permission ceilings and app allowlists have a first MCP-enforced
  slice. Authenticated browser-profile sharing and user-friendly
  environment/profile creation are still open.

### A. Prove Runtime Claims With Real Workloads

Before making permissions hard, validate that the current claims actually hold
under real usage:

Current evidence is tracked in [Dogfood Validation](dogfood-validation.md).

- Start Chrome inside the agent workspace and verify it is controllable without
  stealing the host desktop.
- Run workspace QA against several real user projects, including apps that need
  local dev servers, build tools, and browser testing.
- Verify `network.mode=disabled` really blocks external network access from
  workspace-launched commands and browsers.
- Verify `network.mode=local_only` allows loopback inside the workspace while
  blocking internet access, and document the current host-localhost bridging
  limitation.
- Verify mount policies with both read-only and read-write paths, including
  failed writes to read-only mounts.
- Verify screenshots, window listing, input, clipboard, app logs, events, and
  cleanup across both successful and failed app launches.
- Verify daemon-crash recovery cleans manifest-recorded orphan app process
  groups and X11 runtime processes before hard permission enforcement.
- Keep network allowlist marked as declared intent until a real filtering
  backend exists and is tested.

### B. Embed Workspace Visibility In Conversation

Before final permission hardening, the user should be able to see what the agent
is doing without leaving the conversation flow:

- Embed the active workspace view in the Codex conversation surface.
- Show when a hidden workspace is active, which profile/policy is applied, and
  which apps are running.
- Provide obvious stop/revoke controls near the embedded view.
- Surface screenshots or live view updates from the workspace, not only status
  JSON.
- Keep the settings page for saved environments, but make the conversation view
  the place where live agent activity is visible.

### C. Validate Capability Coverage

Before locking permissions down, confirm the primitive set covers the optional
tasks users may reasonably ask for:

- Desktop app QA with project mounts, setup commands, local dev servers, browser
  testing, screenshots, logs, and cleanup.
- Browser-centered tasks such as shopping or web workflows, including profiles
  or mounted browser data when the user explicitly grants that environment.
- Arbitrary apps chosen through file pickers or configured startup apps.
- Long-running auto-loop agents that need preconfigured network/file/app
  ceilings without Codex-specific UI.
- Recovery and inspection flows: list active/stopped workspaces, inspect
  artifacts, read logs/events, stop, cleanup, and delete saved environments.

Hard enforcement should be implemented only after these gates have been tested
well enough that failures are known product gaps rather than permission-system
surprises.
