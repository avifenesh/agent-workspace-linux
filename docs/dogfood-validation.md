# Dogfood Validation

This file records real MCP dogfood results that gate the later permission
hardening work. It is intentionally evidence-oriented: verified behavior goes
here, while policy design stays in `permission-boundary-roadmap.md`.

## 2026-05-24 MCP Pass

Environment:

- Auto-loop A/B/C gate pass ran from
  `/home/avifenesh/projects/agent-workspace-linux` with the current local tree.
- `scripts/integration_smoke.sh` passed end to end. This revalidated MCP
  permission ceilings, profile import/export/delete/validate, `workspace
  open-profile --dry-run`, real setup/startup, local-only networking,
  disabled networking, read-only/read-write mounts, X11 window launch,
  screenshots, window targeting, keyboard input, clipboard, app wait/logs,
  artifacts, event history, browser local-dev QA through Chrome/Chromium when
  installed, disabled-network browser launch coverage when Chrome/Chromium is
  installed, crashed-daemon stale cleanup, and self-stop from inside a workspace
  app.
- The same smoke was rerun against the installed user-facing binary with
  `BIN=/home/avifenesh/.local/bin/agent-workspace-linux scripts/integration_smoke.sh`.
  It passed, confirming the installed CLI path used by the Codex app/MCP has
  the same current behavior as the repo build for permission ceilings, network
  isolation, mount enforcement, browser QA, screenshots/input/clipboard, events,
  cleanup, daemon-crash recovery, and self-stop.
- `cargo test` passed 40 tests, including permission-ceiling checks,
  local-only/disabled network policy planning, launch-preview daemon
  requirements, workspace socket-path validation, stop behavior, browser-session
  template behavior, and profile validation.
- The Codex for Linux side-by-side dev app was rebuilt from
  `/home/avifenesh/projects/codex-desktop-linux` with `make build-dev-app` and
  launched inside the hidden `mcp-visible` workspace. A launcher bug was found
  and fixed: inherited `ELECTRON_RENDERER_URL` from the host Codex app could
  make the dev app render the wrong webview. The launcher now uses the managed
  local webview URL unless `CODEX_LINUX_ALLOW_RENDERER_URL_OVERRIDE=1` is set.
  Chrome DevTools Protocol confirmed the hidden app loaded
  `http://127.0.0.1:5176/?mcpAppSandboxDevtools=1`.
- The Codex app embedded workspace preview was dogfooded against the real
  hidden app. It appears in the conversation view with live screenshot,
  workspace metadata, Refresh, Stop, and Revoke. It now hides on Settings pages,
  where the dedicated Agent Workspaces page already shows the active workspace
  and controls. A follow-up side-by-side dev-app pass confirmed the panel hides
  within the Settings transition and reappears after Back to app, using both
  Chrome DevTools Protocol DOM checks and hidden-workspace screenshots.
- The Agent Workspaces settings page in the dev app showed the MCP permissions
  card, one active-workspace card, saved-workspaces section, and a working
  Status/Hide status toggle. The corresponding feature tests now pass 12 tests.
- A C-gate browser-session probe added a starter profile for explicitly
  user-approved browser data directories. The first probe mounted a temporary
  user-data dir read-write and launched Chrome without `--no-sandbox`; Chrome
  exited before opening a window. The generated `browser-session` template keeps
  that caveat visible by adding `--no-sandbox`, mounting the selected data dir at
  `/workspace/browser-user-data`, requiring mount enforcement, and inheriting
  host networking for authenticated web tasks. A live template probe then opened
  `about:blank - Google Chrome` with
  `mount_isolation=bubblewrap_mount_namespace` and `network_isolation=host`.
- The installed Codex MCP path then revalidated network enforcement against the
  already-running `mcp-visible` workspace. Two saved profiles were created
  through `profile_put` with explicit no-overwrite dry runs:
  `dogfood-network-disabled` and `dogfood-network-local-only`. `profile_check`
  reported `state=enforced` with `backend=bubblewrap_unshare_net` for disabled
  networking and `backend=bubblewrap_loopback_only` for local-only networking.
  `workspace_run_app` with the disabled profile blocked `1.1.1.1:80` with
  `Network is unreachable` and DNS with temporary name-resolution failure.
  `workspace_run_app` with the local-only profile successfully round-tripped
  through an in-sandbox `127.0.0.1` listener and blocked `1.1.1.1:80` with
  `Network is unreachable`.

Remaining gaps from this pass:

- `local_only` remains sandbox-local loopback. Host-localhost bridging is still
  a product/runtime gap.
- Network allowlists remain declared intent until an egress-filter backend is
  implemented and tested.
- Browser tasks that need logged-in sessions now have a starter
  `browser-session` profile for explicitly user-approved browser data dirs, but
  the product UI still needs a friendly picker/copy/lock-warning flow before it
  is comfortable for real account profiles.
- Hard permission enforcement in Codex for Linux should still wait until the UI
  approval boundary is wired so agents cannot call the same workspace tools
  outside the user-approved path.
- MCP app-action responses still include large full-status payloads with long
  stopped-app history. They are correct but too noisy for agent context and for
  any UI that wants concise action feedback.

Previous environment:

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
- Profile JSON preflight now has an explicit no-write path. `profile validate
  --json PATH` returns the parsed profile plus the same policy, warning, and
  acknowledgement preflight as `profile check`, and rejects invalid shared
  profiles before saving.

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
- Chrome under the bubblewrap disabled-network namespace needed `--no-sandbox`
  on this machine. Without it, Chrome aborted before opening a window because
  its SUID sandbox helper was not usable in the launch context. Browser launch
  templates should make this tradeoff visible instead of hiding it in JSON.
- Fixed while codifying the browser smoke: host Wayland environment variables
  could make Chrome prefer the host Wayland session over the hidden X11 display.
  Workspace launches now scrub inherited Wayland hints and set common toolkit
  defaults toward X11.

Previous post-patch verification:

- Local `cargo test` passes 27 tests, including coverage for active profile
  cwd/env inheritance and explicit per-launch profile override.
- The installed CLI was rebuilt with `./install.sh`. A regression dogfood
  launched a workspace app that invoked `workspace stop` from inside its own
  workspace process group. Even though that stop client was terminated before it
  could receive the response, the daemon still marked the workspace stopped and
  wrote `ready=false` plus `stopped_at_unix` to the manifest.
- The installed CLI status shape was checked after reinstall and reports
  `daemon_pid`, `x_server_pid`, and `window_manager_pid` for process-aware stale
  cleanup.
- `scripts/integration_smoke.sh` now covers profile validation, invalid-profile
  rejection, profile delete dry-run plus actual delete, and that self-stop
  lost-client case in addition to doctor, profile import/export,
  open-profile dry-run, real `workspace open-profile` execution with setup
  success, setup artifact creation, startup window wait, startup screenshot,
  startup app targeting, workspace stop, and stopped manifest log reads,
  `network.mode=local_only`, `network.mode=disabled`, read-write/read-only mount
  enforcement, session tracking, a real X11 window with screenshot, window
  listing, clipboard, keyboard input, app wait, artifact inspection, browser
  local-dev QA through Chrome when Chrome/Chromium is installed, optional
  disabled-network Chrome launch/screenshot coverage when Chrome/Chromium is
  installed, event history,
  stopped manifests, and daemon-crash recovery where stale cleanup removes
  manifest-recorded orphan app and X11 runtime processes.
- MCP dogfood covered the local-dev browser QA path: a hidden workspace launched
  `python3 -m http.server` from this repo, a workspace command fetched
  `README.md` over `127.0.0.1`, Chrome opened the served page with
  `wait_window` plus `screenshot_window`, targeted `ctrl+l`, paste, and
  `Return` navigated to `docs/dogfood-validation.md`, `workspace_observe`
  captured a screenshot/events snapshot, and `workspace_stop` terminated both
  the dev server and Chrome before stale cleanup removed the runtime directory.
- MCP dogfood covered a real Codex desktop QA run through the MCP surface using
  a temporary profile for `/home/avifenesh/projects/codex-desktop-linux`
  mounted read-write at `/workspace/project`, `network.mode=disabled`, and
  `require_enforced_policy=true`. `profile_check` and `workspace_open_profile
  --dry-run` reported bubblewrap enforcement for mounts and disabled networking
  with only the hidden-workspace acknowledgement required. Inside the hidden
  workspace, a Python socket probe to `1.1.1.1:80` failed with `Network is
  unreachable`, while `node --test linux-features/agent-workspace/test.js`
  passed 9 tests. After `workspace_stop`, saved-manifest log reads and events
  still reported the passed test output and app history; stale cleanup removed
  the runtime and the temporary profile was deleted.
- MCP dogfood covered browser behavior under `network.mode=disabled`: a
  temporary `dogfood-chrome-netoff` profile opened Chrome in the hidden X11
  workspace with `network_isolation=bubblewrap_unshare_net`, captured a window
  screenshot, and Chrome rendered its own `ERR_INTERNET_DISCONNECTED` page for
  `example.com`. `workspace_observe`, `workspace_screenshot_window`, event
  history, `workspace_stop`, stale cleanup, and profile deletion all worked
  after the browser run.
- The Chrome sandbox caveat is now captured as a first-class
  `restricted-chrome` profile template. The generated JSON visibly includes
  `network.mode=disabled`, `require_enforced_policy=true`, an isolated
  user-data dir, and `--no-sandbox`; the integration smoke validates that
  generated profile before the browser launch coverage.
- MCP dogfood revalidated disabled-network Chrome through the installed MCP
  tools after the template work. Because the current MCP server process was
  still stale, `profile_template(kind="restricted-chrome")` returned
  `unknown profile template "restricted-chrome". Expected: project-dev`; the
  pass created the equivalent temporary profile with `profile_put` instead.
  `workspace_start --dry-run` and real start both reported
  `network.enforcement.state="enforced"` with `backend="bubblewrap_unshare_net"`
  and only the hidden-workspace acknowledgement required. Host networking could
  fetch `http://1.1.1.1`, while a workspace `curl http://1.1.1.1` probe failed
  with status 7 and no routes. Chrome launched with `--no-sandbox` inside that
  disabled-network workspace, first rendered `ERR_INTERNET_DISCONNECTED`, then
  targeted `ctrl+l`, `paste-window`, and `Return` navigated to a `data:` page
  proving scoped input. The event log recorded paste byte/character counts, not
  raw pasted text. `workspace_manifest`, `workspace_artifacts`,
  `workspace_ipc_info`, `workspace_list_apps`, `workspace_stop`,
  `workspace_cleanup_stale`, and profile deletion all worked afterward.
- Fixed after this pass: the installer now warns when an
  `agent-workspace-linux mcp` process is already running and clarifies that
  Codex/MCP reload is required for new tool schemas, parameters, templates, and
  runtime behavior. README install docs now make the same upgrade caveat
  explicit.
- MCP dogfood covered the Codex for Linux conversation-visibility slice with the
  real side-by-side dev app. A hidden workspace launched
  `/home/avifenesh/projects/codex-desktop-linux/bin/codex-cua-lab` and rendered a
  `codex-cua-lab` window. `workspace_observe --screenshot` captured the Codex app
  with the embedded Agent Workspace panel visible inside the conversation view,
  including a live recursive workspace screenshot, profile/app metadata, and
  Stop/Revoke controls. A workspace-local click on the panel's Stop button
  triggered `workspace_stop` from inside the nested Codex app; the saved event
  log shows the click, the app exit, and `workspace_stop`, then stale cleanup
  removed the runtime. This validates the B-gate first slice against the actual
  app patch, not only synthetic tests.
- A second live side-by-side dev-app pass verified the settings route. Inside the
  hidden workspace, the app opened Settings, the sidebar included Agent
  Workspaces, the page displayed the active workspace card, and the Chrome
  template button opened the create form with `restricted-chrome`,
  `network=disabled`, `/tmp`, and the `restricted-chrome-no-sandbox` startup app.
  The profile was not saved; profile list remained empty after the pass. The
  embedded panel Stop button again stopped the workspace and stale cleanup
  removed the runtime.
- MCP-locked permission ceilings now have a first implementation and smoke
  pass. Unit coverage verifies that a disabled-network ceiling rejects
  unprofiled host-network launches, allows a disabled-network profile launch,
  enforces local-only/allowlist host subsets, caps mounts to same-or-child paths
  without read-only to read-write upgrades, and rejects launch commands outside
  the app allowlist. CLI smoke verified `mcp --help`, missing permission-file
  errors, invalid app allowlist errors, and that a valid permissions file loads
  and starts the stdio MCP server when stdin stays open. The added stdio MCP
  smoke initializes the server, calls `mcp_permissions`, verifies the structured
  ceiling response, rejects an inherited-network profile under a disabled
  ceiling, rejects a startup app outside the allowlist, and accepts a narrowed
  disabled-network profile dry-run.
- Full `scripts/integration_smoke.sh` still passes after the MCP ceiling patch,
  covering profile import/export, open-profile dry-run/setup/startup, local-only
  and disabled-network enforcement, read-only/read-write mounts, screenshots,
  input/clipboard, artifacts, browser local-dev QA, crashed-daemon cleanup, and
  workspace self-stop.
- CLI bridge parity smoke now covers `agent-workspace-linux --permissions PATH`:
  a profile using `network.mode=inherit_host` is rejected under a disabled
  ceiling, while a disabled-network profile with an allowlisted `sh` startup app
  is accepted in dry-run mode. This gives the Codex for Linux CLI bridge a way
  to honor a locked MCP permission file instead of bypassing it.
- Dogfooding the Codex dev app in a hidden workspace exposed a startup failure
  for long workspace ids: the derived `control.sock` path could exceed the Unix
  socket `sun_path` limit, causing the daemon to fail after Xvfb/openbox were
  already spawned. The runtime now validates the socket path in start preview,
  start preparation, and daemon startup before spawning workspace processes; unit
  coverage verifies the boundary.
- After the socket guard, the MCP workspace surface launched
  `/home/avifenesh/projects/codex-desktop-linux/bin/codex-cua-lab` in a short-id
  hidden workspace, found the `codex-cua-lab` window, captured a window
  screenshot plus root screenshot/events, stopped the app/workspace cleanly, and
  removed the stopped runtime through `workspace_cleanup_stale` dry-run plus
  actual cleanup.
- The installed CLI was rebuilt with the socket guard. A too-long workspace id
  now exits with the explicit socket-path error before start, and the process
  count check showed no new workspace X11/window-manager process was spawned.
- A second real-project QA pass used the freshly installed CLI against
  `/home/avifenesh/projects/agent-chrome-bridge`. The temporary profile mounted
  the project read-only at `/workspace/project`, set that as the profile cwd,
  requested `network.mode=disabled`, and required enforcement. The workspace
  reported `bubblewrap_mount_namespace` plus `bubblewrap_unshare_net`, and
  `npm test` passed from the mounted cwd with 32 MCP tools exposed by the
  project smoke. Stop, stale cleanup, and profile deletion completed afterward.
- The same pass also re-confirmed why Codex/MCP reload matters after install:
  the already-running MCP server process still used older profile-cwd behavior
  until the installed CLI was invoked directly. This is an upgrade/reload
  lifecycle issue, not a current-runtime failure.
- The installer stale-MCP warning was tightened after dogfood showed it could
  include its own process-scanning helper in the match list. `install.sh
  --skip-build --no-doctor` now reports only the actual
  `agent-workspace-linux mcp` process that must be restarted.
- Codex-spawned MCP dogfood found a second launcher lifecycle issue: if the MCP
  process starts without `XDG_RUNTIME_DIR`, it used to look under
  `/tmp/agent-workspace-linux-$USER` while desktop/shell-launched workspaces
  lived under `/run/user/<uid>`. The runtime now discovers `/run/user/<uid>` as
  the fallback before `/tmp`, so MCP tools, the app bridge, and shell commands
  converge on the same active workspace directory.
- After restarting Codex so the MCP server respawned from the installed binary,
  the real MCP `workspace_list` and `workspace_status` tools saw
  `/run/user/1000/agent-workspace-linux/mcp-visible` and connected to the live
  workspace. The same MCP pass generated a `browser-session` profile by calling
  `profile_template` with `kind="browser-session"` and `user_data_dir`, proving
  the installed MCP path exposes the new browser-session behavior after reload.
