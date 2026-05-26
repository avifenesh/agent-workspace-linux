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
- expose screenshots, input, window listing, and a small native host-visible viewer

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
should use sandbox loopback but not the internet; with bubblewrap it is enforced
as loopback-only inside the sandbox. Optional `allow_hosts` entries can label
expected localhost targets, but they are not required. Host-loopback services
are not bridged into that namespace yet, so services needed by the app should
be started inside the workspace or the profile should use `inherit_host`.
Profiles can also set `require_enforced_policy=true` to fail closed: if any
requested mount or network policy is not enforced by the current runtime, starts
and launches are rejected even when the caller passes the unenforced-policy
acknowledgement.

For the current bubblewrap runtime, profile mount sources must use absolute host
paths, and mount destinations must be non-overlapping absolute paths under
`/workspace/`.

The MCP server can run in open host-controlled mode, or with an optional
spawn-time permission ceiling loaded from JSON. In open mode, it does not
secretly reduce a Codex session that the user already granted full access to:
after the one hidden-workspace acknowledgement, workspace-local launch/input
actions are treated as scoped to that approved environment. A configured
ceiling is different: it is an immutable narrowing layer for that MCP process
and even a full-access client cannot broaden it. The richer human approval
boundary in Codex for Linux is still being dogfooded. See
[Permission Boundary Roadmap](docs/permission-boundary-roadmap.md) for the
authority model and validation gates, and
[Dogfood Validation](docs/dogfood-validation.md) for the current evidence log.
The current requirement-by-requirement readiness state is tracked in
[Prod Readiness Audit](docs/prod-readiness-audit-2026-05-25.md).

## Commands

```bash
cargo run -- doctor
cargo run -- guardrails
cargo run -- permissions template local --allow-host localhost:3000 --mount "$PWD:/workspace/project:read_write" --app sh
cargo run -- permissions validate --json ./permissions.json
cargo run -- mcp --permissions ./permissions.json
cargo run -- --permissions ./permissions.json profile validate --json ./profile.json
cargo run -- profile path
cargo run -- profile list
cargo run -- profile template project-dev --host-path "$PWD"
cargo run -- profile template restricted-chrome --browser-path /usr/bin/google-chrome
cargo run -- profile template browser-session --browser-path /usr/bin/google-chrome --user-data-dir "$HOME/.config/google-chrome"
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
cargo run -- viewer
cargo run -- viewer --id default
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

Building the GPUI viewer from source also needs the development files used by
GPUI's X11 keyboard path:

```bash
sudo apt install pkg-config libxkbcommon-x11-dev
```

`doctor` is implemented first so missing runtime dependencies are visible before
the workspace runtime grows. It reports workspace readiness separately from
host-visible viewer readiness: `ready_for_x11_workspace` covers the hidden X11
workspace runtime, while `ready_for_host_viewer` checks whether the current MCP
or CLI environment has a host display through `DISPLAY` or `WAYLAND_DISPLAY`.
The `viewer.source_build_xkbcommon_x11` check points source builders at the
`libxkbcommon-x11-dev` package when the GPUI linker metadata is missing, and
`viewer.host_opener` reports whether `Files`, `Evt`, and `Log` buttons can open
paths through `xdg-open` or `gio`. Doctor also reports optional policy backend
candidates such as bubblewrap, firejail, unshare, and slirp4netns without
treating them as active enforcement. The workspace commands use a small local
Unix socket daemon:

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
Linux can own the approval UI and honor the user's session choice, including
full-access sessions that should not be prompted repeatedly after approving the
hidden workspace. For MCP hosts or auto-loop agents that need fixed permissions
at server spawn, pass a ceiling file during install:

```bash
agent-workspace-linux permissions template local \
  --allow-host localhost:3000 \
  --mount "$PWD:/workspace/project:read_write" \
  --app sh \
  > /home/YOU/.config/agent-workspace-linux/permissions.json
agent-workspace-linux permissions validate --json /home/YOU/.config/agent-workspace-linux/permissions.json
./install.sh --permissions /home/YOU/.config/agent-workspace-linux/permissions.json
```

That writes:

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

If `--permissions` is omitted, the MCP does not impose its own permission
ceiling; it reports that the host/client harness owns the session boundary and
only classifies actions for the agent. In a configured permissions file,
omitted or empty dimensions are open. Populated dimensions are hard ceilings for
that MCP process: profiles and launches may narrow access, but they cannot
broaden network mode, mount paths/access, or launch programs. Call
`mcp_permissions` after connecting to see whether a ceiling is configured,
restricted, or open. App allowlists match the launched program only; allowing
shells, package managers, or browsers delegates whatever those programs can do
inside the workspace policy.
Use `permissions template open|closed|local` to generate a starter ceiling, and
`permissions validate --json PATH` to parse and check a file without starting an
MCP server.

The same ceiling can be applied to standalone CLI operations by placing
`--permissions PATH` before the command, for example
`agent-workspace-linux --permissions permissions.json workspace open-profile ...`.
This lets Codex for Linux reuse the MCP server's configured permission file when
it needs to call the local CLI bridge.

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

Use `["mcp", "--headless"]` when the MCP host must never open a host-visible
viewer window. Without `--headless`, agents can still run fully headless, but
`workspace_open_viewer` remains available as an explicit live-monitor action.

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
checks both restricted and clean/default MCP JSON-RPC paths, checks pre-daemon
approval previews, starts a real local-only workspace, verifies loopback-only
and disabled-network enforcement, checks read-write/read-only mount
enforcement, checks session tracking, exercises a real X11 window with window
listing, screenshot, clipboard, keyboard input, app wait, and artifact
inspection, runs a synthetic browser-session profile through visible startup,
observation, browser-data mount write-through, and stop when Chrome/Chromium is
available, drives a local grocery browser workflow that drafts cart contents
through workspace-local input while keeping checkout locked, verifies that a
workspace app can trigger workspace shutdown even if
its stop client disappears before the response, and stops the workspace before
exiting. The clean MCP smoke starts `mcp --headless` with no
`--permissions` file and proves that `mcp_permissions` reports
`configured=false`, the action catalog stays advisory, and app-QA/browser plans
do not invent permission blockers. The non-headless MCP viewer smoke starts
plain `mcp` with no `--headless` flag and verifies that host-visible viewer
steps are offered only as explicit open-world checkpoints. A companion
no-host-display smoke unsets `DISPLAY` and `WAYLAND_DISPLAY` to prove a normal
non-headless MCP still runs workspace/planning flows, but suppresses viewer
recommendations and refuses `workspace_open_viewer` with a doctor-backed host
display message.

The guarded real-account grocery probe is separate from the synthetic grocery
browser smoke:

```bash
scripts/real_grocery_dogfood_probe.js
```

By default it runs in plan-only mode and proves that `mcp_task_plan` keeps cart
drafting approval separate from checkout/order/account approval. To open a real
grocery site, set `REAL_GROCERY_DOGFOOD=1`, `GROCERY_TARGET_URL`,
`GROCERY_USER_DATA_DIR`, and `GROCERY_PROFILE_IS_DISPOSABLE_COPY=1`.
`GROCERY_TARGET_URL` must be an HTTPS, non-local grocery site rather than a
localhost, reserved, or private-network URL. Prepare that disposable copy with:

```bash
scripts/prepare_grocery_profile_copy.js --source "$REAL_BROWSER_PROFILE" --dest "$GROCERY_PROFILE_COPY_DIR"
```

The copy helper excludes browser locks, sockets, caches, crash dumps, and
extension/web-app payloads, then writes
`.agent-workspace-grocery-profile-copy.json` into the destination. The guarded
wrapper defaults that destination outside the repo `target/` tree under
`$XDG_RUNTIME_DIR/agent-workspace-linux/grocery-profile-copy`, or `/tmp` when
`XDG_RUNTIME_DIR` is unavailable. The real-browser probe requires that manifest
before it opens the site. It refuses to run when `CHECKOUT_APPROVED=1` or
`REAL_WORLD_ACTION_APPROVED=1`; real checkout/order/account changes are outside
this dogfood gate.

The guarded wrapper prepares the disposable copy, validates the cart-draft step
file, and refuses checkout/order/account authority before any live browser run.
During the live run the probe records a workspace-event baseline after browser
launch, requests an event tail sized from the declared step file, and the
release gate rejects reports that do not show enough allowed input events to
cover the declared cart-draft input steps. It also stops and cleans the
workspace runtime by default. The report must also include
`workspace_browser_targets` evidence showing that the real grocery page was
discovered through the workspace Chrome/Chromium app's loopback DevTools
endpoint, plus MCP-owned browser snapshot/navigation evidence where the task
requires page readback or URL changes, not the user's host Chrome bridge. The
release report stores only page URL/title plus text length/truncation metadata;
raw logged-in page text, excerpts, links, and headings are omitted and rejected
by the importer/audit. Set `REAL_GROCERY_PRESERVE_WORKSPACE=1` only for
debugging, because preserved workspace runtimes are not release eligible.
Generate and validate a starter cart-draft step file through the same wrapper:

```bash
scripts/collect_real_grocery_evidence.sh --print-cart-draft-steps-template > target/cart-draft-steps.json
scripts/collect_real_grocery_evidence.sh --validate-cart-draft-steps target/cart-draft-steps.json
```

Use `--preflight-only` first; it does not open the grocery site:

```bash
REAL_BROWSER_PROFILE="$REAL_BROWSER_PROFILE" \
REAL_GROCERY_URL="$REAL_GROCERY_URL" \
REAL_GROCERY_CART_DRAFT_STEPS="$REAL_GROCERY_CART_DRAFT_STEPS" \
scripts/collect_real_grocery_evidence.sh --preflight-only
```

If the grocery login lives in a named Chrome/Chromium profile inside the copied
user-data directory, also set `REAL_GROCERY_PROFILE_DIRECTORY`, for example
`REAL_GROCERY_PROFILE_DIRECTORY="Profile 1"`. The wrapper validates that this
directory exists in the source and prepared copy, passes it to Chromium as
`--profile-directory=...`, and records it in the preflight/live evidence.

The preflight writes a durable JSON report under
`target/real-grocery-preflight/` by default. Set `REAL_GROCERY_PREFLIGHT_DIR`
when collecting evidence in a copied source bundle or another machine. The
final review bundle includes the latest preflight report path, so the human
review can see exactly which real URL, disposable profile copy, browser
executable, step file, and checkout-refusal state were validated before the
live browser opened.

Then run the same wrapper with the explicit live-browser flag:

```bash
REAL_BROWSER_PROFILE="$REAL_BROWSER_PROFILE" \
REAL_GROCERY_URL="$REAL_GROCERY_URL" \
REAL_GROCERY_CART_DRAFT_STEPS="$REAL_GROCERY_CART_DRAFT_STEPS" \
scripts/collect_real_grocery_evidence.sh --run-real-browser
```

Set `GROCERY_PROFILE_COPY_DIR` to choose the disposable-copy destination, and
set `REPLACE_GROCERY_PROFILE_COPY=1` or pass `--replace-profile-copy` when the
copy should be recreated.

The local app-QA dogfood collector records a release artifact for a real GUI app
inside a hidden workspace:

```bash
scripts/app_qa_dogfood_smoke.sh
```

It launches a disposable `xmessage` target, captures a window screenshot,
observes the workspace, verifies app log and event artifacts, stops the
workspace, and writes `target/app-qa-dogfood/*.json`. The release audit rejects
stale/source-mismatched reports and reports that do not prove local GUI launch,
screenshot, logs, events, non-destructive QA scope, and clean stop.

Run the focused GPUI viewer smoke after changing the floating monitor:

```bash
scripts/gpui_viewer_smoke.sh
```

The viewer smoke builds the binary, starts a disposable X11 workspace with
`xclock`, creates a stopped companion workspace so the workspace switcher
renders, proves the workspace window screenshot path, opens the GPUI viewer
through X11/Xwayland with a seeded `viewer.json` compact size, opt-in screen
stream, and `task` footer preference, captures the viewer window, and checks
that the capture is nonblank and sized from that preference. The same smoke
seeds and verifies popup position so the floating monitor does not quietly fall
back to the default top-right placement.

To record a release-matrix row for the current desktop/session, run:

```bash
scripts/viewer_desktop_matrix_probe.sh
```

The probe writes a JSON report under `target/viewer-desktop-matrix/` with OS,
session, display, command-availability, and viewer-smoke results. It runs the
same focused GPUI viewer smoke when the host has a display and the required X11
inspection tools. Release rows also include display-server attestation: local
Wayland/X11 sockets and their display-server processes are checked with `lsof`,
remote X forwarding is rejected, and known nested/headless display servers such
as Xvfb, xpra, Xephyr, and headless Weston do not count as real desktop
coverage. Use
`REQUIRE_VIEWER_SMOKE=1` in release validation when a skipped visual smoke
should fail the gate. For native Wayland compositor coverage, set both
`NATIVE_WAYLAND_LAYER_SHELL_OBSERVED=1` and
`NATIVE_WAYLAND_LAYER_SHELL_NOTES="..."`; the probe rejects a bare observation
flag without notes, X11 sessions, GNOME/Xwayland fallback sessions, forced
X11/Xwayland viewer backends, and notes that lack a positive
layer-shell/top-layer claim. The release audit only counts the observation from
a Wayland session when the notes make that positive claim. GNOME/Xwayland
fallback observations and notes that say the viewer was not layer-shell do not
satisfy the native Wayland release row.

For a broader local pre-release gate, run:

```bash
scripts/prod_readiness_smoke.sh
```

This gate runs formatting, build, unit tests, MCP clean/restricted/viewer
smokes, the local app-QA dogfood collector, the visible GitHub Explore dogfood
collector, the grocery browser workflow, the integration smoke,
`git diff --check`, and, when available, the sibling Codex Desktop
agent-workspace feature tests. It also runs the GPUI viewer smoke when the host
has a display and the required X11 inspection tools, via the desktop matrix
probe above, then writes a release-gate audit under
`target/release-gate-audit/`, a final human-review bundle under
`target/final-review-bundle/`, a smoke report under
`target/prod-readiness-smoke/`, and a requirement-level objective audit under
`target/objective-completion-audit/`. The final bundle includes the latest
missing gates, release-audit/current-source and review-scope consistency, a
human-review marker template, generated runtime/Desktop review diffs, and
copy-pasteable next evidence commands for KDE/X11/native Wayland viewer rows,
GitHub Explore dogfood, and strict release validation. Set
`REQUIRE_GUI_SMOKE=1`,
`REQUIRE_DESKTOP_SMOKE=1`, or `REQUIRE_RELEASE_GATES=1` when optional GUI,
Desktop, or external/manual release gates must be mandatory in a release
environment.

When a real GPUI monitor is already open for interactive dogfood, use the
non-disturbing validation mode:

```bash
AGENT_WORKSPACE_NO_NEW_VIEWER=1 scripts/prod_readiness_smoke.sh
```

That mode still runs the source, permission, browser, app-QA, GitHub Explore,
Desktop, release-audit, bundle, and objective-audit checks, but skips
viewer-spawning lifecycle/visual-smoke steps and records
`visible_viewer_smoke.mode=metadata-only-no-new-viewer` in the smoke report.
Unset the flag for strict release validation where temporary GPUI viewer windows
are expected evidence.

Repeated smoke runs prune timestamped local evidence reports under
`target/` after the objective audit, keeping the latest 25 grouped runs per
evidence directory by default. Set `AGENT_WORKSPACE_REPORT_RETENTION=N` to
choose another keep count, or `AGENT_WORKSPACE_REPORT_RETENTION=0` to skip
pruning. The retention helper protects rare release-grade rows even when they
fall outside the normal keep window: KDE/Plasma viewer rows, X11 viewer rows,
release-positive native Wayland compositor observation rows, and passed
GitHub Explore dogfood reports. GNOME/Xwayland fallback notes are pruned
like ordinary viewer rows. You can also run
`scripts/prune_evidence_reports.py --dry-run` to preview the bounded cleanup
without deleting anything.

To inspect the release-only gaps directly without rerunning the whole smoke
suite:

```bash
scripts/release_gate_audit.py
```

The audit scans `target/viewer-desktop-matrix/`,
`target/app-qa-dogfood/`, and `target/github-explore-dogfood/` and reports
whether the current evidence proves GNOME/KDE plus X11/Wayland viewer coverage,
local GUI app-QA dogfood, visible GitHub Explore repository-discovery dogfood
with `workspace_open_viewer` launch metadata, and final human diff
review. Viewer evidence must carry release-eligible session and display
attestation, so contradictory session claims, remote displays, and
nested/headless host displays are rejected by default. Evidence must be fresh
by default:
`--max-evidence-age-days` defaults to 14, and `0` disables that freshness check.
Evidence must also match the current combined source identity. That identity
hashes the runtime source (`Cargo.toml`, `Cargo.lock`, `src/`, and `scripts/`)
plus the sibling Codex Desktop integration source
(`../codex-desktop-linux/linux-features/agent-workspace` and
`../codex-desktop-linux/agent-workspaces-linux.js`, or
`CODEX_DESKTOP_LINUX_REPO` when set). Use `--no-source-identity-check` only
when intentionally auditing imported evidence outside the current checkout. Use
`--require-all` to make missing release-only evidence fail. The human review
gate is only considered proven when
`target/release-gate-human-review.json` exists with schema
`agent-workspace-linux.human_final_diff_review.v1`, status `reviewed`, the
current combined source identity, the current review-scope identity, and
`review_artifacts` entries for the generated runtime and sibling Desktop review
diffs with matching SHA-256 hashes. The reviewer and notes fields must be
meaningful, non-placeholder text. In a dirty worktree, the review-scope
identity hashes each repo's status, staged and unstaged diffs, and non-ignored
untracked file contents. In a clean worktree, it hashes the current `HEAD`
commit content. This lets the marker bind either to the reviewed dirty diff
before staging or to the reviewed final commit before shipping, and prevents
reuse after local diff, commit drift, or review-artifact drift. Do not create
that marker before actual human review; after review, set
`HUMAN_REVIEW_NOTES` to specific scope/acceptance notes and run
`scripts/create_human_review_marker.py --reviewer "$USER" --confirm-reviewed --notes "$HUMAN_REVIEW_NOTES"`.
That binds the marker to freshly generated review artifacts and current
source/review-scope hashes.
`scripts/release_gate_audit.py --self-test` verifies the audit logic against
synthetic pending and complete evidence fixtures, including stale complete
evidence and mismatched review scope that must stay pending; the broad smoke
runs that self-test before using the current machine's real evidence. In strict
release mode, `REQUIRE_RELEASE_GATES=1` also passes
`--require-clean-source`, so those runtime paths and the sibling Desktop
feature paths must have no git status entries.

To check the full thread objective as a requirement map rather than a single
release gate, run:

```bash
scripts/objective_completion_audit.py
```

The objective audit reads the latest prod-readiness smoke report, release-gate
audit, and final review bundle, verifies that each matches the current combined
source and review-scope identity, and reports which objective requirements are
still pending. It exits successfully while requirements are pending unless
`--require-complete` is passed; strict smoke mode uses that flag only after the
release gates are required.

When evidence is collected on another desktop or browser environment, copy the
JSON reports back to this checkout and import them before rerunning the audit:

```bash
scripts/export_release_evidence_bundle.py
scripts/import_release_evidence.py /path/to/copied/report-or-directory
scripts/release_gate_audit.py
```

Use `scripts/export_release_evidence_bundle.py` before collecting viewer rows,
app-QA evidence, GitHub Explore evidence, or final human review on another desktop
or machine. It writes a tarball under `target/release-evidence-source-bundle/`
containing the runtime source, sibling Desktop feature source,
source/review-scope manifest, `collect-viewer-evidence.sh`,
`collect-app-qa-evidence.sh`, `collect-github-explore-evidence.sh`, and
`create-human-review-marker.sh`. Extract that bundle on the target desktop, run
`./collect-viewer-evidence.sh` for viewer rows, `./collect-app-qa-evidence.sh`
for app-QA evidence, `./collect-github-explore-evidence.sh` for GitHub Explore
dogfood, or create the human review marker after review:

```bash
HUMAN_REVIEW_NOTES="<specific scope and acceptance notes>"
./create-human-review-marker.sh --reviewer "$USER" --confirm-reviewed --notes "$HUMAN_REVIEW_NOTES"
```

Then copy the generated JSON report or human-review marker plus
review artifacts back to the release machine. The importer accepts
viewer-matrix, app-QA, GitHub Explore, and human-review marker reports. For copied
human-review markers, copy the marker JSON together with the generated
runtime/Desktop review artifact files; the importer verifies the artifact hashes
and rewrites marker paths to local `target/final-review-bundle/` files before
writing `target/release-gate-human-review.json`. By default it
rejects source-hash mismatches, skipped/failed viewer rows, unsafe app-QA rows,
GitHub Explore reports without `workspace_open_viewer` metadata, reports that
use host Chrome/Codex app MCP/Computer Use/curl/Playwright as evidence, missing
workspace cleanup evidence, human-review marker review-scope mismatches, and
missing reviewed artifact bytes. Viewer, app-QA, and GitHub Explore reports
must also include `evidence_boundary` showing they were collected by the repo-owned
`agent-workspace-linux` runtime and did not use the Codex app MCP, Computer Use
MCP, Playwright MCP, or Codex Desktop bridge as release evidence.
Each executed cart-draft step must also report a successful result.
Use override flags only for diagnostics because the release audit still will
not count mismatched or nonpassing evidence.
`scripts/import_release_evidence.py --self-test` verifies those default
rejections, and `scripts/export_release_evidence_bundle.py --self-test` verifies
the portable source bundle shape.

For the final human diff review, inspect the newest files under:

```bash
target/final-review-bundle/
```

The bundle records combined source identity, review-scope identity, runtime and
sibling Desktop dirty scope, latest evidence paths, release-audit/current-source
and review-scope consistency, pending release gates, next evidence commands, a
review checklist, generated runtime/Desktop review diffs, and the JSON marker
template for
`target/release-gate-human-review.json`. The template includes the
`review_artifacts` hashes that the audit later verifies. It does not create
that marker. After actual human review, run
`scripts/create_human_review_marker.py --reviewer "$USER" --confirm-reviewed --notes "$HUMAN_REVIEW_NOTES"`
with specific non-placeholder notes to regenerate
the runtime/Desktop review artifacts and write the marker.
Because docs and non-ignored untracked files are part of the review scope,
rerun `scripts/release_gate_audit.py` and `scripts/final_review_bundle.py`
after final doc edits and before creating the marker.
`scripts/final_review_bundle.py --self-test` verifies the command recipes and
stale-audit detection logic, and
`scripts/create_human_review_marker.py --self-test` verifies the marker it
generates is accepted by the audit.

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
  Local-only network profiles may leave `allow_hosts` empty. If entries are
  provided, they are validated as localhost or loopback labels such as
  `localhost:3000` or `127.0.0.1:5173`. With bubblewrap, local-only profiles are
  enforced without `--ack-unenforced-policy` by giving launched apps a network
  namespace where only sandbox loopback works. Host-loopback bridging is still a
  limitation and is reported in `applied_policy.enforcement.network`.
  If the saved profile sets `require_enforced_policy=true`, the runtime refuses
  to start or launch with unenforced policy instead of accepting that
  acknowledgement.
  The current product network model is intentionally limited to closed
  (`disabled`), local (`local_only`), and open (`inherit_host`). Legacy or
  advanced profiles may still contain `network.mode=allowlist`; those values are
  saved as declared intent only, always require `--ack-unenforced-policy`, and
  do not promise host filtering.
  It then chooses a free X11 display, creates an `xauth` file, starts `Xvfb`,
  starts a lightweight window manager, and binds a control socket under the
  runtime base. `XDG_RUNTIME_DIR` is preferred; if a desktop app or MCP launcher
  omits it, the runtime falls back to `/run/user/<uid>` when that user-owned
  directory exists before using `/tmp/agent-workspace-linux-$USER`. With
  `--profile`, profile width/height are applied unless explicit flags override
  them, and the profile's mounts/network/setup intent is snapshotted into
  status.
- `profile validate --json PATH` parses and validates a shared profile file
  without saving it, and returns the same policy, warning, and acknowledgement
  preflight shape used by `profile check`.
- `profile template project-dev` creates a starter project QA profile. It
  mounts the selected project read-write and, when detected, mounts Cargo's
  `bin` shims plus rustup toolchains read-only. It deliberately uses a
  throwaway `CARGO_HOME` and a small system `PATH` inside the workspace instead
  of mounting Cargo credentials, registry/cache state, or volatile host shell
  paths. `profile template restricted-chrome` creates a browser starter profile
  with
  `network.mode=disabled`, `require_enforced_policy=true`, an isolated Chrome
  user-data dir, and an explicit `--no-sandbox` startup command. `profile
  template browser-session --user-data-dir PATH` creates an authenticated-browser
  starter that mounts the selected browser data directory read-write at
  `/workspace/browser-user-data`, inherits host networking, and starts the
  browser with that mounted profile. The browser templates keep `--no-sandbox`
  visible in generated JSON because Chrome can abort before opening a window
  inside bubblewrap namespaces. They also set
  `--remote-debugging-address=127.0.0.1` with
  `--remote-debugging-port=0`, so browser agents can attach to the workspace
  Chrome DevTools endpoint from the generated `DevToolsActivePort` file instead
  of controlling the user's host Chrome. Use browser-session only for
  explicitly user-approved browser data, and close the host browser or point it
  at a copied profile to avoid profile lock/corruption.
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
  app in the top-level `apps` field without embedding the full historical app
  list in the nested status. Use `workspace status`, `workspace observe`, or
  `workspace apps` when the full app list is needed. Completed apps report both a human
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
  optionally a root screenshot in one IPC call. Repeated live observation reuses
  `observe-frame.png` by default so UI polling does not accumulate timestamped
  screenshots; pass `--output` when you want a durable observe artifact.
  Screenshot records include path, dimensions, PNG byte size, and capture
  timestamp. `observe --events`,
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
  not raw text. Browser snapshot, navigation, and search-result events are also
  metadata-only: raw DOM text, headings, links, and result-card excerpts stay in
  the direct browser tool response and are not persisted into the event log.
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
`mcp_action_catalog`, `mcp_session_brief`, `mcp_control_state`,
`mcp_control_update`, `workspace_doctor`, `workspace_guardrails`,
`profile_path`, `profile_list`, `profile_get`,
`profile_check`, `profile_validate`, `profile_template`, `profile_put`,
`profile_import`, `profile_export`, `profile_delete`, `workspace_start`,
`workspace_open_profile`, `workspace_list`, `workspace_open_viewer`,
`workspace_list_viewers`, `workspace_close_viewer`, `workspace_cleanup_stale`,
`workspace_status`, `workspace_manifest`, `workspace_artifacts`,
`workspace_ipc_info`, `workspace_env`, `workspace_launch_app`, `workspace_run_app`,
`workspace_launch_profile_apps`, `workspace_list_apps`, `workspace_browser_targets`,
`workspace_browser_snapshot`, `workspace_browser_navigate`, `workspace_list_windows`,
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

`mcp_action_catalog` returns a machine-readable action taxonomy for the whole
MCP surface, including whether each tool is read-only, mutating, destructive,
idempotent, host-visible/open-world, and how live `active` / `read_only` /
`paused` control treats it. It also includes advisory `parameter_notes` for
arguments that change risk, such as `dry_run`, `replace`, `output_path`, and
`kill_on_timeout`; these notes guide approval UX but do not create an extra
permission ceiling when MCP permissions are empty. Agents should use this
catalog, along with tool annotations, when no permission ceiling is configured
or when deciding what the user likely needs to approve.

`mcp_session_brief` is a read-only agent UX summary for hosts that need a
single orientation call. It returns the permission ceiling, live control mode,
runtime readiness, known profile/workspace counts, compact activity for
running/stopped workspaces and their recent apps, inferred task intent from
profile/app activity, headless state, and suggested next MCP actions with action
type, idempotency, and compact approval/open-world checkpoints. Its
recommendations include read-only
`mcp_task_plan` entries for common app-QA,
browser/shopping/grocery, observation, and cleanup situations, so agents can
derive a safer workflow before jumping into mutating tools. Each recommendation
also carries an `approval_summary`, and the brief has a top-level
`approval_summary` across its already-prioritized recommendations, so hosts can
show the first required user boundary before calling a separate planner.
`mcp_task_plan` is
the intent-specific companion for those workflows. It suggests safe dry-run
previews, profile templates, approval points, required user inputs, step
dependencies, and live-control constraints before the agent calls mutating
tools. The plan also includes structured `approval_checkpoints` for host UI and
agent UX: required input, dry-run approval surfaces, profile writes, hidden
workspace starts, live-control blockers, host-visible UI, permission-ceiling
blockers, destructive actions, and separate real-world approvals for checkout,
purchases, order submission, or account changes. Live-control checkpoints carry
the exact reactivation input (`confirmed_user_request=true` for
`mcp_control_update mode=active`) so host UI does not need to scrape text to
resume mutating actions. It also returns
`task_context`, a compact structured summary of the normalized task kind,
target workspace, provided inputs, missing inputs, safety boundaries, action
boundaries, and the approval kinds present in the plan, so agents and host UI do
not need to scrape step prose. `approval_summary` adds the UI-ready rollup:
blocking checkpoint count, approval-required count, all approval kinds, and the
single next boundary a host should render first, such as missing input, a
permission ceiling, live-control reactivation, hidden-workspace approval, or a
real-world approval. The plan also exposes `host_viewer_ready`,
`viewer_available`, and `viewer_unavailable_reason`, so a host can distinguish
an explicit `mcp --headless` session from a non-headless service/no-display
session where the floating viewer is withheld until `workspace_doctor` reports
host display readiness. Browser/shopping action boundaries separate
observation, navigation/search, item comparison, cart mutation, and
checkout/account changes; cart mutation and real-world checkout/account actions
are explicit approval classes rather than inferred from prose. Grocery plans
also accept explicit approval-state inputs such as `cart_mutation_approved`,
`final_cart_reviewed`, and `real_world_action_approved`; action boundaries
report `approved` and `missing_approvals` so a host can allow cart drafting
without silently allowing checkout. App-QA action
boundaries now separate read-only observation, hidden workspace start/attach,
post-start evidence collection, workspace-local input, and mounted project file
writes. File writes are a distinct `project_file_write` approval class so host
UI can avoid treating code edits as a normal QA click path. App-QA plans
generated from natural phrases like "test the local UI", "verify the frontend",
or "run smoke checks" normalize to the app-QA planner and carry through
reviewed profile save, approved profile start, and post-start observation.
Browser/shopping plans carry the sequence through
approved profile start and post-start observation, and only suggest the floating
viewer after the plan has a runnable browser workspace step. They also name the
workspace-owned Chrome DevTools path as the preferred browser-control surface
when a launched workspace browser exposes `DevToolsActivePort`; `workspace_browser_targets`
derives that endpoint from the running workspace app's `--user-data-dir`, maps
`/workspace/browser-user-data` back to the approved host profile copy when a
mount profile is used, and returns the workspace Chrome page targets.
`workspace_browser_snapshot` reads title, URL, visible text, headings, and links
through that same workspace-owned target, `workspace_browser_search_results`
extracts structured product/search cards when the page is a results list, can
filter GPU-like results with `min_vram_gb`, and
`workspace_browser_navigate` changes the workspace browser page while logging a
`browser_navigate` event. Browser tool responses warn when the workspace event
log cannot be updated, which helps catch stale daemons or IPC/schema skew. This
keeps shopping/browser automation inside the isolated workspace rather than
attaching to the user's normal Chrome bridge or asking agents to invent their
own CDP/Playwright/curl side path. For
shopping/grocery intents, including natural phrases like "buy", "purchase",
"add to cart", "checkout", "order", or "delivery", `mcp_task_plan` also asks
for task inputs such as
`target_url`, `shopping_list`, `fulfillment`, `substitution_policy`, and
`budget`; these are required-input prompts, not MCP permission blockers, so a
clean/default MCP still respects the host/client session boundary. Cleanup
plans include the destructive follow-up and verification step after the dry-run
approval surface. Fresh-start and already-running app-QA/browser plans collect
read-only evidence before input: recent workspace events are read first, while
app logs and focused window screenshots wait until observation provides a stable
`app_id` or `active_window.id`. When the requested workspace is already running,
the plan continues from that live workspace instead of starting another profile.
Browser/shopping plans still surface the separate real-world approval boundary
before purchases, checkout, order submission, or account changes. Generated
profile steps are preflighted against the active MCP
permission ceiling, and saved-profile plan steps are checked against the same
ceiling, so agents can see permission blockers before attempting the next tool
call.

`mcp_control_state` reports the live MCP mode shared by the server and GPUI
viewer: `active`, `read_only`, or `paused`. `mcp_control_update` changes that
mode at runtime, and records `updated_by`, `updated_at_unix`, and an optional
reason in `mcp_session_brief`. If an MCP client tries to switch from
`read_only` or `paused` back to `active`, it must set
`confirmed_user_request=true`, making reactivation an explicit user/control-UI
approval rather than a casual agent toggle. `read_only` and `paused` block
mutating agent actions at the MCP boundary, including profile writes, workspace
starts, app launches, workspace-local input, window manipulation, cleanup, and
app termination. They still allow read-only inspection plus the safety-oriented
workspace stop path, so the user can observe or shut down work without letting
the agent continue to act. Dry-run approval previews remain callable while live
control is `read_only` or `paused`; host-output writes such as `profile_export`
with `output_path` are blocked until control returns to `active`.

`workspace_open_viewer` opens the viewer as a small host-visible GPUI monitor
intended for passive monitoring while the user keeps working elsewhere. The MCP
does not have to show this window; if the server is started with `--headless`,
the tool refuses instead of launching any host-visible UI. By default it does
not request always-on-top state; optional Wayland layer-shell or X11/Xwayland
above/sticky behavior is used only when `always_on_top` is explicitly requested.
Repeated opens for the same workspace reuse any registered live viewer for that
workspace instead of creating duplicate detached windows, even if a later request
asks for a different topmost mode.
Viewers launched through `workspace_open_viewer` are bound to the selected
workspace and exit once that workspace runtime is removed, so a finished run
does not leave an orphan GPUI window behind. The viewer runtime registry keeps
the launch metadata for bound MCP monitors and direct/free viewer launches, so a
reused manual viewer reports whether it is persistent or target-bound. Direct
`agent-workspace-linux viewer` launches stay persistent; when no workspace is
running, the direct viewer can cycle saved profiles and start the selected one
from the monitor.
Use `agent-workspace-linux viewer list` or the MCP `workspace_list_viewers` tool
to inspect registered GPUI monitors without depending on compositor window
introspection. Use `agent-workspace-linux viewer close --id ID` or
`workspace_close_viewer` to send `SIGTERM` only to registered viewer pids whose
command line still matches the registry entry; `--dry-run`/`dry_run=true`
previews the close path first, and `--all`/`all=true` is the explicit orphan
cleanup escape hatch.
When more than one workspace is known, a compact workspace position
button cycles between running and stopped workspaces without opening a full
manager. The default shape is a square-ish screen-first monitor with subtle
silver-edged controls and hover tooltips for compact labels like `Shot`, `Rev`,
`Evt`, and `Log`; drag from the header area to reposition it, and use the
bottom-right corner grip to resize it. The viewer persists its size, popup
position, opt-in screen stream preference, and footer context mode in `XDG_CONFIG_HOME/agent-workspace-linux/viewer.json`
(falling back to `$HOME/.config/agent-workspace-linux/viewer.json`) so the
small monitor returns with the user's last shape and placement. Its compact
live-control buttons write the same MCP control state: active mode offers `RO`
and `Pause`, read-only mode offers `Run` and `Pause`, and paused mode offers
`Run` and `RO`, letting the user switch the agent boundary while watching the
workspace without a hidden cycling control.
Running workspaces expose compact monitor actions for capturing the active
window (`Shot`) or streaming screen frames (`View`) without turning the monitor
into a full management app. Screen streaming is off by default: periodic status,
activity, app, and control-state refreshes continue, but root screenshots are
only captured while `View` is enabled. The viewer reuses `viewer-frame.png`
inside the workspace runtime for the stream frame, avoiding a new timestamped
PNG for each refresh. The
footer link strip exposes `Files` for the workspace artifact folder, `Evt` for
the workspace event log, and `Log` when the active or most recent app has a
saved stdout/stderr log, using the same live-or-manifest lookup paths as the CLI
tools. Running workspaces expose a two-step `Rev` action that
stops the workspace and removes its runtime files; plain `Stop` remains a
stop-only action. Stopped workspaces expose a two-step `Clean` action that
removes the stopped workspace runtime through the same stale-workspace cleanup
path used by `workspace cleanup`.
Set `AGENT_WORKSPACE_VIEWER_BACKEND=x11` or
`AGENT_WORKSPACE_VIEWER_BACKEND=wayland` to force a backend while testing
desktop-specific behavior. By default the viewer does not request always-on-top
state: on X11/Xwayland it skips taskbar/pager and keeps a utility type hint
without requesting above/sticky or notification state.
Use `viewer --always-on-top` or `workspace_open_viewer` with
`always_on_top=true` only when the user explicitly wants overlay behavior; that
opt-in path requests popup/layer-shell plus above/sticky and notification-style
hints where the desktop supports them. If a viewer is already registered for the
workspace, `workspace_open_viewer` reuses it rather than opening a second topmost
variant. When `workspace_open_viewer` launches the child viewer from MCP, it
carries the active MCP permission ceiling into the child; a direct CLI viewer can
use the same global `--permissions PATH` option before `viewer`.
With no configured permissions file, the viewer keeps the clean default state
and does not add its own ceiling. With an explicit ceiling, profile-backed
starts validate the saved profile and applied policy before opening the
workspace. Start, profile-backed start, and stop operations run in the
background so longer setup/startup work does not freeze the overlay. Periodic
workspace status refresh runs in the background without screenshot capture by
default; opt-in screen streaming also runs in the background and overwrites one
viewer frame file instead of accumulating PNGs. The footer uses live
workspace context instead of capture metadata: it names in-flight viewer
actions, revoke/cleanup confirmations, the latest actionable workspace event,
including workspace-owned browser page reads and navigations, the active
app/window, inferred task intent, and the MCP ceiling in the isolation mode
when available.
A compact footer mode control cycles between activity, task intent,
isolation/profile policy, and app summaries without adding a larger management
panel.

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
For launched apps, prefer the returned `apps[0].id`/`app_id` when sending later
window-targeted input. Many GUI programs retitle their windows after startup, so
title matching is useful for discovery but should not be the stable control
handle.
`workspace_list_windows` accepts `include_hidden=true` to return
minimized/hidden windows as well as visible windows. `workspace_observe` also
accepts `include_hidden=true` plus `events`, `events_tail`, and
`events_since_sequence` for single-call polling.
