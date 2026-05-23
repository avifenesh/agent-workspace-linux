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
cargo run -- workspace status
cargo run -- workspace launch -- xterm
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
- `workspace launch` asks the daemon to spawn an app with the workspace
  `DISPLAY` and `XAUTHORITY`.
- `workspace status` and `workspace stop` talk to the same socket.

The MCP server currently exposes the lifecycle surface: `workspace_doctor`,
`workspace_start`, `workspace_status`, `workspace_launch_app`, and
`workspace_stop`. Screenshot/input/window-listing tools are intentionally left
for the next slice after the runtime can start locally.
