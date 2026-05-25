# Dogfood Validation

This file records real MCP dogfood results that gate the later permission
hardening work. It is intentionally evidence-oriented: verified behavior goes
here, while policy design stays in `permission-boundary-roadmap.md`.

## 2026-05-25 Integration Smoke Native GUI Regression Pass

Environment:

- Ran `scripts/integration_smoke.sh` from the runtime repo with the local debug
  build after extending the suite.
- Chrome/Chromium and `gnome-text-editor` were present, so both optional GUI
  paths executed instead of skipping.

Verified:

- The smoke suite now includes a mounted GUI editor regression. It launches the
  editor against a read-write mounted file, waits for a real painted window
  screenshot instead of only a mapped X11 window, clicks into the document,
  selects existing content, pastes replacement text, saves, and verifies the
  host file contains `edited-from-integration-smoke` and
  `mounted-editor-save-ok`.
- The smoke suite now includes a native Chrome input regression. It launches
  Chrome in a hidden workspace, navigates to a generated `data:text/html` page,
  types into a focused input, and waits for the X11 title to change to
  `typed:typed-ok`, proving page-level text input rather than only address-bar
  navigation.
- The smoke suite now starts the synthetic `browser-session` profile end to
  end. It imports the generated profile, opens it with startup Chrome, verifies
  the mounted browser data directory is writable from inside the workspace, then
  stops the workspace and deletes the saved profile.
- Full `scripts/integration_smoke.sh` passed after the change, including
  permission ceilings, profile import/export, open-profile dry-runs, setup and
  startup apps, browser-session startup, disabled and local-only network
  enforcement, mount enforcement, screenshots, input, clipboard, artifacts,
  Chrome local-dev QA, crashed-daemon cleanup, and self-stop.

Findings:

- GTK apps can expose a mapped X11 window before their surface is actually
  painted. The regression now waits for a non-trivial screenshot before sending
  editor input, matching the manual dogfood observation that a later screenshot
  rendered correctly.

## 2026-05-25 Arbitrary App Mounted-File Pass

Environment:

- Dogfood ran through the installed Codex MCP tools in developer-open mode.
- Preflight state was clean: `workspace_list`, `profile_list`, and
  `workspace_cleanup_stale --dry-run` returned no entries. `workspace_doctor`
  reported X11 workspace readiness and bubblewrap mount enforcement support.

Verified:

- A temporary host directory
  `/tmp/agent-workspace-arbitrary-app-mount` was mounted read-write at
  `/workspace/mounted` through a saved profile
  `dogfood-arbitrary-app-mount-20260525` with
  `require_enforced_policy=true`.
- `profile_put --dry-run` previewed creation without overwrite, then the real
  `profile_put` saved the profile. `workspace_open_profile --dry-run` returned
  the hidden-workspace approval bundle and reported mount enforcement through
  `bubblewrap_mount_namespace`. The real `workspace_open_profile` started the
  mounted workspace on `:90` with profile cwd `/workspace/mounted`.
- A command launched inside the mounted workspace wrote
  `/workspace/mounted/probe.txt` with `mount_isolation=bubblewrap_mount_namespace`.
  The host then read
  `/tmp/agent-workspace-arbitrary-app-mount/probe.txt` and saw
  `workspace-write-ok`, proving read-write mount propagation.
- The workspace found `gnome-text-editor` as an installed non-browser desktop
  app. It seeded `/workspace/mounted/editor-note.txt`, then launched
  `gnome-text-editor /workspace/mounted/editor-note.txt`. The app produced a
  visible `editor-note.txt (/workspace/mounted) - Text Editor` window.
- Workspace-local focus, `ctrl+a`, `workspace_paste_text`, and `ctrl+s` edited
  and saved the mounted file in GNOME Text Editor. A window screenshot showed
  `edited-from-agent-workspace` and `mounted-editor-save-ok`; the host read the
  same content from
  `/tmp/agent-workspace-arbitrary-app-mount/editor-note.txt`.
- `workspace_observe` and `workspace_events` recorded app/window state,
  screenshots, and input events. `workspace_stop --dry-run` showed the live
  `gnome-text-editor-dogfood` app. Real `workspace_stop` terminated it, stale
  cleanup removed the runtime directory, the temporary profile was deleted, the
  temporary host mount directory was removed, and final `workspace_list`,
  `profile_list`, and stale cleanup dry-run were empty.
- Follow-up timing against a separate minimal mounted profile
  `timing-mounted-profile-20260525` did not reproduce the several-minute
  `workspace_open_profile` latency from this pass. Installed CLI
  `workspace open-profile --dry-run` and `profile check` both returned with
  `elapsed=0.00`; installed MCP `workspace_open_profile` dry-run returned in
  about 3.4s, and real MCP `workspace_open_profile` returned in about 2.5s.

Findings:

- Arbitrary non-browser app control is viable through the installed MCP surface:
  a mounted host path can be opened in a normal desktop editor, edited through
  workspace-local input, saved, and verified on the host without touching the
  user's visible desktop.
- One mounted-profile `workspace_open_profile` call took several minutes during
  this pass, but immediate follow-up timing did not reproduce it. Keep an eye on
  profile-open latency, but do not treat this as confirmed current behavior.
- GNOME Text Editor initially captured as a black first-window screenshot, then
  rendered correctly on a later capture. Its stderr included DRI3 acceleration
  warnings and a missing `dbus-launch` warning. This did not block the edit/save
  workflow, but it is useful evidence for arbitrary GTK app polish.

## 2026-05-25 Native Chrome Control Pass

Environment:

- Dogfood ran through the installed Codex MCP tools in developer-open mode.
  `mcp_permissions` reported no spawn-time ceiling.
- Preflight state was clean: `workspace_list`, `profile_list`, and
  `workspace_cleanup_stale --dry-run` returned no entries.

Verified:

- `workspace_start --dry-run` returned the hidden-workspace approval bundle
  without creating runtime state. The real `workspace_start` then created a
  1280x800 workspace on `:90` for `Dogfood native browser control`.
- A workspace command found installed everyday GUI candidates:
  `google-chrome`, `firefox`, `gnome-text-editor`, and `xterm`.
- `workspace_launch_app` opened Google Chrome with a temporary
  `/tmp/agent-workspace-native-browser-control` user-data dir, `--no-sandbox`,
  `--no-first-run`, and `about:blank`. It waited for a visible
  `about:blank - Google Chrome` window and captured a first-window screenshot.
- `workspace_focus_window`, `workspace_key ctrl+l`, `workspace_paste_text`, and
  `workspace_key Return` drove Chrome's address bar to a local `data:text/html`
  page. The active window title changed to
  `Agent Workspace Native Browser OK - Google Chrome`, proving paste and
  navigation worked in a normal browser workflow.
- `workspace_type_text` typed into the page's autofocus input. The captured
  window screenshot showed the rendered page with `native-browser-ok`,
  `workspace-browser-control`, and the typed workspace-local input text.
- `workspace_observe` returned the visible Chrome window, hidden Chrome helper
  windows, app records, pointer metadata, a root screenshot, and recent events.
  `workspace_events` recorded key input, typed text as `char_count`, observe,
  and screenshot events without raw typed-text leakage.
- `workspace_stop --dry-run` showed the running Chrome app that would be
  stopped. The real stop ended Chrome with exit code 0, wrote stopped manifest
  state, and `workspace_cleanup_stale` removed the runtime directory. The
  temporary Chrome user-data dir was deleted. Final `workspace_list`,
  `profile_list`, and stale cleanup dry-run were empty.

Findings:

- The installed MCP surface now has a clean proof for a user-like browser flow:
  start, launch Chrome, focus, paste into the address bar, navigate, type into
  page content, observe/screenshot, inspect events, stop, and cleanup. This is a
  better C-gate proxy for shopping/browser QA than terminal-only control.

## 2026-05-25 Start/Stop and Native Control Pass

Environment:

- Dogfood ran through the installed Codex MCP tools in developer-open mode.
  `mcp_permissions` reported no spawn-time ceiling.
- `workspace_doctor` reported the X11 workspace dependencies and bubblewrap
  policy backend ready. `workspace_list`, `profile_list`, and stale cleanup
  dry-run were empty before the pass.

Verified:

- A temporary `dogfood-disabled-network-20260525` profile with
  `network.mode=disabled` and `require_enforced_policy=true` was created through
  `profile_put` after a no-overwrite dry-run. `workspace_open_profile --dry-run`
  returned the hidden-workspace approval bundle and an enforced
  `bubblewrap_unshare_net` network policy. The real start created workspace
  `default` on `:90`.
- Inside the disabled-network workspace, `workspace_run_app` launched
  `disabled-network-probe` with `network_isolation=bubblewrap_unshare_net`. A
  Python socket probe to `93.184.216.34:80` failed with
  `[Errno 101] Network is unreachable` and exited 0.
- A temporary `dogfood-local-only-20260525` profile with
  `network.mode=local_only` and `require_enforced_policy=true` was created the
  same way. Dry-run and real start reported
  `network.enforcement.backend=bubblewrap_loopback_only`, plus the documented
  limitation that host loopback services are not bridged.
- Inside the local-only workspace, `workspace_run_app` launched
  `local-only-probe` with `network_isolation=bubblewrap_loopback_only`. The
  probe successfully round-tripped through an in-sandbox `127.0.0.1` listener
  (`loopback_connect=ok`) and blocked `93.184.216.34:80` with
  `[Errno 101] Network is unreachable`.
- Normal GUI affordances worked through the MCP surface. The workspace found
  installed candidates including `xterm`, `xmessage`, `gnome-text-editor`,
  Firefox, and Google Chrome. `workspace_launch_app` opened `xterm` as
  `native-xterm-dogfood`, waited for a visible window, and captured a window
  screenshot. `workspace_focus_window`, `workspace_type_text`, and
  `workspace_key Return` executed `echo native-input-ok; pwd; echo
  DISPLAY=$DISPLAY; echo WORKSPACE=$AGENT_WORKSPACE_ID` inside the terminal.
  A root screenshot then showed `native-input-ok`, `/tmp`, `DISPLAY=:90`, and
  `WORKSPACE=default`.
- Stop behavior was explicit and inspectable. `workspace_stop --dry-run`
  returned the live `native-xterm-dogfood` app that would be terminated. The
  real stop sent SIGTERM to the xterm app, wrote stopped status to the manifest,
  and `workspace_cleanup_stale` removed the stopped runtime directory. The two
  temporary profiles were deleted, and final `workspace_list`, `profile_list`,
  and cleanup dry-run were empty.

Findings:

- Start, stop, app launch, window discovery, screenshot, focused keyboard input,
  status, events, and cleanup are usable through the installed MCP tools without
  touching the host desktop.
- `workspace_type_text` worked but was slow for a long shell command in xterm.
  `workspace_paste_text` reported success with `shift+Insert`, but this pass did
  not produce a convincing terminal output proof for paste. Count xterm typing
  as validated, and keep richer paste behavior as app/window-specific dogfood
  rather than a generic proof.

## 2026-05-25 MCP Browser-Session Restart Pass

Environment:

- Dogfood ran through the installed Codex MCP tools after restarting the Codex
  app and MCP server.
- `mcp_permissions` reported no spawn-time ceiling for this developer-open run.
- `workspace_doctor` reported the X11 workspace dependencies and bubblewrap
  backend ready.

Verified:

- A harmless `workspace_start` dry-run returned a machine-readable approval
  bundle under `start_preview.approval`, including the hidden-workspace
  acknowledgement requirement. This rechecked the app-facing approval payload
  shape after restart without creating a workspace.
- A synthetic browser data directory at
  `/tmp/agent-workspace-browser-session-dogfood` was used with the
  `browser-session` profile template. The template mounted that directory
  read-write at `/workspace/browser-user-data`, required enforced policy, used
  `network.mode=inherit_host`, and declared the Chrome startup command with
  `--no-sandbox`.
- `workspace_open_profile` dry-run reported the start/profile/startup plan
  without launching a daemon. The real `workspace_open_profile` then started
  `dogfood-browser-session` on `:90`, launched Chrome as
  `browser-session-no-sandbox`, and found a visible
  `about:blank - Google Chrome` window tagged with app id `app-1809254`.
- Runtime status reported `mount_isolation=bubblewrap_mount_namespace`,
  `network_isolation=host`, profile id
  `dogfood-browser-session-synthetic`, and an applied policy snapshot with
  display isolation, input scope, and mounts enforced.
- A workspace command verified
  `/workspace/browser-user-data/Default/Cookies`, wrote
  `/workspace/browser-user-data/Default/AgentWorkspaceMarker`, and exited 0.
  The host then read the same marker at
  `/tmp/agent-workspace-browser-session-dogfood/Default/AgentWorkspaceMarker`
  with content `marker-from-workspace`, proving the mounted browser data path
  was shared as intended.
- `workspace_observe` returned the active Chrome window, hidden Chromium helper
  windows, pointer metadata, recent event history, and a root screenshot.
  `workspace_artifacts` listed the manifest, applied policy, event log, daemon
  logs, app logs, and screenshots.
- `workspace_stop` terminated the Chrome app with SIGTERM and wrote stopped
  status to the manifest. The temporary saved profile was deleted and
  `workspace cleanup` removed the stale runtime directory. `workspace_list`
  then returned no active/stopped runtime entries and `profile_list` returned
  no saved profiles.

Findings:

- The browser-session path is now proven for a synthetic Chrome profile through
  the installed MCP surface: template generation, approval preview, real
  startup, visible Chrome window discovery, mounted data reads/writes,
  screenshot/observe/artifacts, stop, profile deletion, and stale cleanup.
- This is still not a live real-account proof. Treat real shopping/account
  workflows as pending explicit user opt-in and a copied or otherwise safe
  browser profile test.
- The workspace command stderr contained repeated
  `Failed to create stream fd: No such file or directory` messages while still
  exiting 0 and producing the expected marker. This is low-priority runtime
  noise to investigate after the core gates.
- Fixed after this pass: cleanup now detects defunct helper PIDs from
  `/proc/<pid>/stat` before checking process names, so a stopped daemon that
  remains as a zombie is reported as already defunct instead of as an identity
  mismatch. The stale runtime directory removal behavior is unchanged.

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
- `cargo test` passed 46 tests, including permission-ceiling checks,
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
  Status/Hide status toggle. The corresponding feature tests now pass 15 tests.
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
- Follow-up dogfood found that `profile put` still rejected
  `network.mode=local_only` when `allow_hosts` was empty, even though the
  product model is closed/local/open and local means sandbox loopback. Fixed:
  `allow_hosts` is now optional for `local_only`, still validated as loopback
  labels when present, and still required for legacy `allowlist` profiles. The
  installed release binary accepted a dry-run and real
  `dogfood-local-only-empty` save, `profile check` reported
  `backend=bubblewrap_loopback_only`, and a live `workspace run` printed
  `loopback=ok` from an in-sandbox `127.0.0.1` server while
  `https://index.crates.io/config.json` failed DNS inside the sandbox.
- The Codex for Linux Agent Workspaces page now has a first browser-session
  preparation flow instead of jumping from folder picker to raw profile JSON. It
  opens a browser-data folder picker, shows the selected path plus an
  account-data/profile-lock warning, defaults to making a managed copy under
  Agent Workspace data while skipping browser lock files, and keeps direct
  read-write mounting as an explicit option. The feature tests cover the bridge
  copy path and generated UI source, and the installed app bundle was patched in
  both `content/webview` and `resources/app.asar` so the webview and main bridge
  load together after restart.
- A project-dev dogfood pass found a real A-gate bug after Codex/MCP restart:
  the `project-dev` template produced correct Rust mounts, but `cargo test`
  failed inside `mount_isolation=bubblewrap_mount_namespace` with
  `network_isolation=host` because `/etc/resolv.conf` was a symlink to
  `/run/systemd/resolve/stub-resolv.conf` and the restricted mount namespace did
  not expose that target. The runtime now adds a narrow read-only bind for the
  external resolver target directory when `/etc/resolv.conf` needs it. After
  reinstalling and restarting the `mcp-visible` workspace daemon,
  `workspace_run_app` resolved `index.crates.io`, `curl -I
  https://index.crates.io/config.json` returned HTTP 200, and `cargo test`
  passed 49 tests from `/workspace/project` through the live MCP path with
  `mount_isolation=bubblewrap_mount_namespace`.
- The same fresh `mcp-visible` workspace initially appeared as a black embedded
  desktop because it had no visible windows. Launching an `xterm` probe with
  `wait_window` and `screenshot_window` found `Agent Workspace Visible Probe`,
  captured a window screenshot, and `workspace_observe --screenshot` reported
  one visible `XTerm` window. The current black-screen state is therefore an
  empty-workspace UX issue rather than a dead stream.

Remaining gaps from this pass:

- `local_only` remains sandbox-local loopback. Host-localhost bridging is still
  a product/runtime gap.
- Network allowlists are no longer part of the current product gate. The
  user-facing network model for this phase is closed, local, or open; any
  `allowlist` profile data should be treated as advanced/legacy declared intent,
  not a promised filtering backend.
- Browser tasks that need logged-in sessions now have a starter
  `browser-session` profile and a first picker/copy/lock-warning flow. It still
  needs live dogfood against a real account profile before treating it as a
  comfortable default for shopping-style tasks.
- Hard permission enforcement in Codex for Linux should still wait until the UI
  approval boundary is wired so agents cannot call the same workspace tools
  outside the user-approved path.

Addressed in this pass:

- The stdio MCP smoke now covers the stop/revoke path directly. It starts a
  real workspace through MCP, runs a command inside it, verifies compact action
  responses, calls MCP `workspace_stop`, verifies the stopped status, previews
  `workspace_cleanup_stale` for that workspace, and then removes the stopped
  runtime through MCP. The old CLI cleanup remains only as an opt-out fallback
  for harness debugging.
- Agent Workspace approval prompts in Codex for Linux no longer show the raw
  `Params` JSON object when the app supplies a generic display payload. The
  renderer now unwraps the bridge `params` object, recognizes workspace/profile
  actions, and shows readable rows such as Action, Profile, Run setup, startup
  window waits, and acknowledgement flags.
- Browser-session creation in the Codex for Linux settings page no longer drops
  users directly into generated JSON after the folder picker. A preparation
  dialog now makes account data visible, defaults to a managed copied profile,
  excludes common browser lock files, and requires an explicit direct-folder
  choice before mounting a live browser data directory read-write.
- MCP and CLI app-action responses no longer embed long stopped-app history in
  nested `status.apps`. They keep the directly affected app in top-level
  `apps`, while explicit `workspace_status`, `workspace_observe`, and
  `workspace_list_apps` remain the full inspection surfaces. This is covered by
  unit tests, integration smoke, and the stdio MCP smoke.

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
- Addressed after this pass: the `project-dev` profile template now mounts
  Cargo's `bin` shims and rustup toolchains read-only when detected, while
  using a throwaway workspace `CARGO_HOME` so Cargo credentials and registry
  cache state are not mounted by default. Other user-local toolchains still need
  explicit mounts or future template support.
- MCP dogfood verified the same profile shape by saving a temporary
  `dogfood-project-dev` profile, launching a command in the existing
  `mcp-visible` workspace with `mount_isolation=bubblewrap_mount_namespace`,
  and confirming `PWD=/workspace/project`, `cargo --version`, `rustc --version`,
  mounted rust directories, and absence of `/workspace/rust/credentials.toml`.
  The temporary profile was deleted after the run.
- Existing limitation: `local_only` is a sandbox-local loopback namespace. It
  does not bridge host localhost services into the workspace.
- Current product boundary: do not expand the network gate into broad host
  allowlists or egress proxies. The scoped user-facing model is closed, local,
  or open; `allowlist` remains advanced/legacy declared intent if encountered.
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
- 2026-05-25 installed-app B-gate dogfood verified the conversation embedded
  workspace panel against the patched user-facing Codex app bundle. A hidden
  workspace launched
  `/home/avifenesh/.local/opt/codex-desktop-linux/codex-app/start.sh
  --new-instance -- --remote-debugging-port=9338` with real
  `CODEX_HOME=/home/avifenesh/.codex` and
  `CODEX_AGENT_WORKSPACE_BIN=/home/avifenesh/.local/bin/agent-workspace-linux`.
  Chrome DevTools Protocol attached to the installed Electron renderer at
  `http://127.0.0.1:5175/?mcpAppSandboxDevtools=1`. The conversation surface
  showed `Codex app nonoptional MCP card dogfood`, Refresh/Stop/Revoke controls,
  `:90 - 1 app: installed-codex-conversation-refresh-qa`, and an embedded
  screenshot data URL with natural size `1280x800`. A CDP click on Refresh
  completed, the screenshot data URL changed length, and no relevant
  console/runtime errors were observed. Screenshot evidence was captured at
  `/tmp/installed-conversation-panel-after-refresh.png`. Only the launched
  installed-app probe `app-1535846` was killed afterward; the `mcp-visible`
  workspace remained running.
- 2026-05-25 C-gate arbitrary-app dogfood found and fixed a PID-less X11 window
  targeting gap. `xcalc` launched in `dogfood-xcalc-appid` and exposed a
  `Calculator` window with `pid=null`; before the fix, later `--app` window
  filters could not recover that window. The daemon now remembers windows that
  appear from the launch fallback and annotates them with the launched app id.
  Installed-binary validation showed `workspace launch --wait-window
  --screenshot-window -- xcalc` returning window `4194326` with
  `app_id=app-1785548`, `workspace windows --app app-1785548 --all` finding the
  PID-less window, and app-targeted `minimize-window`, `show-window`,
  `move-window`, and `screenshot-window` succeeding. The temporary workspace was
  stopped and cleaned afterward.
- 2026-05-25 Codex-spawned MCP A-gate pass revalidated real workloads after the
  embedded viewer v12 polish. `mcp_permissions` reported no spawn-time ceiling,
  `workspace_doctor` reported ready X11 runtime dependencies plus bubblewrap
  support for mounts, disabled networking, and local-only networking, and both
  `workspace_list` and `profile_list` started empty. A temporary
  `dogfood-project-qa` profile from the `project-dev` template mounted this
  repository read-write at `/workspace/project`, mounted Cargo shims and rustup
  read-only, and kept host networking. `workspace_open_profile --dry-run`
  returned the hidden-workspace approval bundle without creating a daemon; the
  real `workspace_open_profile` then started the workspace on `:90`.
  Daemon-attached `workspace_run_app --dry-run` previewed the launch policy, the
  real preflight command saw `/workspace/project`, Rust `1.95.0`, Cargo
  `1.95.0`, the mounted rustup tree, and a writable project mount, and
  `cargo test --locked` passed 54/54 from inside the hidden workspace.
- The same pass launched `xterm` as `dogfood-xterm` through
  `workspace_launch_app --wait-window --screenshot-window`, focused it by title,
  typed shell commands with workspace-local IPC, sent `Return`, captured window
  and root screenshots, and verified the typed command wrote `typed-ok` through
  the mounted project path. `workspace_observe`, `workspace_events`,
  `workspace_list_windows`, `workspace_ipc_info`, `workspace_artifacts`,
  `workspace_stop --dry-run`, real `workspace_stop`, and
  `workspace_cleanup_stale` all returned useful state. Cleanup preview and
  actual cleanup reported the stopped daemon as already defunct and removed the
  stale runtime, matching the defunct-PID cleanup fix.
- The same pass created strict temporary `dogfood-net-disabled` and
  `dogfood-net-local` profiles with `require_enforced_policy=true`.
  `profile_check` reported `backend=bubblewrap_unshare_net` for disabled
  networking and `backend=bubblewrap_loopback_only` for local-only networking.
  In disabled mode, `curl https://example.com` failed with `BLOCKED:6` and
  `Could not resolve host`. In local-only mode, a Python HTTP server and curl
  running inside one launch namespace returned `LOCAL:loopback-ok`, while the
  same command still blocked external DNS with `EXTERNAL_BLOCKED:6`.
- The same pass created `dogfood-restricted-chrome` from the
  `restricted-chrome` template and used only an isolated throwaway Chrome
  profile under `/tmp`, not the user's browser profile. `workspace_open_profile
  --dry-run` showed one declared startup app without launching a daemon. The
  real open launched Google Chrome with
  `network_isolation=bubblewrap_unshare_net`, found the visible
  `about:blank - Google Chrome` window, and captured a screenshot. Workspace
  input then navigated Chrome to `example.com`; `workspace_observe` captured the
  Chrome page titled `example.com - Google Chrome` showing
  `ERR_INTERNET_DISCONNECTED`. The Chrome workspace was stopped and stale
  cleanup removed the runtime afterward.
- 2026-05-25 installed-app B-gate v12 dogfood verified the polished embedded
  viewer in the patched installed Codex app bundle. A hidden workspace
  `codex-v12` launched
  `/home/avifenesh/.local/opt/codex-desktop-linux/codex-app/start.sh
  --new-instance -- --remote-debugging-port=9340` with
  `CODEX_AGENT_WORKSPACE_BIN=/home/avifenesh/.local/bin/agent-workspace-linux`.
  After Electron painted, `workspace_observe --screenshot` showed the installed
  app conversation surface with the embedded Agent Workspace panel, live
  recursive screenshot, and Refresh/Details/Stop/Revoke controls. A
  workspace-local click opened the new Details tray, which displayed the active
  window as `Codex`, the running app as `installed-codex-v12-viewer`, and the
  hidden display as `:90` without raw JSON. A second workspace-local click on
  the embedded Stop button stopped the workspace from inside the app; saved
  events recorded the click, the installed app exiting by SIGTERM, and
  `workspace_stop`, then stale cleanup removed the stopped runtime.
- 2026-05-25 C-gate locked-host setup polish added installer support for
  `./install.sh --permissions PATH`, which writes the Codex MCP registration as
  `args = ["mcp", "--permissions", "PATH"]` instead of requiring hand edits to
  `config.toml`. Dry-run validation showed both the default open registration
  and the locked registration shape. `bash -n install.sh`,
  `node scripts/mcp_permissions_smoke.js`, and `cargo test --locked` all passed
  afterward.
- 2026-05-25 C-gate locked-host CLI polish added
  `permissions template open|closed|local` and
  `permissions validate --json PATH`, so auto-loop agents and non-Codex MCP
  hosts can generate and preflight ceiling files before spawning MCP. The
  integration smoke now uses the template command to create its locked CLI
  ceiling and validates that file before proving the ceiling blocks broader
  profiles. The same pass also added MCP zombie-child regression coverage after
  stopped workspaces exposed stale `agent-workspace` children under a long-lived
  MCP process.
