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
Profile mounts, network policy, setup commands, and startup apps are stored as
declared intent for the future Codex app/profile UI. The runtime snapshots that
intent into `workspace status` with an enforcement report; this X11 runtime
enforces display/input scoping, profile mounts and disabled-network profiles
through bubblewrap when available, display size, launch cwd, and environment
overrides today.

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
cargo run -- workspace open-profile --ack-hidden-workspace --profile project-dev --setup --setup-timeout-ms 30000
cargo run -- workspace start --ack-hidden-workspace --foreground
cargo run -- workspace list
cargo run -- workspace cleanup
cargo run -- workspace status
cargo run -- workspace launch --profile project-dev -- xterm
cargo run -- workspace run --timeout-ms 30000 --tail-bytes 65536 -- cargo test
cargo run -- workspace launch-profile-apps --profile project-dev
cargo run -- workspace launch --cwd "$PWD" --env AGENT_WORKSPACE=1 -- env
cargo run -- workspace windows
cargo run -- workspace windows --all
cargo run -- workspace active-window
cargo run -- workspace observe --screenshot --output /tmp/agent-observe.png
cargo run -- workspace observe --all-windows
cargo run -- workspace wait-window --title xterm --timeout-ms 10000
cargo run -- workspace screenshot --output /tmp/agent-workspace.png
cargo run -- workspace screenshot-window --title xterm --output /tmp/agent-window.png
cargo run -- workspace focus-window 4194316
cargo run -- workspace focus-window --title xterm --timeout-ms 10000
cargo run -- workspace close-window 4194316
cargo run -- workspace close-window --title xterm --timeout-ms 10000
cargo run -- workspace move-window --title xterm 80 60
cargo run -- workspace resize-window --title xterm 800 500
cargo run -- workspace raise-window --title xterm
cargo run -- workspace minimize-window --title xterm
cargo run -- workspace show-window 4194316
cargo run -- workspace click 100 120
cargo run -- workspace click --button 3 100 120
cargo run -- workspace click-window --title xterm 24 32
cargo run -- workspace click-window --title xterm --count 2 24 32
cargo run -- workspace move-pointer 100 120
cargo run -- workspace move-pointer-window --title xterm 24 32
cargo run -- workspace drag 100 120 180 180
cargo run -- workspace drag-window --title xterm 24 32 180 32
cargo run -- workspace scroll --amount 3 100 120 down
cargo run -- workspace scroll-window --title xterm --amount 3 24 32 down
cargo run -- workspace key Return
cargo run -- workspace key-window --title xterm Return
cargo run -- workspace type "hello from the agent workspace"
cargo run -- workspace type-window --title xterm "hello from the agent workspace"
cargo run -- workspace clipboard-set "hello from the agent workspace"
cargo run -- workspace clipboard-get
cargo run -- workspace paste-window --title Editor "hello from the agent workspace"
cargo run -- workspace logs --stream stdout app-12345
cargo run -- workspace wait-app --timeout-ms 30000 app-12345
cargo run -- workspace events --tail 20
cargo run -- workspace setup --profile project-dev --wait --timeout-ms 30000
cargo run -- workspace kill-app app-12345
cargo run -- workspace stop
cargo run -- mcp
```

On Debian/Ubuntu-like systems, the initial X11 workspace runtime is expected to
need packages along these lines:

```bash
sudo apt install xvfb openbox xdotool xauth x11-utils imagemagick xclip
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
- `workspace open-profile --profile` starts a profile-backed workspace,
  optionally runs setup first with `--setup`, and then launches declared startup
  apps only when setup succeeds, returning the workspace start, setup, and
  startup results plus a top-level `ready` flag in one response.
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
- `workspace run` is a QA-friendly launch helper that launches an app, waits for
  completion or timeout, and returns stdout/stderr log content with structured
  completion fields in one response.
- `workspace windows`, `workspace active-window`, `workspace observe`,
  `workspace wait-window`, `workspace screenshot`, `workspace screenshot-window`,
  `workspace focus-window`, `workspace close-window`, `workspace move-window`,
  `workspace resize-window`, `workspace raise-window`,
  `workspace minimize-window`, `workspace show-window`, `workspace click`,
  `workspace click-window`, `workspace move-pointer`,
  `workspace move-pointer-window`, `workspace drag`, `workspace drag-window`,
  `workspace scroll`, `workspace scroll-window`, `workspace key`,
  `workspace key-window`, `workspace type`, `workspace type-window`,
  `workspace clipboard-set`, `workspace clipboard-get`, `workspace paste`,
  `workspace paste-window`, `workspace logs`, `workspace wait-app`,
  `workspace events`, `workspace setup`, and
  `workspace kill-app` inspect or act through the same daemon, scoped to the
  workspace display. `windows` lists visible windows by default; `--all`
  includes minimized/hidden windows with a `visible` flag so they can be shown
  again by id. `active-window` reports the current workspace-local focus,
  and `observe` returns status, windows, active window, and optionally a root
  screenshot in one IPC call. `observe --all-windows` uses the same
  hidden-window listing as `windows --all`. `focus-window`, `screenshot-window`,
  `close-window`, `move-window`, `resize-window`, `raise-window`,
  `minimize-window`, `key-window`, and `type-window` can use either a raw X11
  window id or the same title/pid/app filters as `wait-window`; app filters
  match the launched process and its child processes. `move-window` and
  `resize-window` update the returned window geometry so screenshots and
  interactions can be staged predictably. `raise-window`, `minimize-window`,
  and `show-window` manage visibility and stacking without terminating apps.
  `move-pointer` and
  `move-pointer-window` move the workspace pointer without clicking. `click` and
  `click-window` can set button/count for right-clicks and double-clicks.
  `drag` and `drag-window` can set the mouse button for press/move/release
  gestures. `scroll` and `scroll-window` send wheel ticks in the requested
  direction. Window-targeted pointer tools resolve the same targets and use
  window-relative coordinates.
- `workspace clipboard-set` and `workspace clipboard-get` read/write the X11
  clipboard selection inside the workspace using `xclip` or `xsel`. Clipboard
  set events record only size metadata, not the raw clipboard text.
- `workspace paste` and `workspace paste-window` set the workspace clipboard,
  then send a paste key chord, defaulting to `ctrl+v`. Paste events record only
  size metadata, not the raw pasted text.
- `workspace events` reads a workspace-local JSONL event log for IPC actions.
  App launches and exits are recorded with structured metadata. Typed text is
  logged as metadata such as character count, not raw text.
- `workspace setup --profile` launches the profile's setup commands as ordinary
  workspace apps; with `--wait`, commands are supervised in sequence and the
  result reports whether they completed and exited successfully. Their status
  and logs are available through the same app status/log tools. If setup uses a
  profile with policy that remains unenforced, it requires
  `--ack-unenforced-policy`.
- `workspace launch-profile-apps --profile` launches the profile's declared
  startup apps as ordinary workspace apps, preserving profile cwd/env and policy.
  If startup apps use a profile with policy that remains unenforced, they
  require `--ack-unenforced-policy`.
- `workspace status` reports the workspace profile id, launched apps, and app
  profile ids when a profile shaped the workspace or app. It also reports the
  start timestamp, hidden-workspace acknowledgement, unenforced-policy
  acknowledgement, applied policy snapshot, policy backend candidates discovered
  at start time, and which parts are currently enforced. `workspace status` and
  `workspace stop` talk to the same socket.

The MCP server currently exposes the same control surface: `workspace_doctor`,
`profile_path`, `profile_list`, `profile_get`, `profile_check`,
`profile_template`, `profile_put`, `profile_delete`, `workspace_start`,
`workspace_open_profile`, `workspace_list`, `workspace_cleanup_stale`,
`workspace_status`, `workspace_launch_app`, `workspace_run_app`,
`workspace_launch_profile_apps`, `workspace_list_windows`,
`workspace_active_window`, `workspace_observe`, `workspace_wait_window`,
`workspace_screenshot`, `workspace_screenshot_window`, `workspace_focus_window`,
`workspace_focus_matching_window`, `workspace_close_window`, `workspace_click`,
`workspace_close_matching_window`, `workspace_move_window`,
`workspace_resize_window`, `workspace_raise_window`,
`workspace_minimize_window`, `workspace_show_window`, `workspace_click_window`,
`workspace_move_pointer`, `workspace_move_pointer_window`, `workspace_drag`,
`workspace_drag_window`, `workspace_scroll`, `workspace_scroll_window`,
`workspace_key`, `workspace_key_window`, `workspace_type_text`,
`workspace_type_window`, `workspace_set_clipboard`, `workspace_get_clipboard`,
`workspace_paste_text`, `workspace_paste_window`, `workspace_read_app_log`,
`workspace_wait_app`, `workspace_events`, `workspace_run_profile_setup`,
`workspace_kill_app`, and `workspace_stop`. `workspace_list_windows` and
`workspace_observe` accept `include_hidden=true` to return minimized/hidden
windows as well as visible windows.
