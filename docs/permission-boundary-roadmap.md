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

`./install.sh --permissions PATH` writes this locked MCP registration for Codex
without hand-editing `config.toml`; running `./install.sh` without that flag
keeps the default UI-owned open registration.

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
  launches. The standalone CLI can also generate and validate ceiling files for
  hosts that do not have the Codex for Linux UI.
- The CLI also accepts a leading `--permissions PATH` global option. When used,
  profile and workspace actions are checked against the same ceiling. This is
  intended for the Codex for Linux bridge when it discovers a locked MCP server
  config and needs to avoid bypassing that ceiling.

## Gates Before Hard Enforcement

Current gate status on 2026-05-25:

- A is validated for the current X11/bubblewrap runtime surface covered by the
  integration smoke. Real MCP dogfood and `scripts/integration_smoke.sh` have
  covered Chrome, native browser text input, local-dev browser QA, mounted GUI
  editor save-through, synthetic browser-session startup, Codex desktop feature
  tests, disabled networking, local-only networking, read-only/read-write
  mounts, setup/startup commands, screenshots, window targeting, input,
  clipboard, app logs, events, manifests, stop, stale cleanup, daemon-crash
  recovery, self-stop from inside
  a workspace app, direct MCP stop/revoke cleanup, and consistent workspace
  discovery when a Codex/MCP launcher omits `XDG_RUNTIME_DIR`, and MCP daemon
  child cleanup so stopped workspaces do not leave zombies under a long-lived
  MCP process. A
  Codex-spawned MCP pass on this repository also revalidated project-dev
  mounts, Rust toolchain access, GUI input, events, Chrome, and current network
  enforcement. `cargo test` currently passes 56 tests.
- A still has known product gaps: host-localhost bridging for `local_only` and
  more varied real-project coverage. Broad network allowlists and egress proxy
  filtering are out of scope for this pass; the product network model is
  closed, local, or open.
- B has a first Codex for Linux slice: the conversation surface can show a live
  active-workspace panel with screenshot, profile/policy/app metadata, Stop, and
  Revoke. The side-by-side dev app and the installed user-facing app have both
  been dogfooded inside a hidden workspace. The launcher now avoids inherited
  renderer URLs, the live panel appears in the conversation view, and it hides
  on Settings pages where the dedicated Agent Workspaces controls are shown. The
  installed app was verified through Chrome DevTools Protocol with real MCP
  bridge calls, a live screenshot, and Refresh interaction. A later installed
  app dogfood pass verified the polished viewer with Refresh/Details/Stop/
  Revoke controls, a Details tray for active window/running app/hidden display,
  and an embedded Stop action that stops the workspace from inside the app. B
  still needs the final UI approval boundary before it should become a hard
  trust boundary.
- C is partially covered. Desktop QA, local-dev browser QA, arbitrary startup
  app configuration, PID-less arbitrary app window targeting, and
  recovery/inspection flows work at the primitive level.
  MCP-locked permission ceilings and app allowlists have a first MCP-enforced
  slice, and `./install.sh --permissions PATH` now gives locked MCP hosts a
  repeatable setup path without hand-editing Codex config. The CLI also has
  `permissions template open|closed|local` and
  `permissions validate --json PATH` so non-Codex hosts can generate and check a
  ceiling before spawning MCP. The Codex for Linux app picker now accepts both
  executable files and `.desktop` launchers, parsing launcher `Name`/`Exec`
  fields into startup app commands without a shell.
  Authenticated browser-profile sharing now has a `browser-session` starter
  template and a first Codex for Linux picker/copy/lock-warning flow for
  explicitly user-approved browser data directories. The installed MCP path has
  also proven that template end to end with a synthetic Chrome profile: approval
  preview, real startup, visible Chrome window, mounted browser-data read/write,
  screenshots/observe/artifacts, stop, profile deletion, and stale cleanup. Live
  real-account dogfood is still needed before making that path the default
  recommendation for shopping-style tasks.

### A. Prove Runtime Claims With Real Workloads

Before making permissions hard, validate that the current claims actually hold
under real usage:

Current evidence is tracked in [Dogfood Validation](dogfood-validation.md).

- Validated: Chrome/Chromium launches inside the agent workspace and is
  controllable through workspace-local window, keyboard, and paste operations
  without stealing the host desktop.
- Validated: workspace QA has run against this repo, Codex for Linux, and
  `agent-chrome-bridge`, including local dev server/browser paths and
  project-mounted test commands.
- Validated: `network.mode=disabled` blocks external socket/DNS access from
  workspace-launched commands and browser windows when bubblewrap is available.
- Validated: `network.mode=local_only` allows sandbox loopback while blocking
  external network access. Host-localhost bridging remains a documented
  limitation.
- Validated: read-write mounts accept writes and read-only mounts reject writes
  through the bubblewrap mount namespace.
- Validated: screenshots, window listing, input, clipboard, app logs, events,
  artifacts, stop, stale cleanup, and stopped-manifest inspection work across
  successful and failed app launches.
- Validated: daemon-crash recovery removes manifest-recorded orphan app process
  groups and X11 runtime processes.
- Keep the user-facing network model to closed, local, or open. Do not treat
  broad host allowlists or egress proxy filtering as part of this gate.

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
- Browser-centered tasks such as shopping or web workflows, including the
  `browser-session` starter for mounted browser data when the user explicitly
  grants that environment.
- Arbitrary apps chosen through file pickers or configured startup apps,
  including Linux `.desktop` launchers in the Codex for Linux app picker.
- Long-running auto-loop agents that need preconfigured network/file/app
  ceilings without Codex-specific UI.
- Recovery and inspection flows: list active/stopped workspaces, inspect
  artifacts, read logs/events, stop, cleanup, and delete saved environments.

Hard enforcement should be implemented only after these gates have been tested
well enough that failures are known product gaps rather than permission-system
surprises.
