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

## Commands

```bash
cargo run -- doctor
cargo run -- workspace start
cargo run -- workspace start --foreground
cargo run -- workspace list
cargo run -- workspace status
cargo run -- workspace launch -- xterm
cargo run -- workspace launch --cwd "$PWD" --env AGENT_WORKSPACE=1 -- env
cargo run -- workspace windows
cargo run -- workspace screenshot --output /tmp/agent-workspace.png
cargo run -- workspace focus-window 4194316
cargo run -- workspace close-window 4194316
cargo run -- workspace click 100 120
cargo run -- workspace key Return
cargo run -- workspace type "hello from the agent workspace"
cargo run -- workspace logs --stream stdout app-12345
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
the workspace runtime grows. The workspace commands use a small local Unix
socket daemon:

- `workspace start` chooses a free X11 display, creates an `xauth` file, starts
  `Xvfb`, starts a lightweight window manager, and binds a control socket under
  `$XDG_RUNTIME_DIR/agent-workspace-linux/<id>/control.sock`.
- `workspace start --foreground` runs the same workspace daemon in the current
  process, which is useful for MCP hosts or dev runners that clean up detached
  child processes.
- `workspace list` scans the runtime directory and reports which known
  workspaces are currently reachable.
- `workspace launch` asks the daemon to spawn an app with the workspace
  `DISPLAY` and `XAUTHORITY`. It can also set a launch cwd and per-app
  environment overrides. Each launched app gets workspace-local stdout/stderr
  log files reported in `workspace status`.
- `workspace windows`, `workspace screenshot`, `workspace focus-window`,
  `workspace close-window`, `workspace click`, `workspace key`, `workspace type`,
  `workspace logs`, and `workspace kill-app` inspect or act through the same
  daemon, scoped to the workspace display.
- `workspace status` and `workspace stop` talk to the same socket.

The MCP server currently exposes the same control surface: `workspace_doctor`,
`workspace_start`, `workspace_list`, `workspace_status`, `workspace_launch_app`,
`workspace_list_windows`, `workspace_screenshot`, `workspace_focus_window`,
`workspace_close_window`, `workspace_click`, `workspace_key`,
`workspace_type_text`, `workspace_read_app_log`, `workspace_kill_app`, and
`workspace_stop`.
