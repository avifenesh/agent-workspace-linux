---
name: agent-workspace-linux
description: "Use when a task needs an isolated, hidden Linux desktop or browser the agent fully controls — GUI app QA, web/browser/shopping automation, or observing a sandboxed app — without touching the user's real mouse, keyboard, focus, or host Chrome. Triggers: 'test/QA this app', 'shop on', 'browse/automate a site', 'run a GUI app in a sandbox', 'clean desktop to test in'. Routes the agent-workspace-linux MCP tools on demand. Does NOT apply to driving the user's real/host desktop (use computer-use) or to pure code/file edits."
---

# agent-workspace-linux

Drive an isolated, agent-owned Linux desktop (hidden X11 display) and a workspace-owned browser through the `agent-workspace-linux` MCP server. The workspace runs apps and sends input only to its own display — never the user's real desktop, focus, or Chrome.

This skill is the entry point. The MCP exposes ~86 tools; **do not load them all**. Read the phase you are in below and call only those tools. Names are the logical MCP tool names; your host namespaces them (Claude Code: `mcp__agent-workspace-linux__<name>`; Codex: same logical name). If your host defers tool schemas, load a tool's schema on demand right before calling it.

## When to use

- **App / GUI QA** in a throwaway desktop: launch an app, drive it, screenshot, read logs, stop.
- **Browser / web / shopping** tasks that must not hijack the user's live Chrome or steal focus.
- **Observe / inspect** a sandboxed app's windows, output, or events.
- **Cleanup** of stale or orphaned workspaces.

Skip unless: the task needs to *run or drive a real GUI app or browser* in isolation. For driving the user's actual desktop, use `computer-use`. For pure code, file, or shell work, do not start a workspace.

## Permission model (read before mutating)

- **Default**: the MCP imposes no ceiling and defers to the host tool's boundary (Claude Code / Codex own approval). Call `mcp_permissions` once to confirm `configured=false`. After the user approves the hidden workspace once, workspace-local actions are scoped to it — do not re-prompt per action.
- **Developer ceiling**: a dev may set `AGENT_WORKSPACE_PERMISSIONS=/path/to/permissions.json` (or launch the MCP with `--permissions PATH`). Then network mode, mount paths, and an app allowlist are a hard ceiling enforced at both the MCP front-end and the workspace daemon — you cannot broaden it. Check it with `mcp_permissions` before planning launches.
- **Live control is best-effort**: the viewer's read_only / paused state (`mcp_control_state`) honors a user's runtime pause when readable, but it is a convenience layer, not the authoritative boundary. The permission ceiling is.
- **Real-world actions** (checkout, purchase, account changes, sending messages) always need explicit user approval. Keep cart-drafting separate from checkout.

Skip unless: you are about to start a workspace or launch/input — then `mcp_permissions` first.

## Safe workflow

Always orient before mutating. Prefer the read-only planners.

1. **Orient** — `mcp_agent_context` for one snapshot (active/read_only/paused, viewer, handles, recovery), then `mcp_task_plan` for a safe step sequence matched to the intent (app-QA / browser / observe / cleanup). `workspace_doctor` reports runtime + host-display readiness; `workspace_list` shows existing/stale workspaces.
2. **Start** — `workspace_start` with `ack_hidden_workspace=true` and a human `purpose`, or `workspace_open_profile` for a saved profile. A host-visible viewer opens by default unless headless.
3. **Observe** — `workspace_observe` (status + windows + pointer + optional screenshot in one call), `workspace_screenshot`, `workspace_list_windows`.
4. **Act** — `workspace_launch_app` / `workspace_run_app`; input via `workspace_click`, `workspace_type_text`, `workspace_key`, `workspace_paste_text`; manage windows with focus/move/resize/close tools.
5. **Stop** — `workspace_stop` when done. Use `workspace_cleanup_stale` to reclaim orphaned runtimes.

## Browser tasks

Use the **workspace-owned** browser over its loopback DevTools endpoint — never the user's host Chrome.

- `workspace_open_browser` launches workspace Chrome/Chromium with an isolated `--user-data-dir` and loopback CDP.
- `workspace_browser_targets` discovers pages; `workspace_browser_snapshot` reads page text/links; `workspace_browser_navigate` changes URL; `workspace_browser_search_results` extracts structured cards; `workspace_browser_click` interacts.

Skip unless: the task is browser/web automation. Do not attach to the host Chrome bridge, Playwright, or Computer Use as a shortcut.

## Tool map (load per phase, not all at once)

| Phase | Tools |
|-------|-------|
| Orient (read-only) | `mcp_agent_context`, `mcp_session_brief`, `mcp_task_plan`, `mcp_permissions`, `mcp_action_catalog`, `workspace_doctor`, `workspace_list`, `workspace_status` |
| Profiles | `profile_list`, `profile_get`, `profile_template`, `profile_put`, `profile_check` |
| Start | `workspace_start`, `workspace_open_profile` |
| Observe (read-only) | `workspace_observe`, `workspace_screenshot`, `workspace_list_windows`, `workspace_active_window`, `workspace_read_app_log`, `workspace_events` |
| Act (mutating) | `workspace_launch_app`, `workspace_run_app`, `workspace_click`, `workspace_type_text`, `workspace_key`, `workspace_paste_text`, `workspace_focus_window`, `workspace_close_window` |
| Browser | `workspace_open_browser`, `workspace_browser_targets`, `workspace_browser_snapshot`, `workspace_browser_navigate`, `workspace_browser_search_results`, `workspace_browser_click` |
| Control / viewer (best-effort) | `mcp_control_state`, `mcp_control_update`, `workspace_open_viewer`, `workspace_list_viewers`, `workspace_close_viewer` |
| Teardown | `workspace_stop`, `workspace_kill_app`, `workspace_cleanup_stale` |

## Do NOT

- Do not load or list all workspace tools up front — pull only the current phase's tools.
- Do not send input to or screenshot the user's real desktop; this MCP is the *isolated* workspace (use `computer-use` for the host).
- Do not start a workspace without `ack_hidden_workspace` and a `purpose`.
- Do not perform checkout / purchase / account / send actions without explicit user approval.
- Do not attach to the user's host Chrome, Playwright, or Computer Use for browser work — use the workspace browser tools.
- Do not leave workspaces running; stop and, if needed, `workspace_cleanup_stale`.

## Example trajectory

Task: "QA the settings dialog of myapp."
1. `mcp_agent_context` → not running, host display present, no ceiling.
2. `mcp_task_plan` (app-QA) → reviewed plan: start → observe → input → evidence → stop.
3. `workspace_start { ack_hidden_workspace: true, purpose: "QA myapp settings" }`.
4. `workspace_launch_app { command: ["myapp"] }` → `workspace_observe` to find the window.
5. `workspace_click` / `workspace_type_text` to open and exercise Settings; `workspace_screenshot` for evidence.
6. `workspace_read_app_log` for errors; `workspace_stop`.

## Constraints

- Always-loaded cost is the frontmatter only (~70 tokens). The body (~1.5–2k tokens) loads on activation; individual tool schemas load on demand.
- This skill intentionally omits a restrictive `allowed-tools` list so it can route to the full `agent-workspace-linux` tool family on demand. The host's deferred tool-loading keeps schemas out of context until used.
- Validate with `agnix --target all` (zero errors/warnings expected).
