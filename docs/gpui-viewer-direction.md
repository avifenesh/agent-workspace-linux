# GPUI Viewer Direction

Last updated: 2026-05-26

`agent-workspace-linux` owns the primary visible Agent Workspace monitor. Codex
Desktop and other MCP hosts should be thin launchers/settings surfaces, while
the runtime provides the reusable viewer, lifecycle registry, and workspace
control contract.

## Product Shape

- `agent-workspace-linux mcp` runs the stdio MCP server.
- `agent-workspace-linux viewer` opens the native GPUI monitor directly.
- `workspace_start` and `workspace_open_profile` open a target-bound viewer by
  default when the server is not headless and
  `workspace_doctor.ready_for_host_viewer=true`.
- `workspace_open_viewer` reopens or reuses a target-bound viewer explicitly.
- The viewer runs as a child/subcommand process, not inside the MCP stdio event
  loop.

The default viewer is a compact, square-ish, screen-first monitor for passive
work observation. It is draggable from the header area, resizable from the
bottom-right grip, and persists size, position, screen-stream preference, and
footer mode in the user's XDG config directory.

## Viewer Contract

- The default viewer opens unless the MCP is started with `--headless`, the host
  display is unavailable, or the tool call sets `open_viewer=false`.
- The default viewer does not request always-on-top state.
- `--always-on-top` and `workspace_open_viewer(always_on_top=true)` are opt-in
  overlay modes for hosts or users that explicitly ask for that behavior.
- `workspace_open_viewer` reuses an existing registered viewer for the target
  workspace instead of opening duplicate GPUI windows.
- MCP-opened viewers pass `--exit-when-workspace-gone`, so monitors opened for a
  task disappear when their selected workspace runtime is removed.
- Direct `agent-workspace-linux viewer` launches remain persistent, so they can
  act as a reusable local monitor.
- `viewer list` / `workspace_list_viewers` and `viewer close` /
  `workspace_close_viewer` are the repo-owned recovery surface for orphan or
  compositor-invisible viewer windows.

The viewer refreshes workspace status, app state, event state, and live-control
state without constantly capturing screen pixels. Screen streaming is off by
default; enabling `View` overwrites one reusable frame file instead of creating a
new timestamped screenshot per refresh. The footer favors user-useful context:
in-flight viewer actions, latest workspace events, browser reads/navigations,
active app/window, inferred task intent, and permission/isolation state.

## Agent Boundary

Agents should treat the viewer as host-visible/open-world UI. It is available
when the user or host wants to watch, pause, resume, or inspect the workspace.
It should not be used as release evidence unless the evidence was collected
through the repo-owned runtime/MCP paths, not through Codex Desktop, Computer
Use, Playwright, or a host browser bridge.

Browser work should use Chrome/Chromium launched inside the workspace and the
workspace-owned MCP browser tools:

- `workspace_browser_targets`
- `workspace_browser_snapshot`
- `workspace_browser_search_results`
- `workspace_browser_navigate`

This keeps browser automation inside the workspace profile, permission, event
log, and audit boundary.

## Release Evidence

Run source-bound release checks after source or documentation edits:

```bash
scripts/release_gate_audit.py
scripts/final_review_bundle.py
scripts/objective_completion_audit.py
scripts/release_next_steps.py
```

Use `AGENT_WORKSPACE_NO_NEW_VIEWER=1 scripts/prod_readiness_smoke.sh` when a
human already has a real viewer open and iterative validation should avoid
opening another monitor. Run the default smoke when strict visual viewer
evidence is needed.

Release-only viewer evidence is intentionally tracked by
`scripts/release_gate_audit.py`, and the current public-readiness checkpoint has
source-matched local viewer evidence. Do not copy timestamped evidence paths or
source hashes from docs into release decisions; regenerate the audit and final
review bundle for the current source.
