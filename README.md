# agent-workspace-linux

Isolated Linux desktop workspaces for AI agents.

This project is intentionally separate from `computer-use-linux`. The existing
MCP controls the user's current desktop. This repo is for an agent-owned
workspace that can launch and control apps without stealing the user's real
mouse, keyboard, focus, or active desktop.

## Initial Scope

The first target is a small X11-backed workspace:

- start an isolated display
- launch apps inside that display
- expose local IPC for status and control
- later add screenshots, input, window listing, and an embedded viewer

The key invariant is that workspace input must only target the agent workspace,
not the host desktop.

Profiles are persisted in a local JSON file under the user's config directory.
Profile mounts, network policy, and setup commands are stored as declared intent
for the future Codex app/profile UI. The runtime snapshots that intent into
`workspace status` with an enforcement report; this X11 runtime enforces
display/input scoping, profile mounts and disabled-network profiles through
bubblewrap when available, display size, launch cwd, and environment overrides
today.

For the current bubblewrap runtime, profile mount sources must use absolute host
paths, and mount destinations must be non-overlapping absolute paths under
`/workspace/`.

## Commands

```bash
cargo run -- doctor
cargo run -- profile path
cargo run -- profile list
cargo run -- profile template project-dev --host-path "$PWD"
cargo run -- profile put --json ./profile.json
cargo run -- profile get project-dev
cargo run -- profile check project-dev
cargo run -- profile delete project-dev
cargo run -- workspace start --ack-hidden-workspace --ack-unenforced-policy --profile project-dev
cargo run -- workspace start --ack-hidden-workspace --foreground
cargo run -- workspace list
cargo run -- workspace cleanup
cargo run -- workspace status
cargo run -- workspace launch --profile project-dev -- xterm
cargo run -- workspace launch --cwd "$PWD" --env AGENT_WORKSPACE=1 -- env
cargo run -- workspace windows
cargo run -- workspace screenshot --output /tmp/agent-workspace.png
cargo run -- workspace focus-window 4194316
cargo run -- workspace close-window 4194316
cargo run -- workspace click 100 120
cargo run -- workspace key Return
cargo run -- workspace type "hello from the agent workspace"
cargo run -- workspace logs --stream stdout app-12345
cargo run -- workspace wait-app --timeout-ms 30000 app-12345
cargo run -- workspace events --tail 20
cargo run -- workspace setup --profile project-dev
cargo run -- workspace kill-app app-12345
cargo run -- workspace stop
cargo run -- mcp
```

On Debian/Ubuntu-like systems, the initial X11 workspace runtime is expected to
need packages along these lines:

```bash
sudo apt install xvfb openbox xdotool xauth x11-utils imagemagick
```

`doctor` is implemented first so missing runtime dependencies are visible before
the workspace runtime grows. It also reports optional policy backend candidates
such as bubblewrap, firejail, unshare, and slirp4netns without treating them as
active enforcement. The workspace commands use a small local Unix socket daemon:

- `workspace start` requires `--ack-hidden-workspace` so the user explicitly
  acknowledges that a separate agent-controlled environment is being created.
  If the profile asks for mounts or restricted networking, the current X11
  runtime also requires `--ack-unenforced-policy` when any requested policy is
  visible but not enforced yet. Mount profiles and disabled-network profiles do
  not need that extra acknowledgement when bubblewrap is available because
  launches run inside a bubblewrap mount namespace and/or with
  `bwrap --unshare-net`.
  It then chooses a free X11 display, creates an `xauth` file, starts `Xvfb`,
  starts a lightweight window manager, and binds a control socket under
  `$XDG_RUNTIME_DIR/agent-workspace-linux/<id>/control.sock`. With `--profile`,
  profile width/height are applied unless explicit flags override them, and the
  profile's mounts/network/setup intent is snapshotted into status.
- `workspace start --foreground` runs the same workspace daemon in the current
  process, which is useful for MCP hosts or dev runners that clean up detached
  child processes.
- `workspace list` scans the runtime directory and reports which known
  workspaces are currently reachable.
- `workspace cleanup` removes stale workspace runtime directories while skipping
  running workspaces.
- `workspace launch` asks the daemon to spawn an app with the workspace
  `DISPLAY` and `XAUTHORITY`. It can also set a launch cwd and per-app
  environment overrides. With `--profile`, profile cwd/env are applied unless
  explicit flags override them, and profile mounts/network policy apply to that
  launched app. If a launch profile requests policy that remains unenforced, the
  launch requires `--ack-unenforced-policy`. Each launched app gets
  workspace-local stdout/stderr log files reported in `workspace status`.
  Profile-backed launches also report the profile id and effective
  mount/network isolation on the app entry. Completed apps report both a human
  `exit_status` string and structured `exit_code`/`exit_signal` fields.
- `workspace windows`, `workspace screenshot`, `workspace focus-window`,
  `workspace close-window`, `workspace click`, `workspace key`, `workspace type`,
  `workspace logs`, `workspace wait-app`, `workspace events`, `workspace setup`,
  and `workspace kill-app` inspect or act through the same daemon, scoped to the
  workspace display.
- `workspace events` reads a workspace-local JSONL event log for IPC actions.
  Typed text is logged as metadata such as character count, not raw text.
- `workspace setup --profile` launches the profile's setup commands as ordinary
  workspace apps; their status and logs are available through the same app
  status/log tools.
- `workspace status` reports the workspace profile id, launched apps, and app
  profile ids when a profile shaped the workspace or app. It also reports the
  hidden-workspace acknowledgement, unenforced-policy acknowledgement, applied
  policy snapshot, policy backend candidates discovered at start time, and
  which parts are currently enforced. `workspace status` and `workspace stop`
  talk to the same socket.

The MCP server currently exposes the same control surface: `workspace_doctor`,
`profile_path`, `profile_list`, `profile_get`, `profile_check`,
`profile_template`, `profile_put`, `profile_delete`, `workspace_start`,
`workspace_list`, `workspace_cleanup_stale`, `workspace_status`,
`workspace_launch_app`, `workspace_list_windows`, `workspace_screenshot`,
`workspace_focus_window`, `workspace_close_window`, `workspace_click`,
`workspace_key`, `workspace_type_text`, `workspace_read_app_log`,
`workspace_wait_app`, `workspace_events`, `workspace_run_profile_setup`,
`workspace_kill_app`, and `workspace_stop`.
