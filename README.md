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
through bubblewrap when available, local-only network profiles through a
bubblewrap loopback-only network namespace when available, display size, launch
cwd, and environment overrides today. Each enforcement report includes a
machine-readable state, active backend, limitations, and any required
acknowledgement so the app can show exactly what is enforced before it starts
the hidden environment.
`network.mode=local_only` is available for profile intent where workspace apps
should reach localhost or loopback services but not the internet; with
bubblewrap it is enforced as loopback-only inside the sandbox. Host-loopback
services are not bridged into that namespace yet, so services needed by the app
should be started inside the workspace or the profile should use `inherit_host`.
Profiles can also set `require_enforced_policy=true` to fail closed: if any
requested mount or network policy is not enforced by the current runtime, starts
and launches are rejected even when the caller passes the unenforced-policy
acknowledgement.

For the current bubblewrap runtime, profile mount sources must use absolute host
paths, and mount destinations must be non-overlapping absolute paths under
`/workspace/`.

The MCP server can run in open host-controlled mode, or with an optional
spawn-time permission ceiling loaded from JSON. The richer human approval
boundary in Codex for Linux is still being dogfooded. See
[Permission Boundary Roadmap](docs/permission-boundary-roadmap.md) for the
authority model and validation gates, and
[Dogfood Validation](docs/dogfood-validation.md) for the current evidence log.

## Commands

```bash
cargo run -- doctor
cargo run -- guardrails
cargo run -- mcp --permissions ./permissions.json
cargo run -- profile path
cargo run -- profile list
cargo run -- profile template project-dev --host-path "$PWD"
cargo run -- profile template restricted-chrome --browser-path /usr/bin/google-chrome
cargo run -- profile validate --json ./profile.json
cargo run -- profile put --json ./profile.json --dry-run
cargo run -- profile put --json ./profile.json
cargo run -- profile import --json ./profile.json --dry-run
cargo run -- profile put --json ./profile.json --dry-run --replace
cargo run -- profile put --json ./profile.json --replace
cargo run -- profile get project-dev
cargo run -- profile check project-dev
cargo run -- profile export project-dev --output ./profile-export.json
cargo run -- profile delete --dry-run project-dev
cargo run -- profile delete project-dev
cargo run -- workspace start --dry-run --profile project-dev
cargo run -- workspace start --ack-hidden-workspace --purpose "QA run" --ack-unenforced-policy --profile project-dev
cargo run -- workspace open-profile --dry-run --purpose "Project QA" --profile project-dev --setup --setup-timeout-ms 30000 --setup-kill-on-timeout --startup-wait-window --startup-screenshot-window
cargo run -- workspace open-profile --ack-hidden-workspace --purpose "Project QA" --profile project-dev --setup --setup-timeout-ms 30000 --setup-kill-on-timeout --startup-wait-window --startup-screenshot-window
cargo run -- workspace start --ack-hidden-workspace --foreground
cargo run -- workspace list
cargo run -- workspace cleanup --dry-run
cargo run -- workspace status
cargo run -- workspace manifest
cargo run -- workspace artifacts --existing
cargo run -- workspace ipc-info
cargo run -- workspace env
cargo run -- workspace env --shell
cargo run -- workspace launch --dry-run --name terminal --profile project-dev -- xterm
cargo run -- workspace launch --name terminal --profile project-dev -- xterm
cargo run -- workspace launch --name terminal --wait-window --screenshot-window --window-timeout-ms 10000 -- xterm
cargo run -- workspace run --dry-run --name test-suite --timeout-ms 30000 --tail-bytes 65536 --kill-on-timeout -- cargo test
cargo run -- workspace run --name test-suite --timeout-ms 30000 --tail-bytes 65536 --kill-on-timeout -- cargo test
cargo run -- workspace run --cwd "$PWD" --env AGENT_WORKSPACE=1 -- env
cargo run -- workspace launch-profile-apps --dry-run --profile project-dev --wait-window --screenshot-window --window-timeout-ms 10000
cargo run -- workspace launch-profile-apps --profile project-dev --wait-window --screenshot-window --window-timeout-ms 10000
cargo run -- workspace launch --cwd "$PWD" --env AGENT_WORKSPACE=1 -- env
cargo run -- workspace apps
cargo run -- workspace apps --running
cargo run -- workspace apps --name terminal
cargo run -- workspace apps --command xterm
cargo run -- workspace windows
cargo run -- workspace windows --all
cargo run -- workspace windows --app app-12345
cargo run -- workspace windows --class xterm
cargo run -- workspace active-window
cargo run -- workspace pointer
cargo run -- workspace observe --screenshot --output /tmp/agent-observe.png
cargo run -- workspace observe --all-windows
cargo run -- workspace wait-window --title xterm --timeout-ms 10000
cargo run -- workspace wait-window --class xterm --timeout-ms 10000
cargo run -- workspace screenshot --output /tmp/agent-workspace.png
cargo run -- workspace screenshot-window --class xterm --output /tmp/agent-window.png
cargo run -- workspace focus-window 4194316
cargo run -- workspace focus-window --class xterm --timeout-ms 10000
cargo run -- workspace close-window --dry-run 4194316
cargo run -- workspace close-window 4194316
cargo run -- workspace close-window --dry-run --title xterm --timeout-ms 10000
cargo run -- workspace close-window --title xterm --timeout-ms 10000
cargo run -- workspace move-window --class xterm 80 60
cargo run -- workspace resize-window --title xterm 800 500
cargo run -- workspace raise-window --title xterm
cargo run -- workspace minimize-window --title xterm
cargo run -- workspace show-window 4194316
cargo run -- workspace show-window --class xterm --timeout-ms 10000
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
cargo run -- workspace logs --stream stdout terminal
cargo run -- workspace wait-app --timeout-ms 30000 app-12345
cargo run -- workspace wait-app --timeout-ms 30000 --kill-on-timeout test-suite
cargo run -- workspace wait-app --timeout-ms 30000 terminal
cargo run -- workspace events --tail 20
cargo run -- workspace events --since 42
cargo run -- workspace observe --events-since 42 --events-tail 20
cargo run -- workspace setup --dry-run --profile project-dev --wait --timeout-ms 30000 --kill-on-timeout
cargo run -- workspace setup --profile project-dev --wait --timeout-ms 30000 --kill-on-timeout
cargo run -- workspace kill-app --dry-run app-12345
cargo run -- workspace kill-app app-12345
cargo run -- workspace stop --dry-run
cargo run -- workspace stop --timeout-ms 30000
cargo run -- mcp
```

On Debian/Ubuntu-like systems, the initial X11 workspace runtime is expected to
need packages along these lines:

```bash
sudo apt install xvfb openbox xdotool xauth x11-utils imagemagick xclip bubblewrap
```

`doctor` is implemented first so missing runtime dependencies are visible before
the workspace runtime grows. It also reports optional policy backend candidates
such as bubblewrap, firejail, unshare, and slirp4netns without treating them as
active enforcement. The workspace commands use a small local Unix socket daemon:

## Install

Use the installer to build the release binary, install it to
`~/.local/bin/agent-workspace-linux`, and register the MCP server in
`~/.codex/config.toml`:

```bash
./install.sh
```

The installer writes this Codex MCP entry automatically and is safe to rerun:

```toml
[mcp_servers.agent-workspace-linux]
command = "/home/YOU/.local/bin/agent-workspace-linux"
args = ["mcp"]
```

That default registration is intentionally open at the MCP layer so Codex for
Linux can own the approval UI. For MCP hosts or auto-loop agents that need fixed
permissions at server spawn, pass a ceiling file:

```toml
[mcp_servers.agent-workspace-linux]
command = "/home/YOU/.local/bin/agent-workspace-linux"
args = ["mcp", "--permissions", "/home/YOU/.config/agent-workspace-linux/permissions.json"]
```

Example ceiling file:

```json
{
  "network": {
    "mode": "local_only",
    "allow_hosts": ["localhost:3000"]
  },
  "mounts": [
    {
      "host_path": "/home/YOU/project",
      "workspace_path": "/workspace/project",
      "mode": "read_write"
    }
  ],
  "apps": {
    "allow": ["/usr/bin/google-chrome", "/usr/bin/npm"]
  }
}
```

Omitted or empty dimensions are open. Populated dimensions are hard ceilings for
that MCP process: profiles and launches may narrow access, but they cannot
broaden network mode, mount paths/access, or launch programs. Call
`mcp_permissions` after connecting to see the active ceiling. App allowlists
match the launched program only; allowing shells, package managers, or browsers
delegates whatever those programs can do inside the workspace policy.

After installation or upgrade, restart Codex or reload MCP servers so new
workspace tools, parameters, profile templates, and runtime behavior become
available. Already-running MCP server processes keep serving their old schema
until the host restarts or reloads them. Run a preview without writing files
with:

```bash
./install.sh --dry-run
```

For MCP hosts that read `.mcp.json`, the equivalent manual shape is:

```json
{
  "mcpServers": {
    "agent-workspace-linux": {
      "command": "/home/YOU/.local/bin/agent-workspace-linux",
      "args": ["mcp"]
    }
  }
}
```

After adding the server, restart or reload the MCP host and call
`workspace_doctor` first. The server speaks MCP over stdio; running
`agent-workspace-linux mcp` directly is expected to wait for an MCP client rather
than print a standalone report.

## Integration smoke

Run the local integration smoke before changing workspace runtime behavior:

```bash
scripts/integration_smoke.sh
```

The smoke uses temporary config/runtime directories, imports disposable profiles,
checks pre-daemon approval previews, starts a real local-only workspace, verifies
loopback-only and disabled-network enforcement, checks read-write/read-only
mount enforcement, checks session tracking, exercises a real X11 window with
window listing, screenshot, clipboard, keyboard input, app wait, and artifact
inspection, verifies that a workspace app can trigger workspace shutdown even if
its stop client disappears before the response, and stops the workspace before
exiting.

- `workspace start` requires `--ack-hidden-workspace` so the user explicitly
  acknowledges that a separate agent-controlled environment is being created.
  `--dry-run` returns a start preview without creating a runtime directory,
  Xauthority file, X server, window manager, or daemon socket.
  `--purpose TEXT` records a human-readable reason in status and the start event
  so an app UI can explain why the unseen workspace exists.
  If the profile asks for mounts or restricted networking, the current X11
  runtime also requires `--ack-unenforced-policy` when any requested policy is
  visible but not enforced yet. Mount profiles and disabled-network profiles do
  not need that extra acknowledgement when bubblewrap is available because
  launches run inside a bubblewrap mount namespace, `bwrap --unshare-net`, or
  the local-only loopback namespace.
  Local-only network profiles validate `allow_hosts` to localhost or loopback
  targets such as `localhost:3000` or `127.0.0.1:5173`. With bubblewrap, they
  are enforced without `--ack-unenforced-policy` by giving launched apps a
  network namespace where only sandbox loopback works. Host-loopback bridging is
  still a limitation and is reported in `applied_policy.enforcement.network`.
  If the saved profile sets `require_enforced_policy=true`, the runtime refuses
  to start or launch with unenforced policy instead of accepting that
  acknowledgement.
  Network allowlists are different: `allow_hosts` is saved and shown as profile
  intent, but host filtering is not active yet, so allowlist profiles always
  require `--ack-unenforced-policy` and report the limitation in
  `applied_policy.enforcement.network`.
  It then chooses a free X11 display, creates an `xauth` file, starts `Xvfb`,
  starts a lightweight window manager, and binds a control socket under
  `$XDG_RUNTIME_DIR/agent-workspace-linux/<id>/control.sock`. With `--profile`,
  profile width/height are applied unless explicit flags override them, and the
  profile's mounts/network/setup intent is snapshotted into status.
- `profile validate --json PATH` parses and validates a shared profile file
  without saving it, and returns the same policy, warning, and acknowledgement
  preflight shape used by `profile check`.
- `profile template project-dev` creates a starter project QA profile. `profile
  template restricted-chrome` creates a browser starter profile with
  `network.mode=disabled`, `require_enforced_policy=true`, an isolated Chrome
  user-data dir, and an explicit `--no-sandbox` startup command. That flag is
  visible in the generated profile because Chrome's SUID sandbox can abort
  before opening a window inside the bubblewrap network namespace; use this
  template with an isolated browser profile and edit the browser path or command
  before saving when needed.
- `profile put --json --dry-run` previews whether the profile would be created,
  replaced, or rejected without writing. Its response includes the requested
  profile and, when the id already exists, the existing saved profile.
  `profile put --json` creates a saved profile by id. If that id already exists,
  it fails unless `--replace` is passed explicitly.
  `profile import --json` is the same file-based flow with a clearer verb for
  file-picker import surfaces.
- `profile export ID --output PATH` writes a saved profile as pretty JSON for
  file-picker/import flows. Existing output files are not overwritten unless
  `--replace` is passed explicitly. If `PATH` is an existing directory, export
  writes `<profile-id>.json` inside it; missing parent directories are created.
- `profile delete --dry-run` returns the saved profile that would be removed
  without deleting it, so a UI can ask for confirmation with the full profile
  content visible.
- `workspace start --foreground` runs the same workspace daemon in the current
  process, which is useful for MCP hosts or dev runners that clean up detached
  child processes.
- `workspace open-profile --profile` starts a profile-backed workspace,
  optionally waits for setup first with `--setup`, and then launches declared
  startup apps only when setup succeeds, returning the workspace start, setup,
  and startup results plus a top-level `ready` flag in one response.
  `--dry-run` returns the start preview plus declared setup and startup commands
  for approval without creating a workspace or spawning apps. Setup/startup
  command entries are declarations; daemon-attached launch previews are available
  only after a workspace is running.
  Preview responses include an `approval` bundle with required
  acknowledgements, missing approval flags, MCP parameters to set, and blockers
  so the Codex app can render one confirmation surface.
  `--setup-timeout-ms` overrides the default setup wait timeout.
  `--setup-kill-on-timeout` terminates a timed-out setup command process group.
  `--startup-wait-window` waits for each startup app's first visible window.
  `--startup-screenshot-window` also captures each first startup window.
- `workspace list` scans the runtime directory and reports which known
  workspaces are currently reachable. Running and stale runtime directories can
  include a durable manifest with the hidden-workspace acknowledgement, purpose,
  profile, per-run `session_id`, display, dimensions, IPC paths captured at
  startup, event log path, detached daemon stdout/stderr log paths, and stop
  timestamp when the workspace shut down cleanly. Stopped manifests also include
  workspace runtime duration.
  The manifest preserves the final IPC event sequence and app snapshot, so
  stopped workspaces can still correlate event history and show what was running
  when they were torn down.
- `workspace manifest` reads that saved manifest directly from disk without
  contacting the workspace daemon, making it suitable for stopped workspaces or
  post-run audit views. `workspace status` remains live IPC state.
- `workspace artifacts` returns a read-only inventory of files in the runtime
  directory, including the manifest, control socket, Xauthority file, applied
  policy snapshot, event log, daemon logs, app logs, and any screenshots
  captured into the workspace runtime directory. Each artifact reports whether
  the path exists, its filesystem type, and byte size for regular files.
  `--existing` returns only paths that currently exist.
- `workspace cleanup --dry-run` previews stale workspace runtime directories in
  `candidates` without deleting files or signaling processes. Candidates
  include `process_cleanup` actions for manifest-recorded orphan app process
  groups, X server, window manager, and daemon PIDs when the process identity
  can be verified. `workspace cleanup` runs those best-effort process cleanup
  actions and removes stale runtime directories while skipping running
  workspaces.
- `workspace launch --dry-run` previews the command, cwd/env overrides, launch
  profile policy, acknowledgement requirements, mount/network isolation labels,
  and whether the app would launch without spawning a process or adding an app
  record. It requires a running workspace daemon; use `workspace start --dry-run`
  for the pre-daemon start preview.
- `workspace launch` asks the daemon to spawn an app with the workspace
  attachment environment: `DISPLAY`, `XAUTHORITY`, `AGENT_WORKSPACE_ID`,
  `AGENT_WORKSPACE_SESSION_ID`, `AGENT_WORKSPACE_RUNTIME_DIR`, and
  `AGENT_WORKSPACE_SOCKET`. Launches also scrub inherited Wayland hints and set
  common toolkit defaults toward X11 so GUI apps attach to the hidden X11
  display instead of the host desktop session. It can also set a launch cwd and
  per-app environment overrides. `--name` gives the app a stable
  workspace-local name that can be used anywhere an app id is accepted,
  including logs, waits, kills, and window `--app` filters. `--wait-window`
  waits for the launched app's first visible
  window and returns it in the same
  response. `--screenshot-window`
  captures that first launched-app window in the same response, implying
  `--wait-window`. With `--profile`, profile cwd/env are applied unless explicit
  flags override them, and profile
  mounts/network policy apply to that launched app. If a launch profile requests
  policy that remains unenforced, the launch requires `--ack-unenforced-policy`.
  Each launched app gets
  workspace-local stdout/stderr log files reported in `workspace status`.
  `workspace logs` can also read those saved log files after a workspace stops
  when its manifest remains on disk.
  Profile-backed launches also report the profile id and effective
  mount/network isolation on the app entry. App entries include the launch pid
  and process group id so forked GUI child windows can still be associated with
  the launched app. App action responses such as
  `launch`, `logs`, `wait-app`, and `kill-app` include the directly affected
  app in the top-level `apps` field. Completed apps report both a human
  `exit_status` string and structured `exit_code`/`exit_signal` fields, plus
  `stopped_at_unix` and `runtime_seconds` timing metadata.
- `workspace run` is a QA-friendly launch helper that launches an app, waits for
  completion or timeout, and returns stdout/stderr log content with structured
  completion fields in one response. It accepts the same `--name`, `--cwd`,
  `--env`, `--profile`, and `--ack-unenforced-policy` launch shaping flags as
  `workspace launch`.
  `--dry-run` returns the launch preview plus timeout, log-tail, and
  kill-on-timeout options without spawning the app. It requires a running
  workspace daemon.
  `--kill-on-timeout` terminates the launched app process group if the timeout
  elapses, while preserving stdout/stderr logs in the response.
- `workspace apps` lists launched apps from the daemon IPC state without dumping
  the full workspace status. If the daemon has stopped, it falls back to the
  saved manifest app snapshot. It can filter by `--app APP_ID_OR_PID_OR_NAME`,
  app `--name TEXT`, `--command TEXT`, `--profile PROFILE`, `--running`, or
  `--stopped`.
- `workspace windows`, `workspace active-window`, `workspace pointer`,
  `workspace observe`,
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
  workspace display. `windows` lists visible windows by default; window records
  include X11 id, title, `wm_class`, `wm_instance`, pid, workspace `app_id` when
  process metadata links the window to a launched app, geometry, and visibility.
  `--all` includes minimized/hidden windows so they can be shown again by id.
  `windows` can also filter the current list with `--title`, `--class`,
  `--pid`, or `--app`. `active-window` reports the current workspace-local focus,
  `pointer` reports the current workspace-local pointer coordinates, and
  `observe` returns status, apps, windows, active window, pointer, and
  optionally a root screenshot in one IPC call. Screenshot records include path,
  dimensions, PNG byte size, and capture timestamp. `observe --events`,
  `--events-tail`, and `--events-since` include recent or incremental event
  records in the same IPC response. `observe --all-windows` uses the same
  hidden-window listing as `windows --all`. `focus-window`, `screenshot-window`,
  `close-window`, `move-window`, `resize-window`, `raise-window`,
  `minimize-window`, `key-window`, and `type-window` can use either a raw X11
  window id or the same title/class/pid/app filters as `wait-window`; class
  filters match `wm_class` and `wm_instance`, while app filters
  match the launched process and its child processes. `move-window` and
  `resize-window` update the returned window geometry so screenshots and
  interactions can be staged predictably. `focus-window` returns the focused
  target and settled active window when focus can be resolved. `raise-window`,
  `minimize-window`, and `show-window` manage visibility and stacking without
  terminating apps; `minimize-window` and `show-window` return refreshed
  visibility state.
  `show-window` match filters also search minimized/hidden windows, so agents
  can restore an app by class, title, pid, or app id without first listing a raw
  X11 id. `close-window --dry-run` returns the targeted window record without
  closing it, and `close-window` returns the targeted window record before close
  is requested.
  `move-pointer` and
  `move-pointer-window` move the workspace pointer without clicking and return
  the resulting pointer coordinates. `click` and `click-window` can set
  button/count for right-clicks and double-clicks, and also return the resulting
  pointer coordinates.
  `drag` and `drag-window` can set the mouse button for press/move/release
  gestures and return the resulting pointer coordinates. `scroll` and
  `scroll-window` send wheel ticks in the requested direction and also return
  pointer coordinates. Successful pointer/mouse-action responses include the
  active window when focus can be resolved after the action. Window-targeted
  pointer tools resolve the same targets and use window-relative coordinates.
- `workspace key`, `workspace type`, and `workspace paste` return the
  workspace-local active window when one can be resolved after the input action.
  Their window-targeted variants also return the matched target window in
  `windows`, so agents can confirm both what they targeted and what focus looks
  like after the action.
- `workspace clipboard-set` and `workspace clipboard-get` read/write the X11
  clipboard selection inside the workspace using `xclip` or `xsel`. Clipboard
  set events record only size metadata, not the raw clipboard text.
- `workspace paste` and `workspace paste-window` set the workspace clipboard,
  then send a paste key chord, defaulting to `ctrl+v`. Paste events record only
  size metadata, not the raw pasted text.
- `workspace events` reads a workspace-local JSONL event log for IPC actions.
  `--since SEQUENCE` returns events after a previously seen sequence, and
  `--tail N` can cap the returned window. If the workspace daemon is already
  stopped but the runtime directory remains, it falls back to the saved event
  log for read-only history inspection. App launches and exits are recorded with
  structured metadata, including launch log paths, wait/kill results, and app
  lifecycle timing. Typed text is logged as metadata such as character count,
  not raw text.
- `workspace setup --profile` launches the profile's setup commands as ordinary
  workspace apps; with `--wait`, commands are supervised in sequence and the
  result reports whether they completed and exited successfully. Their status
  and logs are available through the same app status/log tools. If setup uses a
  profile with policy that remains unenforced, it requires
  `--ack-unenforced-policy`. `--dry-run` requires a running workspace daemon and
  returns one launch preview per setup command without spawning any app
  processes. `--kill-on-timeout` terminates a timed-out setup command process
  group and records the kill response in the setup result.
- `workspace launch-profile-apps --profile` launches the profile's declared
  startup apps as ordinary workspace apps, preserving profile cwd/env and policy.
  `--dry-run` requires a running workspace daemon and returns one launch preview
  per startup app without spawning any app processes.
  Profile setup commands and startup apps may include `name` fields, and those
  names become stable app targets after launch. If startup apps use a profile
  with policy that remains unenforced, they require `--ack-unenforced-policy`.
  `--wait-window` returns each startup app's first visible window when it appears
  before the timeout. `--screenshot-window` also captures each first startup
  window.
- `workspace status` reports the workspace profile id, launched apps, and app
  profile ids when a profile shaped the workspace or app. It also reports the
  start timestamp, optional purpose, hidden-workspace acknowledgement,
  unenforced-policy acknowledgement, applied policy snapshot, policy backend
  candidates discovered at start time, which parts are currently enforced,
  `state`, `backend`, `limitations`, and `required_acknowledgement` for each
  policy area, and the last event sequence for incremental event polling.
  `workspace status` and `workspace stop` talk to the same socket; use
  `workspace manifest` for saved disk state after shutdown.
  `workspace stop --dry-run` previews running apps
  without stopping the workspace. `workspace stop` waits for the daemon IPC
  socket to close before returning; `--timeout-ms` overrides the default 30000ms
  wait. Its response includes apps terminated by the workspace shutdown.
- `workspace ipc-info` reports daemon IPC protocol metadata for the workspace,
  including protocol version, Unix socket path, framing, and encoding. Each
  workspace runtime directory is created with user-only permissions before the
  daemon socket and Xauthority file are placed inside it, and the control socket
  itself is marked user-only.
- `workspace env` reports the workspace-local attachment environment in one IPC
  response, including `DISPLAY`, `XAUTHORITY`, runtime directory, and control
  socket variables plus `AGENT_WORKSPACE_SESSION_ID` for tools that need to join
  the hidden workspace explicitly.
  `--shell` prints shell-safe `export` lines for manual attachment.

The MCP server currently exposes the same control surface: `mcp_permissions`,
`workspace_doctor`, `workspace_guardrails`, `profile_path`, `profile_list`, `profile_get`,
`profile_check`, `profile_validate`, `profile_template`, `profile_put`,
`profile_import`, `profile_export`, `profile_delete`, `workspace_start`,
`workspace_open_profile`, `workspace_list`, `workspace_cleanup_stale`,
`workspace_status`, `workspace_manifest`, `workspace_artifacts`,
`workspace_ipc_info`, `workspace_env`, `workspace_launch_app`, `workspace_run_app`,
`workspace_launch_profile_apps`, `workspace_list_apps`, `workspace_list_windows`,
`workspace_active_window`, `workspace_pointer`, `workspace_observe`,
`workspace_wait_window`,
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
`workspace_kill_app`, and `workspace_stop`. `workspace_list_apps` can filter by
app id/pid/name, app name substring, command substring, profile id, or
running/stopped state, including against saved manifest app snapshots after a
workspace stops.
`workspace_guardrails` returns a machine-readable summary of acknowledgement,
dry-run, explicit override, timeout-termination, and workspace-scope rules for
approval UI flows. It also includes `policy_modes`, which describes how profile
mounts and each network mode map to `profile_check` state, backend, limitation,
and acknowledgement fields.
`profile_put` accepts `dry_run=true` to preview whether a profile would be
created, replaced, or rejected. It rejects existing profile ids by default; set
`replace=true` only when intentionally overwriting a saved environment profile.
`profile_import` performs the same save/replace/dry-run flow from a local JSON
file path, which is useful for file-picker import UI.
`profile_export` returns a saved profile and can write it to `output_path`;
existing files require `replace=true` before they are overwritten. Existing
directory outputs are treated as export folders and receive `<profile-id>.json`.
`profile_delete` accepts `dry_run=true` to return the profile that would be
removed without deleting it.
`workspace_start` accepts `dry_run=true` to preview hidden-workspace
acknowledgement, runtime readiness, profile policy acknowledgement, strict
policy blocks, and whether a new workspace would be created. Dry-run previews
include an `approval` bundle that summarizes missing acknowledgements, approval
flags, MCP parameters, and non-approval blockers.
`workspace_open_profile` accepts `dry_run=true` to preview the profile-backed
start, setup, and startup plan without creating a workspace. Its `approval`
bundle merges the start, setup, and startup approval requirements.
These two dry-run tools are pre-daemon previews.
`workspace_launch_app` accepts `dry_run=true` to preview command, cwd/env,
profile policy acknowledgement, isolation labels, blockers, and whether an app
would be launched without spawning it. This requires a running workspace daemon.
`workspace_run_app` accepts `dry_run=true` to preview the launch plus timeout,
log-tail, and kill-on-timeout options without spawning the command. This requires
a running workspace daemon.
`workspace_launch_profile_apps` accepts `dry_run=true` to return one launch
preview per startup app declared by the profile without spawning any of them.
This requires a running workspace daemon.
`workspace_run_profile_setup` accepts `dry_run=true` to return one launch preview
per setup command declared by the profile without spawning any of them. This
requires a running workspace daemon.
Launch, run, setup, and startup app dry-runs are daemon-attached previews; they
fail instead of starting a workspace when no daemon exists.
`workspace_close_window` and `workspace_close_matching_window` accept
`dry_run=true` to resolve and return the targeted window without closing it.
`workspace_cleanup_stale` accepts `dry_run=true` to preview stale runtime
directory candidates and verified orphan process cleanup without deleting files
or signaling processes.
`workspace_kill_app` accepts `dry_run=true` to resolve and return the matched
app without terminating it.
`workspace_stop` accepts `dry_run=true` to preview currently running apps
without stopping the workspace. It accepts `timeout_ms` to control how long it
waits for the daemon IPC socket to close after requesting shutdown.
`workspace_run_app` accepts `kill_on_timeout=true` to terminate the launched app
process group when its timeout elapses.
`workspace_wait_app` accepts `kill_on_timeout=true` for the same timeout cleanup
behavior on an already launched app.
`workspace_events` accepts `since_sequence` for incremental polling and `tail`
to cap the returned event list, and can read saved event history after a
workspace has stopped.
`workspace_run_profile_setup` accepts `kill_on_timeout=true`, and
`workspace_open_profile` accepts `setup_kill_on_timeout=true`, for the same
setup-command cleanup behavior.
`workspace_list_windows` and window-targeted tools can filter by title, class,
pid, app id, or app name, with class matching `wm_class` and `wm_instance`.
`workspace_list_windows` accepts `include_hidden=true` to return
minimized/hidden windows as well as visible windows. `workspace_observe` also
accepts `include_hidden=true` plus `events`, `events_tail`, and
`events_since_sequence` for single-call polling.
