# Handover: GPUI Viewer Direction

Date: 2026-05-25
Last updated: 2026-05-26

## User Intent

The user wants `agent-workspace-linux` to mature into an MCP that is useful
outside Codex Desktop too. The revised product direction is:

- The MCP distribution should own the main Agent Workspace UI.
- Codex Desktop integration should become thinner rather than maintaining a
  second serious workspace UI surface.
- Use Rust/GPUI for a native viewer, inspired by the user's `cidux` project.
- It is acceptable for this OSS MCP to accept some GPUI/product risk and iterate
  quickly, because Zed itself is built on GPUI and `cidux` already proves GPUI is
  practical on this machine.

The intended command shape is:

```sh
agent-workspace-linux mcp
agent-workspace-linux viewer
```

The MCP now exposes `workspace_open_viewer`, which launches the viewer
intentionally as a small floating host-visible monitor window unless the MCP
process was started with `--headless`.

Important caveat: do not run a GPUI event loop inside the MCP stdio server
process. The viewer should be a subcommand/child process in the same binary, not
the active stdio MCP process.

## Repos And Branches

Runtime repo:

```sh
cd /home/avifenesh/projects/agent-workspace-linux
git status --short --branch
```

Desktop repo:

```sh
cd /home/avifenesh/projects/codex-desktop-linux
git status --short --branch
```

Do not trust the inline status from older handovers. The runtime tree is
intentionally dirty while this feature is being brought to release shape, and
the sibling Desktop repo is part of the review scope through
`../codex-desktop-linux/linux-features/agent-workspace`. Use
`scripts/release_next_steps.py --json` for the authoritative current source
hash, review-scope hash, source bundle, and remaining gates.

## Last Pushed Runtime State

Latest pushed commit:

```text
8d0cdf2 Prefer app ids for workspace window control
```

Recent pushed work before the handover:

- `762c164 Add native GUI smoke coverage`
- `8f00de7 Smoke test browser session startup`
- `0e0873a Treat workspace actions as scoped after approval`
- `8d0cdf2 Prefer app ids for workspace window control`

The last pushed runtime was verified with:

```sh
cargo fmt --check && cargo test --locked
cargo build --locked && node scripts/mcp_permissions_smoke.js
scripts/integration_smoke.sh
scripts/gpui_viewer_smoke.sh
AGENT_WORKSPACE_BIN=/home/avifenesh/.local/bin/agent-workspace-linux node scripts/mcp_permissions_smoke.js
```

It was also installed with:

```sh
./install.sh
```

The installer reported old MCP processes still running at that time, so Codex
needed a restart/reload to see the newest MCP schema.

## Current Uncommitted GPUI Work

The GPUI viewer is now wired and smoke-tested locally, but still uncommitted.
Local changes in the runtime repo include:

Additional local verification after the stopped-workspace `Clean`,
running-workspace `Rev`, compact active-app `Log`, MCP permission-ceiling
handoff/display, live MCP control-state gating, X11 overlay window-manager hints, compact
workspace-switcher/footer-intelligence, and event-log shortcut slices:

```sh
cargo fmt --check
cargo check --locked
cargo build --locked
cargo test --locked
cargo test --locked viewer::tests
node scripts/mcp_permissions_smoke.js
scripts/gpui_viewer_smoke.sh
```

Latest local prod-readiness passes after wiring real-grocery dogfood
requirements into `mcp_task_plan`, removing the embedded Codex conversation
screen, adding reusable observe/viewer frame coverage, external evidence
import/source-bundle hardening, viewer duplicate-launch protection, and direct
stdio MCP viewer lifecycle cleanup. Browser control now has a repo-owned path
too: `workspace_browser_targets` discovers page targets from the running
workspace Chrome/Chromium app's profile and loopback DevTools endpoint,
`workspace_browser_snapshot` reads page title/text/links, and
`workspace_browser_navigate` changes the workspace browser page while logging
workspace events, so agents can use the workspace browser instead of the user's
host Chrome bridge or external browser-control workarounds:

```sh
scripts/prod_readiness_smoke.sh
```

That gate covers `cargo fmt --check`, `cargo build --locked`,
`cargo clippy --locked -- -D warnings`, `cargo test --locked`, the focused
MCP/viewer/grocery smokes, `scripts/integration_smoke.sh`,
`scripts/viewer_desktop_matrix_probe.sh`, `git diff --check`, and the sibling
Codex Desktop agent-workspace node checks. It also verifies that copied
external evidence rejects stale/nonpassing rows, rejects source identity
recomputed from an extracted no-git bundle, accepts manifest-stamped external
bundle rows, and that `release_next_steps.py` recomputes live
source/review-scope identity instead of trusting stale bundles.

Operational boundary: do not use the Codex app MCP, Computer Use MCP,
Playwright MCP, or Codex Desktop bridge as evidence for this runtime work.
Exercise the repo-owned MCP
directly with `target/debug/agent-workspace-linux mcp` over stdio and the
repo-owned CLI subcommands. For browser work, use Chrome/Chromium launched
inside the workspace and the MCP `workspace_browser_targets`,
`workspace_browser_snapshot`, and `workspace_browser_navigate` tools; do not
substitute the host Chrome bridge unless the user explicitly asks for a
host-browser session. The current direct viewer lifecycle smoke is
`scripts/mcp_viewer_lifecycle_smoke.js`; it starts the direct MCP, opens a GPUI
viewer with `workspace_open_viewer`, finds it through `workspace_list_viewers`,
previews and closes it with `workspace_close_viewer`, and verifies the registry
no longer reports a live viewer. The current direct browser-control smoke is
`scripts/mcp_workspace_browser_cdp_smoke.js`; it launches workspace Chrome,
discovers the loopback DevTools targets through the MCP, then performs page
snapshot and navigation through MCP tools without touching host Chrome or a
separate CDP client.

Do not copy old timestamped evidence paths or hashes from this document into a
release decision. Run these after source/doc edits instead:

```sh
scripts/release_gate_audit.py
scripts/final_review_bundle.py
scripts/objective_completion_audit.py
scripts/release_next_steps.py
```

The latest release audit, source bundle, final-review bundle, and human-marker
template should be regenerated after any documentation edit, because the
review-scope identity intentionally includes docs and untracked files. Use
`scripts/release_next_steps.py` after regeneration for the authoritative
current paths and hashes.

When the user already has a real GPUI viewer open for dogfood, do not run the
default broad smoke in a way that opens and closes a second monitor window. Use
`AGENT_WORKSPACE_NO_NEW_VIEWER=1 scripts/prod_readiness_smoke.sh` for
non-disturbing iterative validation; it skips the direct viewer lifecycle smoke
and visual GPUI smoke while still refreshing source-bound non-viewer evidence.
Run the default broad smoke only when strict visual viewer evidence is needed.

`scripts/release_gate_audit.py` now turns the remaining release-only checks into
a machine-readable report under `target/release-gate-audit/`. Default mode is
non-failing so local smoke can still pass while naming the external gaps;
`--require-all` or `REQUIRE_RELEASE_GATES=1` makes those gaps block a release.
`--max-evidence-age-days` defaults to 14 so old JSON does not silently prove a
new release; `0` disables that freshness check. Evidence also needs to match the
current combined source identity, a hash over the runtime source
(`Cargo.toml`, `Cargo.lock`, `src/`, and `scripts/`) plus the sibling Codex
Desktop feature source (`../codex-desktop-linux/linux-features/agent-workspace`
and `../codex-desktop-linux/agent-workspaces-linux.js`, or
`CODEX_DESKTOP_LINUX_REPO` when set), unless `--no-source-identity-check` is
passed intentionally. Strict release mode also passes `--require-clean-source`,
so those runtime and Desktop feature source paths must have no git status
entries. The human-review marker must also match the current review-scope
identity. In a dirty worktree it hashes both repos' status, staged and unstaged
diffs, and non-ignored untracked file contents; in a clean worktree it hashes
the current `HEAD` commit content. `--self-test` verifies pending,
stale-complete, mismatched-source, mismatched-review-scope, dirty-source, and
fresh-clean-complete synthetic evidence paths, so the audit has a checked
success path before real external rows are collected.
Native Wayland rows need notes: the matrix probe rejects
`NATIVE_WAYLAND_LAYER_SHELL_OBSERVED=1` unless
`NATIVE_WAYLAND_LAYER_SHELL_NOTES` is set, and the audit only counts the row
when those notes are present on a Wayland session and make a positive
layer-shell/top-layer claim. The importer and audit reject GNOME/Xwayland
fallback notes or notes that say the viewer was not layer-shell. Real-grocery
release rows also require an HTTPS, non-local grocery URL; localhost, reserved,
and private-network URLs are limited to synthetic smoke coverage.
Viewer matrix rows now include session consistency metadata. When `loginctl`
session metadata is available, rows with contradictory session-type claims are
not release eligible, and the importer rejects them by default.
The audit currently reports the expected pending items for the active source
hash: KDE/Plasma viewer row, real X11 viewer row, native Wayland compositor
observation with notes, real-browser grocery cart-draft pass with a disposable
copied profile, and human final diff review. The current Ubuntu GNOME Wayland
host continues to prove the default X11/Xwayland viewer path, but do not carry
old native Wayland observation rows forward after source changes; collect/import
fresh source-matched rows instead.
`workspace_open_viewer` and direct `viewer` launches now keep a small runtime
registry under `$XDG_RUNTIME_DIR/agent-workspace-linux/viewers` so repeated
direct launches for the same workspace/backend/topmost/lifecycle mode reuse the
live viewer instead of creating duplicate detached GPUI windows. The MCP
`workspace_open_viewer` path is more conservative: if any registered viewer for
the requested workspace is already alive, it returns that viewer instead of
opening another topmost or backend variant. Registry liveness checks still verify
the workspace id, executable, `--always-on-top`, and `--exit-when-workspace-gone`
command shape before a row is considered alive.
MCP-opened viewers now pass `--exit-when-workspace-gone`, which keeps the direct
CLI viewer useful as a persistent launcher while preventing `workspace_open_viewer`
monitors from becoming orphan GPUI windows after their target runtime is removed.
If a GPUI viewer is still orphaned or hidden from compositor-level automation,
use the repo-owned registry path rather than host UI tooling:

```sh
target/debug/agent-workspace-linux viewer list
target/debug/agent-workspace-linux viewer close --id <workspace-id> --dry-run
target/debug/agent-workspace-linux viewer close --id <workspace-id>
```

The same recovery surface is available through direct MCP as
`workspace_list_viewers` and `workspace_close_viewer`. The close path only sends
`SIGTERM` to registered viewer pids whose `/proc/$pid/cmdline` still matches
the recorded viewer command, and `--all` / `all=true` is reserved for explicit
cleanup after inspecting the registry.
The human-review gate can pass only after a human-created
`target/release-gate-human-review.json` marker records schema
`agent-workspace-linux.human_final_diff_review.v1` with status `reviewed`.
`scripts/prepare_grocery_profile_copy.js` is the expected way to create the
real-grocery disposable browser profile; it skips browser locks, caches,
extension/web-app payloads, and other volatile artifacts, writes
`.agent-workspace-grocery-profile-copy.json`, and the real-browser probe now
requires that manifest before opening the target grocery site. The guarded
wrapper defaults the copy destination outside the repo `target/` tree under
`$XDG_RUNTIME_DIR/agent-workspace-linux/grocery-profile-copy`, or `/tmp` when
`XDG_RUNTIME_DIR` is unavailable, so live dogfood does not dump a large browser
copy into the checkout by default. Release
grocery evidence is no longer observe-only: it must run
`REAL_GROCERY_INTERACTION_MODE=cart-draft-approved` with explicit cart approval,
final cart review, no checkout approval, and a site-specific
`GROCERY_CART_DRAFT_STEPS_JSON` file whose input steps stay inside the declared
cart-draft boundary. The live real-browser report must now include
`workspace_browser_targets` and `workspace_browser_snapshot` evidence for the
workspace Chrome/Chromium app's loopback DevTools target and page readback;
this keeps grocery dogfood tied to the repo-owned workspace browser path and
not the host Chrome bridge. The release artifact keeps only URL/title plus text
length/truncation metadata from that readback; raw logged-in page text,
excerpts, links, and headings are omitted and rejected. The probe can print a
starter step file with
`--print-cart-draft-steps-template`, validate one with
`--validate-cart-draft-steps PATH`, and self-test those safety checks with
`--self-test`. The same release-facing contract is now visible in grocery
`mcp_task_plan` responses as `task_context.dogfood_requirements[]`, so MCP hosts
do not have to scrape release docs before guiding an agent through a real
cart-draft dogfood run. The requirement status flips to `ready` only when the
planner receives the disposable-profile, validated-step-file, cart-approval, and
final-review prerequisites without checkout/real-world approval.
`scripts/final_review_bundle.py` is the handoff surface for the final human diff
review: it writes JSON and Markdown under `target/final-review-bundle/` with the
combined runtime/Desktop source identity, review-scope identity,
runtime/Desktop dirty scope, latest evidence paths, release-audit/current-source
and review-scope consistency, pending gates, concrete next evidence commands,
checklist, generated runtime/Desktop review diffs, and review-marker template,
without creating the marker. The marker must include meaningful reviewer/notes
metadata plus `review_artifacts` hashes that the release audit verifies before
accepting the human review gate. Set `HUMAN_REVIEW_NOTES` to specific
non-placeholder notes before running the marker command.
Because the review-scope identity includes docs and untracked files, regenerate
the audit and final-review bundle after any final doc edits and before creating
the human-review marker.
`scripts/import_release_evidence.py` is the safe path for copied external
viewer/app-QA/grocery JSON reports; it rejects stale-source, skipped/failed
viewer rows, unsafe app-QA rows, plan-only/local/test grocery evidence, missing
executed cart-draft steps, and executed step labels that mention
checkout/order/payment/account mutation by default before the release audit sees
it. Viewer, app-QA, and real-grocery reports must include an
`evidence_boundary` object proving they were collected through the repo-owned
runtime collector without Codex app MCP, Computer Use MCP, or Codex Desktop
bridge evidence. New reports also carry `playwright_mcp_used=false`; the
release audit and importer reject missing or true values so browser dogfood
cannot quietly fall back to Playwright artifacts. The importer also requires
every executed cart-draft step to report a successful result.
`scripts/export_release_evidence_bundle.py` is the portable source-bundle path
for external viewer rows: it writes a tarball with the runtime source, sibling
Desktop feature source, source/review-scope manifest, and
`collect-viewer-evidence.sh`, so KDE/X11/native Wayland probes can be collected
against the exact source identity the release audit expects. Run it after final
doc/source edits, because the review-scope manifest intentionally includes docs.

`scripts/prune_evidence_reports.py` bounds local evidence churn from repeated
smoke/audit runs. It prunes timestamped groups in the known `target/` evidence
directories while keeping related JSON/Markdown/diff/log/tarball files together.
`scripts/prod_readiness_smoke.sh` runs its self-test and then prunes after the
objective audit, keeping 25 groups per directory by default. Set
`AGENT_WORKSPACE_REPORT_RETENTION=0` for an archival run that should leave all
reports untouched, or run `scripts/prune_evidence_reports.py --dry-run` to
preview. The helper protects rare release-grade external rows beyond the normal
keep window: KDE/Plasma viewer rows, X11 viewer rows, release-positive native
Wayland compositor observation rows, and passed real-browser grocery dogfood
reports. GNOME/Xwayland fallback notes are pruned like ordinary viewer rows.

An extra disposable stopped-workspace smoke rendered the viewer against
`gpui-clean-smoke-*` and verified that `workspace cleanup --id` removes the same
stopped runtime that the viewer action calls. Host click injection still did not
reach the Xwayland popup: `xdotool` left the workspace present, and the Linux
Computer Use portal click timed out after 120s.

The regular `scripts/gpui_viewer_smoke.sh` now creates a stopped companion
workspace before opening the viewer, so the compact workspace position/switcher
control is exercised during the visual smoke without using ydotool-backed host
input. It also checks `workspace artifacts --existing` for a real `event_log`
artifact and `workspace logs --stream stdout xclock` for a real app-log path
before opening the viewer, matching the new `Evt` and `Log` shortcut paths.

The running-workspace `Rev` backend was checked with a disposable
`gpui-revoke-smoke-*` workspace using the same runtime calls as the viewer
action: `workspace start`, `workspace stop --timeout-ms 30000`, then
`workspace cleanup --id`, with cleanup proving the target id was removed.

`cargo test --locked viewer::tests` covers the compact footer mode cycle and
the pure label logic for enforced policy, unenforced-policy acknowledgement,
active/running app summaries, active-app log target selection,
permission-ceiling footer labels, and distinct destructive action busy labels.

1. `Cargo.toml` / `Cargo.lock` now have GPUI dependencies:

```toml
gpui = { git = "https://github.com/zed-industries/zed", branch = "main", default-features = false, features = ["font-kit", "wayland", "x11"] }
gpui_platform = { git = "https://github.com/zed-industries/zed", branch = "main", default-features = false, features = ["wayland", "x11"] }
image = { version = "0.25", default-features = false, features = ["png"] }
x11rb = "0.13.2"
```

2. `src/viewer.rs` was added as a native GPUI viewer.

3. `scripts/gpui_viewer_smoke.sh` was added as a focused X11/Xwayland visual
   smoke for the floating monitor. It builds the binary, starts a disposable
   hidden workspace, creates a stopped companion workspace so the compact
   workspace switcher renders, launches `xclock`, proves
   `workspace screenshot-window`, opens the GPUI viewer, captures the viewer
   X11 window, checks that it advertises skip-taskbar/skip-pager
   notification-style window-manager hints, and checks that the capture is
   nonblank and still monitor-sized.

What `src/viewer.rs` currently provides:

- `ViewerOptions { id }`
- `run(options)` that opens a native GPUI monitor with no titlebar, no app
  chrome, and no keyboard focus, so it behaves like a small monitor rather than
  a full app window. The default path does not request always-on-top state.
  `--always-on-top` opts into Wayland layer-shell anchored top-right on
  `Layer::Overlay` where available, or X11/Xwayland above/sticky hints on GNOME
  Wayland and X11 desktops.
- `open_viewer(id, always_on_top)` that spawns
  `agent-workspace-linux viewer --id <id>` as a child process and returns id /
  pid / backend / `always_on_top` / executable / command
- shared live MCP control state in `mcp-control.json` under the runtime dir:
  `mcp_control_state` reports `active`, `read_only`, or `paused`, and
  `mcp_control_update` switches modes. `read_only` and `paused` block mutating
  MCP actions at the server boundary while leaving read-only inspection and
  safety stop available. MCP-side reactivation from `read_only`/`paused` to
  `active` now requires `confirmed_user_request=true`, while the GPUI viewer
  uses the local control plane for direct compact controls: `RO` / `Pause`
  while active, `Run` / `Pause` while read-only, and `Run` / `RO` while paused.
  Live-control checkpoints include that exact required input, and session briefs
  include the latest control `updated_by`, `updated_at_unix`, and reason so
  agents and UI can explain who changed the boundary.
- `mcp_action_catalog` returns the machine-readable action taxonomy for every
  MCP tool, including read-only, destructive, idempotent, open-world, and
  live-control behavior hints for agent approval planning. It now also returns
  advisory `parameter_notes` for arguments such as `dry_run`, `replace`,
  `output_path`, and `kill_on_timeout`; these notes improve agent approval UX
  but do not impose any extra permission ceiling when the MCP is spawned without
  `--permissions`.
- `mcp_session_brief` returns a single read-only orientation payload for agents
  and MCP hosts: permission ceiling, live control, runtime readiness,
  profile/workspace counts, headless state, and suggested next actions with
  action type, idempotency, and compact approval/open-world checkpoints. It now
  includes read-only `mcp_task_plan` recommendations derived from saved profiles
  and runtime state, so agents are nudged into app-QA, browser/shopping/grocery,
  observation, or cleanup plans before direct mutation.
- `mcp_task_plan` returns read-only intent plans for app QA,
  browser/shopping/grocery work, observation, and cleanup. It points agents to
  safe dry-run previews, profile templates, explicit browser-profile approval
  needs, step dependencies, and live-control constraints before any mutating
  MCP call. It now also emits structured `approval_checkpoints` so host UI and
  agents can render required input, dry-run approval surfaces, profile writes,
  hidden workspace starts, live-control blockers, host-visible UI, permission
  blockers, destructive actions, and separate real-world approvals without
  scraping step prose. Plans also emit `task_context` with normalized task kind,
  target workspace, provided inputs, missing inputs, safety boundaries, and
  approval kinds, so host UI can render inferred user intent without parsing
  step text. App-QA plans generated from a project path now continue through
  reviewed profile save, approved profile start, and post-start observation.
  Browser/shopping plans now continue through the approved browser-profile run
  and post-start observation step, with explicit separate approval text for
  checkout, purchases, and account changes. The optional viewer step is now
  offered only after the browser plan has a concrete run step and only when the
  MCP is not headless, so missing browser-profile input does not suggest a
  host-visible window before there is a valid workspace to watch. Cleanup plans
  now include the destructive follow-up and verification step after the dry-run
  approval surface. Generated project-dev/browser-session profile steps and
  saved-profile preview/run steps are preflighted against the active MCP
  permission ceiling and expose permission blockers before the agent attempts a tool call
  that would be rejected.
- Native view state:
  - workspace list via `workspace::list_workspaces()`
  - compact workspace switching, ordered with running workspaces first and
    stopped workspaces after them
  - profiles via `profile::list_profiles()`
  - selected saved profile id for compact profile-backed start
  - doctor readiness via `workspace::doctor_report()`
  - active-window lookup with a first-visible-window fallback
  - opt-in screen stream via `workspace::screenshot(id, Some("viewer-frame.png"))`
    decoded into `RenderImage`; regular status refresh does not capture pixels
  - compact button chrome with a 10px radius, muted grey surfaces, and subtle
    silvery borders; danger buttons keep a quiet red text cue without the older
    warm brown edge.
  - last-good-frame retention, so transient screenshot errors do not blank the
    monitor
- Buttons:
  - Refresh
  - View/Still screen stream toggle
  - Workspace position/cycle button when more than one workspace is known
  - Profile cycle button when saved profiles exist and the target workspace is
    stopped
  - Shot, which captures the active or first visible workspace window
  - Footer link strip: Files opens the workspace runtime artifact folder, Evt
    opens the workspace event log externally when present and falls back to the
    artifact folder before the log exists, and Log opens the active app log using
    the same live-or-manifest lookup path as `workspace logs`
  - Rev, a two-step running-workspace revoke action that stops the workspace and
    removes its runtime files
  - Clean, a two-step stopped-workspace cleanup action backed by
    `workspace::cleanup_stale_workspaces(Some(id), false)`
  - Start
  - Stop
- Compact UI:
  - Codex-for-Linux-style quiet surface panels, slightly rounded
    silver-edged controls, subdued text, and status pills
  - native GPUI hover tooltips on terse controls so `Shot`, `Rev`, `Clean`,
    `Files`, `Evt`, `Log`, and footer mode labels stay compact but explain the
    action on hover
  - persisted viewer size, popup position, live-refresh preference, and footer
    mode in
    `agent-workspace-linux/viewer.json` under the user's XDG config directory
  - compact active-workspace header with workspace id and live/lifecycle
    controls; passive external links live in the footer link strip so the header
    does not become a full management toolbar
  - screenshot surface inside a restrained card
  - one detail line for display/app/current window or readiness blockers
  - footer mode control for activity, inferred task intent, isolation/profile
    policy, and apps summaries; urgent action/error/revoke/cleanup states still
    override the mode text
  - isolation mode also names the MCP permission ceiling inherited by the viewer
    child process, so the user can see whether starts/launches are open or
    narrowed by network, mount, or app allowlist constraints

Start behavior:

- If no saved profile is selected, Start uses the default hidden workspace start
  path.
- If a saved profile is selected, Start applies the profile, runs setup, launches
  startup apps with `wait_window`, acknowledges the hidden workspace and
  unenforced-policy prompts as an explicit user-facing local action, then
  refreshes the monitor.
- Start, profile-backed start, and stop actions now run through GPUI's
  background executor. The viewer keeps repainting, marks the controls as
  `Working` / `Starting` / `Stopping`, suppresses live refresh while the action
  is in flight, and refreshes the snapshot when the action completes.
- Periodic refresh, workspace/window lookup, screenshot capture, PNG decode, and
  `RenderImage` construction also run through GPUI's background executor. The
  UI thread only applies the completed refresh result, shows a compact `Syncing`
  state while refresh is in flight, and keeps the last good frame on transient
  screenshot errors.
- The footer now favors human work context over raw capture metadata. It shows
  in-flight viewer actions first, then the latest actionable workspace event
  from `events.jsonl`, plus the active app/window or running apps. Passive
  viewer refresh/screenshot noise is filtered out.
- The footer mode control cycles through Activity, Task, Isolation, and Apps.
  Task infers app-QA, browser/shopping, observation, or stopped-workspace review
  from the selected workspace purpose/profile, active window, and app labels.
  The Isolation text summarizes profile id, display/input scope, network mode,
  mount state, and unenforced-policy acknowledgement. The Apps text names the
  active app/window plus running/stopped app counts.
- Latest activity labels now resolve matching app/window ids back to the
  current app or active-window label when possible, so footer text says
  `xclock` rather than raw `app-*` or X11 ids in common monitor paths.

3. `src/main.rs` now exposes:

```sh
agent-workspace-linux viewer [--id ID]
```

4. `src/server.rs` now exposes:

```text
workspace_open_viewer
```

The tool intentionally launches a host-visible viewer child process outside the
MCP stdio server and returns the child pid / executable / command / workspace
id. The server remains UI-capable by default, but `agent-workspace-linux mcp
--headless` disables host-visible viewer launches; in that mode the tool returns
`ok=false` instead of opening a window. `scripts/mcp_permissions_smoke.js` now
starts the MCP with `--headless`, asserts that `workspace_open_viewer` is
annotated as open-world, and checks the refusal path.
`scripts/mcp_clean_permissions_smoke.js` starts `mcp --headless` without a
permissions file and proves the clean/default path stays harness-owned:
`mcp_permissions.configured=false`, action catalog classification remains
advisory, and app-QA/browser task plans do not create permission blockers.
`mcp_session_brief` now also includes compact activity for running/stopped
workspaces and their recent app labels, so agents and host UI can orient around
what is actually active before calling heavier observe or input tools. Activity
entries now also carry an inferred intent from profile/app names, and the brief
adds a read-only `mcp_task_plan` recommendation for the running workspace when
the activity looks like app QA or browser/shopping work.
`mcp_task_plan` now treats an already-running target workspace as a live
continuation: app-QA and browser/shopping plans observe/list running apps rather
than starting another profile. Fresh-start and running plans now collect
read-only evidence before input: recent events are ready to call after the
workspace exists, while app logs and focused window screenshots wait for a
stable `app_id` or `active_window.id`. Browser/shopping plans keep the separate
purchase/checkout/account-change approval boundary. Shopping/grocery plans now
also ask for task details such as `target_url`, `shopping_list`, `fulfillment`,
`substitution_policy`, and `budget`; those prompts are required user input, not
permission blockers, so clean/default MCP creation stays harness-owned unless a
`--permissions` file is explicitly provided.
`task_context.action_boundaries` now gives host UI and agents a structured
browser/grocery boundary map: observe, navigate/search, compare items, draft cart
changes, and checkout/order/account changes. Cart mutation is separate from
final checkout approval so agents can be useful in grocery workflows without
quietly treating order submission as just another browser input.
The locked MCP smoke now verifies both `read_only` and `paused`: dry-run
previews stay callable, planning exposes live-control blockers, real starts are
blocked, read-only profile export still works, and `profile_export` with
`output_path` is blocked without writing a host file.

Verification completed in this session:

```sh
cargo fmt
cargo check
cargo run -- viewer --help
cargo run -- --help
cargo build --locked
cargo fmt --check
cargo test --locked
node scripts/mcp_permissions_smoke.js
timeout 5s target/debug/agent-workspace-linux viewer --id default
```

The bounded direct viewer launch stayed alive until `timeout` killed it
(`exit=124`) with no stderr. A focused JSON-RPC smoke call to
`workspace_open_viewer` returned a real child pid and command, then the child
was killed; no viewer process remained afterward.

Live workspace verification also passed after the compact floating UI/style
changes:

```sh
cargo build --locked
target/debug/agent-workspace-linux workspace start --ack-hidden-workspace --id gpui-live-smoke --purpose "GPUI viewer live smoke" --width 800 --height 500
target/debug/agent-workspace-linux workspace launch --id gpui-live-smoke --name xclock --wait-window --window-timeout-ms 10000 --screenshot-window -- xclock
target/debug/agent-workspace-linux workspace windows --id gpui-live-smoke
target/debug/agent-workspace-linux workspace screenshot --id gpui-live-smoke --output /tmp/agent-workspace-gpui-live-smoke.png
timeout 5s target/debug/agent-workspace-linux viewer --id gpui-live-smoke
target/debug/agent-workspace-linux workspace stop --id gpui-live-smoke --timeout-ms 15000
```

Observed output included `window_titles=xclock`, an 800x500 PNG screenshot with
nonzero pixels (`mean=4896.09`), viewer timeout exit `124` with no stderr, and a
successful workspace stop. The smoke workspace was cleaned up; it is not
running.

Profile-backed smoke also passed with a temporary `XDG_CONFIG_HOME` profile
named `gpui-profile-smoke` whose startup app was `xclock`. The profile was
created, `workspace open-profile` returned `ready=true startup=true`, workspace
windows reported `window_titles=xclock`, the screenshot was 800x500 with
nonzero pixels (`mean=4895.82`), the viewer stayed alive until timeout
(`exit=124`), and the workspace/config were cleaned up.

The integration smoke also now exercises the synthetic `browser-session` path
end to end when Chrome/Chromium is installed: template, validate, import,
profile-backed startup, screenshot-backed observation with events and a running
browser-session app, browser-data mount write-through, stop, and profile
deletion.

System dependency discovered and fixed locally:

```sh
sudo -n apt-get install -y libxkbcommon-x11-dev
```

GPUI linked only after the `libxkbcommon-x11-dev` package provided the
`libxkbcommon-x11.so` linker name.

Follow-up readiness pass:

- `workspace::doctor_report()` now reports a separate `viewer` section with
  `host_display`, `source_build_xkbcommon_x11`, and `host_opener` checks.
- `ready_for_x11_workspace` still describes the hidden workspace runtime, while
  `ready_for_host_viewer` describes whether the current host session can open a
  visible GPUI viewer.
- `workspace_open_viewer` preflights `ready_for_host_viewer` before spawning the
  child process, and `mcp_session_brief.doctor` carries the viewer readiness
  fields.
- The README now documents `pkg-config libxkbcommon-x11-dev` as the
  Debian/Ubuntu-style source-build dependency for the GPUI viewer.
- A no-host-display MCP smoke now unsets `DISPLAY` and `WAYLAND_DISPLAY` while
  running plain `mcp` without `--headless`. It proves hidden workspaces can
  still start, while session briefs and task plans suppress host-visible viewer
  recommendations and `workspace_open_viewer` refuses with a doctor-backed host
  display message.
- `mcp_task_plan` now carries `host_viewer_ready`, `viewer_available`, and
  `viewer_unavailable_reason`, so the absence of viewer steps is
  machine-readable instead of inferred from missing `workspace_open_viewer`
  steps.
- Natural shopping phrases such as "buy", "purchase", "cart", "checkout",
  "order", and "delivery" now normalize to the browser/shopping planner instead
  of falling through to the unknown plan.
- Natural app-QA phrases such as "test the local UI", "verify the frontend",
  "debug the desktop window", "run smoke checks", and "check render behavior"
  now normalize to the app-QA planner. App-QA `task_context.action_boundaries`
  now separate read-only observation, hidden workspace start/attach, evidence
  collection, workspace-local input, and mounted project file writes. Mounted
  project file writes appear as a distinct `project_file_write` approval kind
  so host UI does not need to infer code-change approval from step prose.
- `mcp_task_plan.approval_summary` now gives hosts a compact next-boundary
  rollup: blocking checkpoint count, approval-required count, all approval
  kinds, and the first blocking or approval-required checkpoint. This keeps the
  detailed `approval_checkpoints` list intact while giving Codex Desktop and
  other MCP hosts one obvious prompt to render first.
- `mcp_session_brief` now mirrors that shape for recommendations: each
  recommendation carries `approval_summary`, and the brief has a top-level
  summary across its already-prioritized recommendations. A host can therefore
  show the first boundary from the orientation call before deciding whether it
  needs a full `mcp_task_plan`.
- `scripts/grocery_browser_workflow_smoke.sh` now provides a repeatable
  browser/grocery dogfood path. It opens a local grocery page in a hidden
  browser workspace, drives the shopping-list input through workspace-local
  typing/paste, confirms a drafted cart title of `cart:3:checkout-locked`, and
  fails if the flow crosses into an order-submitted state. It is wired into
  `scripts/integration_smoke.sh` when Chrome/Chromium is available.
- Grocery `task_context.action_boundaries` now carry explicit `approved` and
  `missing_approvals` state. `mcp_task_plan` accepts
  `cart_mutation_approved`, `final_cart_reviewed`, and
  `real_world_action_approved`, so a host can record cart-draft approval while
  keeping checkout/order/account actions blocked.
- Codex Desktop's thin integration can open the native GPUI viewer from the
  Agent Workspaces settings page. It launches `agent-workspace-linux viewer` as
  a detached child, keeps always-on-top opt-in, prepends the same MCP
  `--permissions` path when configured, leaves clean/default usage without a
  synthetic ceiling, and now listens for asynchronous spawn errors so a missing
  binary reports a bridge error instead of crashing the app. Verified with
  `node --test linux-features/agent-workspace/test.js` in
  `/home/avifenesh/projects/codex-desktop-linux`.
- `scripts/prod_readiness_smoke.sh` is now the broad local pre-release gate. It
  runs formatting, build, unit tests, clean/restricted/non-headless/no-display
  MCP smokes, the grocery browser workflow, integration smoke, diff whitespace
  checks, sibling Codex Desktop feature tests when present, and the GPUI viewer
  smoke when the host has `DISPLAY` plus the X11/ImageMagick inspection tools.
  `REQUIRE_GUI_SMOKE=1` and `REQUIRE_DESKTOP_SMOKE=1` make the optional gates
  mandatory in stricter release environments. `bash -n
  scripts/prod_readiness_smoke.sh` and `scripts/prod_readiness_smoke.sh` passed
  on this machine, including the optional GPUI and sibling Desktop checks.
- Detached host processes launched by the viewer are now reaped. This covers
  `workspace_open_viewer`, the X11 replacement relaunch, and host path open
  shortcuts, preventing a long-running MCP process from accumulating zombie
  viewer/helper children after they exit. Verified with
  `cargo test --locked viewer::tests::detached_child_spawn_is_reaped` and the
  full `scripts/prod_readiness_smoke.sh` gate.
- `scripts/viewer_desktop_matrix_probe.sh` now creates a JSON evidence row for
  the current Linux desktop/session under `target/viewer-desktop-matrix/`. It
  records OS/session/display facts, command availability, and the focused GPUI
  viewer smoke result. `scripts/prod_readiness_smoke.sh` uses this probe for GUI
  validation when the host has the required display/X11 tools, so release
  validation can accumulate comparable GNOME/KDE/X11/Wayland-like rows. On this
  machine, `scripts/prod_readiness_smoke.sh` produces a passing
  `ubuntu:GNOME / wayland / ubuntu` row for the X11/Xwayland implementation
  path; use `scripts/release_next_steps.py --json` and the newest
  `target/viewer-desktop-matrix/*.json` rather than copying timestamped paths
  from this handover.
- `scripts/real_grocery_dogfood_probe.js` is now the guarded entrypoint for
  logged-in grocery dogfood. The default plan-only mode verifies through
  `mcp_task_plan` that cart mutation approval is separate from checkout/order
  approval. Real browser mode requires `REAL_GROCERY_DOGFOOD=1`, a target URL,
  a disposable copied `GROCERY_USER_DATA_DIR`, and
  `GROCERY_PROFILE_IS_DISPOSABLE_COPY=1`; the script refuses
  `CHECKOUT_APPROVED=1` or `REAL_WORLD_ACTION_APPROVED=1`. Release-counting
  real browser evidence also requires `REAL_GROCERY_INTERACTION_MODE=cart-draft-approved`,
  `CART_MUTATION_APPROVED=1`, `FINAL_CART_REVIEWED=1`, and
  `GROCERY_CART_DRAFT_STEPS_JSON`; the release audit accepts only declared
  cart-draft input events and at least one explicit cart mutation step. The
  script records page readback as privacy-preserving metadata only, so release
  reports cannot carry raw logged-in page text. It now has
  `--print-cart-draft-steps-template`,
  `--validate-cart-draft-steps PATH`, and `--self-test` paths so step-file
  mistakes are caught before a live account run. It is wired into the broad
  `scripts/prod_readiness_smoke.sh` gate in plan-only mode; use the newest
  `target/real-grocery-dogfood/*.json` and the latest release audit rather than
  copying timestamped paths from this handover.
- `docs/prod-readiness-audit-2026-05-25.md` now maps the active goal to
  current evidence and remaining release gates. Treat it as the completion
  checklist before marking this work done.

## Likely Next Fixes In New Session

The first compact floating monitor slice is live against a real GUI app and can
start saved profiles without blocking the GPUI event loop. Refresh/screenshot
capture no longer runs on the UI thread, and the viewer can now cycle across
known running/stopped workspaces without growing into a full manager. Next
product work should move from "viewer watches" toward "viewer feels ready to
replace the Codex-for-Linux workspace page":

1. Continue focused quick actions only where they serve passive monitoring:
   Stop, Shot/window screenshot, Files/artifacts-folder, Evt/event-log,
   footer Log/active-app log, two-step running-workspace Rev, and two-step
   stopped-workspace Clean are now present.
2. Continue the repeatable visual QA path for both default and opt-in topmost
   backends: X11/Xwayland now has `scripts/gpui_viewer_smoke.sh`, while native
   Wayland layer-shell still needs compositor-level observation rather than X11
   tools. The release audit now rejects GNOME/Xwayland fallback notes and other
   negative observations for the native Wayland row.
3. Keep matching the original `../codex-desktop-linux/agent-workspaces-linux.js`
   visual language: quiet settings surfaces, small rounded borders, status
   pills, and restrained controls.

Reference GPUI files remain useful:

```sh
cd /home/avifenesh/projects/cidux
cargo check
```

Helpful files in `cidux`:

- `Cargo.toml` for GPUI dependency shape
- `src/main.rs` for GPUI app/window setup
- `src/demo.rs` for simple `Render` implementation
- `src/browser.rs` for screenshot/image decoding into `RenderImage`
- `src/ui/button.rs` for button/on-click patterns

## Important Product Decisions

Current recommended product direction:

- One serious UI surface: GPUI viewer owned by this MCP/runtime repo, but shaped
  as an on-demand gentle floating companion window rather than a full
  management app.
- Headless is an explicit MCP startup mode. A normal MCP server can still run
  without opening a window until an agent/user asks for `workspace_open_viewer`;
  `--headless` is the boundary for hosts that must never create host-visible UI.
- Codex app should eventually become a thin launcher/status bridge:
  - Open Viewer
  - Stop
  - Revoke (now implemented in the GPUI viewer as compact `Rev`)
  - small active status
- Do not revert or delete the current Codex-for-Linux workspace UI in
  `../codex-desktop-linux` yet. The user wants to keep that implementation until
  the GPUI replacement in this repo is genuinely ready.
- The GPUI visual direction should stay close to the already-started
  Codex-for-Linux workspace page rather than inventing a separate flashy look.
- The GPUI viewer should eventually own:
  - active/saved/stopped workspaces
  - screenshot/live refresh
  - profile/policy/apps
  - start/stop/revoke/delete
  - small links to logs/events/artifacts (Log/Evt/Files are now implemented)
  - stopped-workspace cleanup/delete semantics
  - permissions state (Iso footer now reports the MCP ceiling)

## Known Maturity Status Before Viewer Pivot

From `docs/permission-boundary-roadmap.md`:

- A is validated for current X11/bubblewrap runtime surface covered by smoke and
  real MCP dogfood.
- A remaining gap: host-localhost bridging for `local_only`; broad allowlists
  and proxy filtering are out of scope for now.
- B has a first Codex for Linux embedded workspace slice, but final direction is
  now likely to move serious UI into GPUI viewer.
- C is partially covered. Desktop QA, local-dev browser QA, browser-session,
  arbitrary startup apps, `.desktop` launcher parsing in Codex app picker, and
  MCP-locked permissions have first slices.

## Guardrails To Preserve

- Preserve user work. Do not revert unrelated changes.
- Use `apply_patch` for manual edits.
- Prefer small compileable increments.
- Do not run destructive git commands.
- Do not stage/commit unless the user explicitly asks for a checkpoint commit.
- If touching the Codex Desktop repo, preserve the current
  `agent-workspaces-linux.js` implementation unless the user explicitly asks to
  revert it after the GPUI replacement is ready.

## Useful Commands

Runtime repo:

```sh
cd /home/avifenesh/projects/agent-workspace-linux
git status --short --branch
git diff --stat
cargo fmt
cargo check
cargo test --locked
node scripts/mcp_permissions_smoke.js
scripts/integration_smoke.sh
scripts/gpui_viewer_smoke.sh
```

Current installed binary:

```sh
/home/avifenesh/.local/bin/agent-workspace-linux doctor
```

Reference GPUI project:

```sh
cd /home/avifenesh/projects/cidux
cargo check
```

Helpful files in `cidux`:

- `Cargo.toml` for GPUI dependency shape
- `src/main.rs` for GPUI app/window setup
- `src/demo.rs` for simple `Render` implementation
- `src/browser.rs` for screenshot/image decoding into `RenderImage`
- `src/ui/button.rs` for button/on-click patterns

## Current Risk

The current uncommitted viewer compiles, launches directly, launches through
MCP, and has been smoke-tested against a real hidden X11 workspace running
`xclock` plus a stopped companion workspace. The remaining risk is
product/runtime quality, not basic linkage:

- The preferred default viewer path now avoids always-on-top state, so the
  monitor is optional rather than forcibly topmost. The opt-in
  `--always-on-top` path uses `WindowKind::LayerShell` on `Layer::Overlay` for
  native Wayland, while GNOME Wayland and X11 desktops can still use explicit
  X11 above/sticky hints because Mutter does not expose `zwlr_layer_shell_v1`.
- On this GNOME Wayland session, the viewer now selects Xwayland and can be
  inspected with X11 tools. The default X11/Xwayland viewer no longer requests
  above/sticky state; it keeps skip-taskbar, skip-pager, and a utility
  window-type hint after mapping. The smoke script now fails if the default path
  advertises `_NET_WM_STATE_ABOVE` or `_NET_WM_STATE_STICKY`.
  Native layer-shell/top-state still needs compositor-level observation rather
  than X11 window tools when `--always-on-top` is explicitly used.
- The current interaction model keeps the viewer as a square-ish, screen-first
  monitor. Header title drags reposition the X11 popup, the screen/footer and
  buttons do not start drags, and the bottom-right corner grip is reserved for
  resize.
  Screen streaming is suppressed during pointer interaction. Periodic status
  refresh continues without screen capture by default, and opt-in screen
  streaming overwrites one `viewer-frame.png` instead of creating timestamped
  PNGs on every refresh.
- The MCP/CLI live observation path follows the same disk hygiene rule:
  `workspace observe --screenshot` reuses `observe-frame.png` unless the caller
  passes an explicit `--output` path.
- The current visual direction is a cooler Codex-style neutral grey palette,
  not the warmer brownish charcoal. Verbose status/detail text now lives in the
  compact chrome, leaving the live screenshot surface mostly unobstructed. The
  shared button helpers now use slightly rounder corners and subtle
  silvery-grey edges rather than flatter dark borders. Compact buttons now use
  native GPUI hover tooltips so the monitor can keep labels short while still
  explaining what each action does, including disabled/busy states. The viewer
  now loads and saves its size, popup position, and live-refresh preference from
  `XDG_CONFIG_HOME/agent-workspace-linux/viewer.json` or the `$HOME/.config`
  fallback, so a user-resized monitor reopens at that shape instead of always
  resetting to 420x420. The selected footer mode also persists, so users can
  reopen directly into Activity, Task, Isolation, or Apps context. Saved popup
  placement is clamped against the current display's visible bounds so stale
  monitor layouts do not reopen the viewer offscreen. The viewer smoke now
  seeds and verifies size, Task footer mode preference, and X11/Xwayland popup
  position. The viewer also sets GPUI's `.ZedSans` explicitly for a cleaner
  app-font feel. The header now leads with the actual workspace id,
  while the footer summarizes the latest agent action and active app/window
  instead of exposing capture metadata. A compact `index/total` workspace
  control appears when more than
  one workspace is known; switching clears stale frames and refreshes into the
  next running/stopped workspace. The footer now has compact Activity, Task,
  Isolation, and Apps modes, with action/error/cleanup states still taking
  priority. `Evt` opens the workspace `events.jsonl` file externally when it is
  present, falling back to the artifact folder if the log has not been written
  yet. The footer now shows `Log` when the active or most recent app has a real
  stdout/stderr log path, preferring stderr only when stdout is empty and stderr
  has content. `workspace_open_viewer` passes the active MCP permission ceiling
  into the viewer child process, and the Isolation footer reports whether the
  ceiling is open, configured/open, or narrowed by network, mounts, and app
  allowlists. Running workspaces now show a compact two-step `Rev` action that
  stops and removes runtime files, while stopped workspaces show a compact
  two-step `Clean` action that asks for a second click in the footer before
  calling the same stale-workspace cleanup backend as `workspace cleanup`. This was live-checked against
  `gpui-actions-smoke` running `xclock`, including the new Shot/Files/Evt
  buttons in the header and a direct
  `workspace screenshot-window --app xclock` proof that the active-window
  capture path produces a real window PNG. Synthetic host clicking through
  `xdotool` did not reach the popup on this GNOME Wayland session, and the
  desktop input portal click attempt timed out, so automated click-level proof
  is still a QA gap rather than a code-level pass.
- Codex Desktop no longer injects the older embedded conversation workspace
  screen. The sibling `codex-desktop-linux` feature keeps the bridge/settings
  controls and native GPUI `Open Viewer` action, but removes the webview
  `conversation-view` patch so the Codex conversation does not compete with the
  real floating GPUI monitor. The Desktop feature test now keeps a stale
  `local-conversation-thread` fixture and asserts the patcher strips only the
  removed conversation monitor runtime, so this removal is guarded rather than
  just a one-time cleanup.
- Follow-up permission pass: viewer profile starts now validate the inherited
  MCP permission state before opening the workspace. Clean/default viewer usage
  keeps the empty open state and does not impose an extra ceiling; explicit
  `--permissions`/MCP ceilings still cap profile policy and startup apps.
- Follow-up MCP instruction pass: `McpPermissionState::from_ceiling(None, ...)`
  now drops any accidental ceiling object, so only an explicit source path can
  activate MCP-level enforcement. The initialize instructions and
  `mcp_permissions` tool description now distinguish `configured=true`
  populated ceilings, explicit configured/open ceilings, and `configured=false`
  clean harness-owned sessions. Both MCP smoke scripts assert those instruction
  surfaces, plus the `confirmed_user_request=true` reactivation hint. The
  action catalog now carries the same distinction, so clean/default MCP clients
  see advisory action classification rather than stale "spawn-time ceiling"
  wording.
- Viewer live-control tooltip now names the MCP-side reactivation contract:
  `read_only`/`paused` block mutating actions, and MCP clients need
  `confirmed_user_request=true` to switch back to active. The viewer itself
  still writes local user-control changes directly through the shared control
  file.
- Planner and mutation-denial live-control text now names the same
  reactivation contract, including `mode=active` and
  `confirmed_user_request=true`, so blocked tool calls, session briefs, task
  plan assumptions, and approval checkpoints all teach the same recovery path.
- The viewer intentionally avoids becoming a full app. Add future controls only
  if they fit the small monitor shape or open heavier details externally.
