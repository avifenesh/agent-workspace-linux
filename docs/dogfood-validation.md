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

Post-patch verification:

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
