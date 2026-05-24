# Dogfood Validation

This file records real MCP dogfood results that gate the later permission
hardening work. It is intentionally evidence-oriented: verified behavior goes
here, while policy design stays in `permission-boundary-roadmap.md`.

## 2026-05-24 MCP Pass

Environment:

- Codex used the installed `agent-workspace-linux` MCP tools.
- `workspace_doctor` reported Xvfb, openbox, xauth, xdotool, screenshot tools,
  xclip, and bubblewrap ready.
- The pass started with no active workspaces and no saved profiles.

Verified:

- `network.mode=local_only` with `require_enforced_policy=true` started without
  unenforced-policy acknowledgement. App launches reported
  `network_isolation=bubblewrap_loopback_only`. A Python socket probe could use
  sandbox loopback and could not connect to `1.1.1.1:80`.
- `network.mode=disabled` with `require_enforced_policy=true` started without
  unenforced-policy acknowledgement. App launches reported
  `network_isolation=bubblewrap_unshare_net`. Direct IP and DNS-resolved
  outbound socket probes were blocked.
- Chrome launched inside the hidden workspace with `wait_window` and
  `screenshot_window`. Window discovery found a visible `Google-chrome` window.
  Targeted `ctrl+l`, targeted clipboard paste, and `Return` navigated the
  workspace Chrome window without host desktop interaction.
- A project mount at `/workspace/project` with `mode=read_write` allowed writes
  through the bubblewrap mount namespace.
- Rust QA worked when the profile explicitly mounted the project read-write,
  mounted `/home/avifenesh/.cargo` and `/home/avifenesh/.rustup` read-only, and
  set `CARGO_HOME`, `RUSTUP_HOME`, and `PATH` to those mounted locations. In
  that environment, `cargo test --quiet` passed for the then-current 21 tests.
- A project mount at `/workspace/project` with `mode=read_only` rejected writes
  with `Read-only file system`.
- `workspace_stop`, `workspace_cleanup_stale`, and profile deletion cleaned up
  all temporary workspaces and dogfood profiles after the pass.

Findings:

- Fixed after this pass: a workspace started with a profile applied mount/network
  policy to later unprofiled launches, but did not apply the profile `cwd` and
  `env`. The runtime now snapshots profile cwd/env at workspace start and uses
  them as the default launch context unless a launch explicitly supplies another
  profile, cwd, or env override.
- Expected product gap: build tools from user-local locations such as
  `~/.cargo` are not visible inside a restricted mount namespace unless the
  profile mounts them. The Codex app profile UI should make common toolchain
  mounts easy instead of forcing users to hand-write JSON.
- Existing limitation: `local_only` is a sandbox-local loopback namespace. It
  does not bridge host localhost services into the workspace.
- Existing limitation: network allowlists remain declared intent until a real
  filtering backend exists and is tested.
- Browser tasks that require logged-in sessions need an explicit browser
  profile/mount story. The Chrome smoke used a temporary isolated user data dir,
  not the user's authenticated browser profile.

Post-patch verification:

- Local `cargo test` passes 23 tests, including coverage for active profile
  cwd/env inheritance and explicit per-launch profile override.
