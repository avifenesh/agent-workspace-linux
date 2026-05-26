# Dogfood Validation

This file records real MCP dogfood results that gate the later permission
hardening work. It is intentionally evidence-oriented: verified behavior goes
here, while policy design stays in `permission-boundary-roadmap.md`.

## 2026-05-26 Workspace-Owned Browser CDP MCP Pass

Environment:

- Ran against the local repository build from
  `/home/avifenesh/projects/agent-workspace-linux`.
- The pass focused on replacing host Chrome bridge assumptions with a
  workspace-owned browser-control surface that agents can discover through the
  repo-owned MCP.

Verified:

- `workspace_browser_targets`, `workspace_browser_snapshot`,
  `workspace_browser_search_results`, and `workspace_browser_navigate` are
  exposed through the direct stdio MCP and the CLI
  `workspace browser-targets`, `workspace browser-snapshot`,
  `workspace browser-search-results`, and `workspace browser-navigate`
  subcommands.
- The tool selects a running workspace Chrome/Chromium app, derives its
  `--user-data-dir`, maps `/workspace/...` profile mounts back to the host
  profile copy when needed, validates that any declared
  `--remote-debugging-address` is loopback-only, reads `DevToolsActivePort`
  when available, and falls back to an explicit loopback
  `--remote-debugging-port` when the browser was launched that way.
- `restricted-chrome` and `browser-session` profile templates now launch
  Chrome/Chromium with loopback DevTools enabled, so browser automation can stay
  attached to the isolated workspace browser rather than the user's normal
  Chrome profile.
- `mcp_task_plan` now points running and newly launched browser/shopping flows
  at `workspace_browser_targets` followed by `workspace_browser_snapshot`,
  `workspace_browser_search_results` for result pages, and, when `target_url`
  is provided, `workspace_browser_navigate`, while preserving the
  real-world/cart/checkout approval boundary.
- `scripts/mcp_workspace_browser_cdp_smoke.js` starts the repo-owned MCP,
  launches Chrome inside a disposable workspace, calls
  `workspace_browser_targets`, asserts that the returned endpoint is loopback
  and belongs to the launched workspace app, then performs page readback and
  navigation through the MCP `workspace_browser_snapshot`,
  `workspace_browser_search_results`, and `workspace_browser_navigate` tools.
  The smoke asserts that structured result cards and browser action events were
  recorded in the workspace event log, with no workspace keyboard/mouse input
  events needed.
- A live Amazon GPU dogfood pass against the already visible
  `real-grocery-visible` workspace used only
  `workspace browser-search-results` on the workspace-owned Chrome app and
  extracted structured RTX/Pro GPU result cards. The follow-up product fix adds
  visible `vram_gb` extraction and `min_vram_gb` filtering, so GPU-shopping
  tasks can ask for cards above a VRAM threshold without parsing raw text or
  returning 16GB false positives. Browser tool responses now also warn when a
  workspace event cannot be recorded, which makes stale-daemon or IPC schema
  skew visible instead of silently dropping activity-footer updates.
- Browser workspace events now carry explicit omission markers and remain
  metadata-only. Snapshot/navigation/search events record target, URL/title,
  counts, and filters, but not raw DOM text, headings, links, or result-card
  excerpts.
- The GPUI viewer activity footer now renders those `browser_snapshot` and
  `browser_navigate` events as readable status such as "Read browser page" and
  "Navigated browser to ...", so the visible monitor tells the user what the
  agent is doing in the workspace browser without opening a larger app panel.
- The real-grocery collector now records the same
  `workspace_browser_targets` discovery inside `real_browser.chrome_devtools`
  during live real-browser runs, and the release/import gates reject
  real-grocery evidence that lacks loopback workspace-browser target proof or
  stores raw logged-in page text.
- `cargo fmt --check`, `cargo test --locked`, `node --check
  scripts/mcp_workspace_browser_cdp_smoke.js`, `node
  scripts/mcp_workspace_browser_cdp_smoke.js`, `node
  scripts/mcp_clean_permissions_smoke.js`, and
  `scripts/prod_readiness_smoke.sh` passed after the change.

Finding:

- Browser dogfood no longer needs the host Chrome bridge, Playwright, curl, or a
  script-owned CDP client as the efficient path. Agents can discover, read,
  extract structured result cards from, and navigate the Chrome instance they
  launched inside the workspace through MCP tools, which keeps browser
  automation inside the same permission, profile-copy, event-log, and audit
  boundary as the rest of the runtime.

## 2026-05-25 No-Host-Display MCP Viewer Pass

Environment:

- Ran a clean/default JSON-RPC MCP smoke against the local repository build with
  `DISPLAY`, `WAYLAND_DISPLAY`, `WAYLAND_SOCKET`, and
  `AGENT_WORKSPACE_VIEWER_BACKEND` removed from the MCP environment.
- The pass focused on keeping the product contract precise: no display is not
  the same as `mcp --headless`, but host-visible viewer work still must not be
  recommended or spawned.

Verified:

- `scripts/mcp_no_host_display_viewer_smoke.js` starts plain `mcp` without
  `--headless`, confirms `mcp_permissions.configured=false`, and confirms
  `mcp_session_brief.headless=false`.
- `mcp_session_brief.doctor.ready_for_host_viewer=false` and
  `viewer_blockers` names the missing `DISPLAY` / `WAYLAND_DISPLAY` host
  display requirement.
- The smoke starts a real hidden X11 workspace through `workspace_start` while
  the host display is unset, proving the isolated workspace runtime can still
  operate without treating the server as headless.
- With that workspace running, `mcp_session_brief` does not recommend
  `workspace_open_viewer`.
- Non-headless app-QA and complete grocery plans suppress
  `workspace_open_viewer` steps and `host_visible_ui` approval checkpoints when
  `ready_for_host_viewer=false`, instead of offering a viewer that cannot open.
- `mcp_task_plan` now exposes and smokes `host_viewer_ready`,
  `viewer_available`, and
  `viewer_unavailable_reason`, so host UI and agents can tell whether viewer
  steps are absent because of explicit `--headless` mode or because this
  non-headless MCP lacks a host display.
- Natural shopping phrases such as "buy milk and eggs for delivery" now route
  to the same browser/shopping plan as explicit grocery wording, preserving
  cart mutation and checkout/account-change approval boundaries.
- `workspace_open_viewer` returns `ok=false` with a host-display readiness
  message rather than spawning a child process that would immediately fail.
- `scripts/integration_smoke.sh` now runs this no-host-display smoke between
  the normal non-headless viewer smoke and the clean/headless MCP smoke.
- `cargo fmt --check`, `cargo build --locked`,
  `cargo test --locked`, `node scripts/mcp_no_host_display_viewer_smoke.js`,
  `node scripts/mcp_non_headless_viewer_smoke.js`,
  `node scripts/mcp_clean_permissions_smoke.js`,
  `node scripts/mcp_permissions_smoke.js`, and `scripts/integration_smoke.sh`
  passed after the availability-field change.

Finding:

- Headless remains an explicit MCP flag, but Linux service/no-display launches
  now have a safer UX: the agent can still plan and run hidden workspaces while
  host-visible viewer affordances are withheld until a real host display is
  available.

## 2026-05-25 App-QA Intent and Action Boundary Pass

Environment:

- Ran against the local repository build from
  `/home/avifenesh/projects/agent-workspace-linux`.
- The pass focused on making app-QA planning as machine-readable as the
  browser/shopping plan, so host UI can show the user's likely next boundary
  without scraping step prose.

Verified:

- `normalize_task_intent` now treats natural app-QA phrases such as "test the
  local UI", "verify the frontend", "debug the desktop window", "run smoke
  checks", and "check render behavior" as `app_qa` intents.
- `mcp_task_plan.task_context.action_boundaries` for app-QA now separates
  read-only project observation, hidden workspace start/attach, post-start
  evidence collection, workspace-local input, and mounted project file writes.
- Mounted project file writes are exposed as a distinct `project_file_write`
  approval kind, so code/file changes are not silently folded into normal app
  driving.
- Clean/default, locked-permissions, and non-headless MCP smokes now assert the
  structured app-QA boundary map.
- `cargo fmt --check`, `cargo test --locked`, `node
  scripts/mcp_clean_permissions_smoke.js`, `node
  scripts/mcp_permissions_smoke.js`, `node
  scripts/mcp_non_headless_viewer_smoke.js`, `node
  scripts/mcp_no_host_display_viewer_smoke.js`, and
  `scripts/integration_smoke.sh` passed after the change.

Finding:

- App-QA host UX can now render the next likely user boundary directly from
  `task_context`, including the difference between observing/driving the
  isolated workspace and changing files in the mounted project.

## 2026-05-25 Approval Summary Pass

Environment:

- Ran against the local repository build from
  `/home/avifenesh/projects/agent-workspace-linux`.
- The pass focused on turning the existing flat approval checkpoint list into a
  host-renderable next-boundary summary.

Verified:

- `mcp_task_plan` now includes `approval_summary` with `blocking_count`,
  `approval_required_count`, `approval_kinds`, and `next_boundary`.
- `next_boundary` chooses the first blocking checkpoint before later
  non-blocking approvals, so host UI can render the next required input,
  permission ceiling, live-control reactivation, hidden-workspace approval, or
  real-world approval without reimplementing checkpoint ordering.
- Clean/default, locked-permissions, and non-headless MCP smokes now assert the
  approval summary for app-QA and browser/grocery plans.
- `cargo fmt --check`, `cargo build --locked`, `cargo test --locked`,
  `node scripts/mcp_clean_permissions_smoke.js`,
  `node scripts/mcp_permissions_smoke.js`,
  `node scripts/mcp_non_headless_viewer_smoke.js`,
  `node scripts/mcp_no_host_display_viewer_smoke.js`, and
  `scripts/integration_smoke.sh` passed after the change.

Finding:

- Hosts now get both the complete checkpoint list and the one boundary most
  likely to need user attention first. This narrows the final approval UI gap
  without changing the clean/default permission model.

## 2026-05-25 Session Recommendation Approval Summary Pass

Environment:

- Ran against the local repository build from
  `/home/avifenesh/projects/agent-workspace-linux`.
- The pass focused on letting `mcp_session_brief` expose a first approval
  boundary before a host has called `mcp_task_plan`.

Verified:

- Each `mcp_session_brief.recommendations[]` entry now carries
  `approval_summary` with blocking count, approval-required count, approval
  kinds, and next boundary.
- The top-level `mcp_session_brief.approval_summary` summarizes the
  already-prioritized recommendations and selects the first blocking boundary,
  or otherwise the first approval-required boundary.
- Clean/default, locked-permissions, and non-headless MCP smokes assert that
  browser/grocery planning recommendations expose `real_world_action` in both
  per-recommendation and top-level summaries.
- `cargo fmt --check`, `cargo build --locked`, `cargo test --locked`,
  `node scripts/mcp_clean_permissions_smoke.js`,
  `node scripts/mcp_permissions_smoke.js`,
  `node scripts/mcp_non_headless_viewer_smoke.js`,
  `node scripts/mcp_no_host_display_viewer_smoke.js`, and
  `scripts/integration_smoke.sh` passed after the change.

Finding:

- Hosts can now show a compact first boundary from the session brief itself,
  then call `mcp_task_plan` only when they need the full workflow. This reduces
  duplicated approval-ordering logic in Codex Desktop or non-Codex MCP hosts.

## 2026-05-25 Grocery Browser Workflow Smoke

Environment:

- Ran against the local repository build from
  `/home/avifenesh/projects/agent-workspace-linux`.
- The pass focused on a concrete browser/grocery dogfood loop rather than only
  planning metadata.

Verified:

- `scripts/grocery_browser_workflow_smoke.sh` starts a hidden workspace, opens
  Chrome/Chromium against a local grocery data page, uses workspace-local
  keyboard and paste actions to draft a grocery list, and submits that draft to
  the page's cart state.
- The smoke waits for the browser window title to become
  `cart:3:checkout-locked`, captures a screenshot-backed observation, and fails
  if the page crosses into an `order-submitted` state.
- `scripts/integration_smoke.sh` now runs the grocery workflow when
  Chrome/Chromium is available, after the existing local-dev and native-browser
  input checks.
- `bash -n scripts/grocery_browser_workflow_smoke.sh
  scripts/integration_smoke.sh`, `cargo fmt --check`, `cargo test --locked`,
  `scripts/grocery_browser_workflow_smoke.sh`, and
  `scripts/integration_smoke.sh` passed after the change.

Finding:

- The runtime now has a repeatable grocery-style browser dogfood path that
  exercises observation, browser focus, address navigation, literal typing,
  paste, cart mutation, screenshot observation, and the no-checkout boundary.
  It is still not live real-account grocery dogfood, but it proves the workflow
  mechanics before that higher-risk pass.

## 2026-05-25 Grocery Approval State Pass

Environment:

- Ran against the local repository build from
  `/home/avifenesh/projects/agent-workspace-linux`.
- The pass focused on representing cart approval separately from checkout
  approval in `mcp_task_plan`.

Verified:

- Grocery `task_context.action_boundaries` now include `approved` and
  `missing_approvals`.
- `mcp_task_plan` accepts `cart_mutation_approved`, `final_cart_reviewed`, and
  `real_world_action_approved` inputs. When only cart mutation and final cart
  review are approved, the cart boundary becomes approved while checkout still
  reports the missing explicit checkout approval.
- Clean/default and locked-permissions MCP smokes assert the new approval state
  fields for grocery/cart boundaries.
- `cargo fmt --check`, `cargo build --locked`, `cargo test --locked`,
  `node scripts/mcp_clean_permissions_smoke.js`,
  `node scripts/mcp_permissions_smoke.js`,
  `node scripts/mcp_non_headless_viewer_smoke.js`,
  `node scripts/mcp_no_host_display_viewer_smoke.js`,
  `scripts/grocery_browser_workflow_smoke.sh`, `scripts/integration_smoke.sh`,
  and `git diff --check` passed after the change.

Finding:

- Hosts can now persist a user's approval to draft a cart without conflating it
  with approval to place an order or change account state. This makes the
  synthetic grocery dogfood path closer to a future real-account grocery flow.

## 2026-05-25 Prod Readiness Gate Pass

Environment:

- Added a broad local pre-release gate at `scripts/prod_readiness_smoke.sh`.
- The gate is intentionally local-first: it can run the runtime checks in this
  repository and the sibling Codex Desktop agent-workspace tests when
  `/home/avifenesh/projects/codex-desktop-linux` is present.

Verified:

- `bash -n scripts/prod_readiness_smoke.sh` and
  `scripts/prod_readiness_smoke.sh` passed on this machine after the gate was
  added.
- The gate covers `cargo fmt --check`, `cargo build --locked`,
  `cargo clippy --locked -- -D warnings`, `cargo test --locked`, all focused
  MCP permission/viewer smokes, the grocery browser workflow,
  `scripts/integration_smoke.sh`, and `git diff --check`.
- It also runs `node --check linux-features/agent-workspace/patch.js`,
  `node --test linux-features/agent-workspace/test.js`, and Desktop
  `git diff --check` when the sibling Desktop repo is present.
- The GPUI viewer smoke runs automatically when the host has `DISPLAY` plus the
  X11/ImageMagick inspection tools, and can be made mandatory with
  `REQUIRE_GUI_SMOKE=1`. Desktop tests can be made mandatory with
  `REQUIRE_DESKTOP_SMOKE=1`.
- A full pass after the real-grocery dogfood requirement readiness state was
  wired into `mcp_task_plan` produced
  `target/real-grocery-dogfood/20260525T205013Z.json`,
  `target/viewer-desktop-matrix/20260525T205030Z.json`,
  `target/release-gate-audit/20260525T205036Z.json`, and
  `target/final-review-bundle/20260525T205036Z.md`; the sibling Desktop node
  test suite reported 16 passing tests.
- `scripts/release_gate_audit.py` now scans generated viewer-matrix and
  real-grocery evidence, writes reports under `target/release-gate-audit/`, and
  can be made strict with `--require-all` or `REQUIRE_RELEASE_GATES=1` through
  the broad smoke script. Release evidence must be fresh by default:
  `--max-evidence-age-days` defaults to 14, with `0` disabling the age check.
  Evidence must also match the current combined source identity, a hash over
  the runtime source (`Cargo.toml`, `Cargo.lock`, `src/`, and `scripts/`) plus
  the sibling Codex Desktop feature source
  (`../codex-desktop-linux/linux-features/agent-workspace` and
  `../codex-desktop-linux/agent-workspaces-linux.js`, or
  `CODEX_DESKTOP_LINUX_REPO` when set), unless `--no-source-identity-check` is
  used explicitly. `REQUIRE_RELEASE_GATES=1` also enables
  `--require-clean-source`, so release mode requires no git status entries
  under those runtime and Desktop feature paths. Its
  human-review marker must also match the current review-scope identity. In a
  dirty worktree that identity hashes both repos' status, staged and unstaged
  diffs, and non-ignored untracked file contents; in a clean worktree it hashes
  the current `HEAD` commit content. Its `--self-test` mode builds synthetic pending,
  stale-complete, mismatched-source, mismatched-review-scope, dirty-source, and
  fresh-clean-complete release-evidence fixtures, proving the audit can fail
  when evidence is missing, stale, dirty, from a different source tree, or from
  a different reviewed diff scope and pass when fresh GNOME/KDE, X11/Wayland,
  native Wayland observation, real-browser grocery, and human-review evidence
  are all present for a clean current combined source. Its current
  pending output names the missing KDE/Plasma row, X11 row, native Wayland
  observation, real-browser grocery pass, and human final diff review. The
  human-review gate remains pending until a human-created
  `target/release-gate-human-review.json` marker records schema
  `agent-workspace-linux.human_final_diff_review.v1` with status `reviewed`.
- `scripts/prepare_grocery_profile_copy.js` now prepares the disposable browser
  profile copy required by real-browser grocery dogfood. It skips browser
  locks, sockets, caches, crash dumps, extension/web-app payloads, and symlinks,
  writes `.agent-workspace-grocery-profile-copy.json`, and has a self-test wired
  into the broad gate. The guarded wrapper defaults the disposable copy outside
  the repo `target/` tree under `$XDG_RUNTIME_DIR/agent-workspace-linux/`, or
  `/tmp` when `XDG_RUNTIME_DIR` is unavailable.
  `scripts/real_grocery_dogfood_probe.js` requires that manifest before it
  opens a real grocery site, and the release-gate audit only
  accepts real-browser grocery reports with a valid profile-copy manifest,
  passed MCP plan assertions, an approved cart-draft interaction, the
  cart-draft safety contract, and a workspace event audit proving only declared
  cart-draft input events occurred. The probe baselines the workspace event log
  after browser launch, collects post-baseline events with a step-sized tail,
  and the audit rejects reports that do not cover every declared cart-draft
  input step. The page readback evidence stores URL/title and text
  length/truncation metadata only; raw logged-in page text, excerpts, links, and
  headings are omitted and rejected. The real-browser probe also stops and
  cleans the workspace runtime by default, and release evidence must prove that
  cleanup happened; `REAL_GROCERY_PRESERVE_WORKSPACE=1` is for debugging only.
- `scripts/final_review_bundle.py` now writes JSON and Markdown bundles under
  `target/final-review-bundle/` during the broad gate. These bundles collect
  combined runtime/Desktop source identity, review-scope identity, runtime and
  sibling Desktop dirty scope, latest evidence reports,
  release-audit/current-source and review-scope consistency, pending release
  gates, concrete next evidence commands, a human review checklist, generated
  runtime/Desktop review diffs, and the exact marker template for
  `target/release-gate-human-review.json`; they intentionally do not claim
  review happened. After actual human review, set `HUMAN_REVIEW_NOTES` to
  specific non-placeholder notes and run the guarded marker command with
  `--notes "$HUMAN_REVIEW_NOTES"` to regenerate the runtime/Desktop review
  artifacts and write the marker. The marker includes meaningful reviewer/notes
  metadata plus `review_artifacts` hashes that the audit verifies before
  accepting the human review gate. The broad gate also runs
  `scripts/final_review_bundle.py --self-test` and
  `scripts/create_human_review_marker.py --self-test` so those command recipes,
  stale-audit checks, and marker validation are covered. Because the
  review-scope identity includes docs and untracked files, regenerate the audit
  and final-review bundle after final doc edits and before creating the
  human-review marker.
- `scripts/release_next_steps.py` now prints the concise release roadmap from
  the latest final-review bundle, including combined source-hash consistency,
  review-scope consistency, pending gates, next commands, and the final strict
  release command.
- Added `scripts/import_release_evidence.py` for copied external
  viewer/app-QA/grocery release reports and copied human-review markers. It
  validates schema, current source identity, passing viewer smoke, local GUI
  app-QA safety/evidence, real-browser grocery mode, checkout refusal,
  disposable profile manifest validity, passed MCP plan assertions, approved
  cart-draft interaction evidence, cart-draft safety contracts, declared-only
  workspace input events, enough input evidence to cover the declared
  cart-draft input steps, workspace cleanup evidence, non-local HTTPS grocery
  URLs, human-review review-scope identity, and human-review artifact bytes before copying
  reports into the audit directories or writing
  `target/release-gate-human-review.json`. The broad gate runs its self-test so
  stale-source, skipped, nonpassing, contradictory session-attestation,
  unexpected workspace-input, weak grocery-safety, missing-review-artifact, and
  review-scope-mismatched reports remain rejected by default.
- Added `scripts/export_release_evidence_bundle.py` for collecting external
  viewer rows, app-QA, real-grocery dogfood, and final human-review markers
  against the exact current source identity. It exports a tarball with the
  runtime source, sibling Codex Desktop feature source, source and review-scope
  manifest, `collect-viewer-evidence.sh`, `collect-app-qa-evidence.sh`,
  `collect-real-grocery-evidence.sh`, and `create-human-review-marker.sh`; the
  broad gate runs its self-test so the bundle shape is checked before release
  evidence is collected elsewhere.
  Export after final doc/source edits because the
  review-scope manifest intentionally includes docs.
- Native Wayland release evidence now requires explanatory observation notes:
  `scripts/viewer_desktop_matrix_probe.sh` rejects
  `NATIVE_WAYLAND_LAYER_SHELL_OBSERVED=1` unless
  `NATIVE_WAYLAND_LAYER_SHELL_NOTES` is also set, and
  `scripts/release_gate_audit.py` only counts native Wayland layer-shell
  evidence when those notes are present on a Wayland session and make a
  positive layer-shell/top-layer claim. GNOME/Xwayland fallback observations and
  notes that say the viewer was not layer-shell are intentionally rejected.
  Real-browser
  grocery release evidence now also requires an HTTPS, non-local grocery URL so
  localhost, reserved, and private-network URLs cannot satisfy the real dogfood
  gate. Viewer matrix reports also include `session_consistency`; when
  `loginctl` metadata is available, contradictory environment/session claims are
  not release eligible. They also include display-server attestation for local
  Wayland/X11 sockets and `lsof` process evidence, and release import/audit
  rejects remote X forwarding plus known nested/headless host displays such as
  Xvfb, xpra, Xephyr, and headless Weston.

Finding:

- The repo now has one executable gate for the current runtime, MCP, viewer,
  dogfood, and thin Desktop integration evidence. The remaining release gaps are
  explicit manual gates: multi-desktop Linux viewer UX, real-account grocery
  dogfood without crossing checkout/account boundaries, and human final diff
  review.

## 2026-05-26 Objective Completion Audit Pass

Environment:

- Added `scripts/objective_completion_audit.py` as the requirement-level audit
  for the active product objective.
- Wired the audit into `scripts/prod_readiness_smoke.sh` after the release-gate
  audit, source bundle, final-review bundle, release-next summary, and smoke
  report are generated.

Verified:

- `scripts/objective_completion_audit.py --self-test` passes and covers the
  important negative cases: current smoke missing, external gates pending, and a
  stale final-review bundle even when release gates are otherwise marked passed.
- The broad smoke now runs the objective audit in non-failing mode by default,
  writing `target/objective-completion-audit/*.json`; when
  `REQUIRE_RELEASE_GATES=1` is set, it also passes `--require-complete` so a
  strict release cannot succeed unless the full objective map is proven.
- The audit verifies current combined source identity and review-scope identity
  for the prod-readiness smoke report, release-gate audit, and final human-review
  bundle before it can mark requirements complete.

Finding:

- Completion is now machine-readable and intentionally conservative. The local
  MCP/runtime/UI work can keep advancing, but the active goal remains pending
  until the existing external viewer matrix, real logged-in grocery dogfood, and
  human review gates are all satisfied for the same reviewed source.

## 2026-05-25 Detached Viewer Reaping Pass

Environment:

- Ran against the local repository build from
  `/home/avifenesh/projects/agent-workspace-linux`.
- The pass focused on the long-running MCP lifecycle behavior of detached
  host-visible viewer launches.

Verified:

- `viewer::open_viewer`, the X11 replacement relaunch path, and host path open
  helpers now spawn detached children through a shared reaper helper instead of
  dropping `std::process::Child` handles.
- Added `viewer::tests::detached_child_spawn_is_reaped`, which starts a
  short-lived child and verifies that `/proc/<pid>` disappears, proving the
  child is waited on rather than left as a zombie under the parent process.
- `cargo fmt --check`, `cargo check --locked`,
  `cargo test --locked viewer::tests::detached_child_spawn_is_reaped`, and
  `scripts/prod_readiness_smoke.sh` passed after the change. The broad gate now
  includes 103 Rust tests plus the focused MCP, grocery, integration, GPUI, and
  sibling Codex Desktop checks.

Finding:

- `workspace_open_viewer` remains a separate child process, which matches the
  product direction, but it is now safer for long-lived MCP servers because
  exited viewer processes are reaped instead of accumulating as zombies.

## 2026-05-25 Viewer Desktop Matrix Probe Pass

Environment:

- Added `scripts/viewer_desktop_matrix_probe.sh` to turn the remaining
  multi-desktop Linux viewer validation into repeatable evidence rows.
- The probe writes JSON reports under `target/viewer-desktop-matrix/`, outside
  the tracked source tree.

Verified:

- `bash -n scripts/viewer_desktop_matrix_probe.sh scripts/prod_readiness_smoke.sh`,
  `scripts/viewer_desktop_matrix_probe.sh`, and
  `scripts/prod_readiness_smoke.sh` passed after the probe was added.
- This machine produced a passing matrix row at
  `target/viewer-desktop-matrix/20260525T165426Z.json` with
  `desktop_label="ubuntu:GNOME / wayland / ubuntu"`,
  `counts_for_release_matrix=true`, and `viewer_smoke.status="passed"`.
- The probe records OS release, kernel/platform, `XDG_SESSION_TYPE`,
  `XDG_CURRENT_DESKTOP`, `DESKTOP_SESSION`, `DISPLAY`, `WAYLAND_DISPLAY`,
  `loginctl` session data when available, X11 root-window metadata when
  available, required command availability, and the focused GPUI viewer smoke
  result.
- `scripts/prod_readiness_smoke.sh` now calls the matrix probe instead of
  invoking `scripts/gpui_viewer_smoke.sh` directly when GUI validation is
  available, so the broad gate leaves a desktop/session evidence row.
- `REQUIRE_VIEWER_SMOKE=1` makes a skipped visual smoke fail the probe;
  `REQUIRE_GUI_SMOKE=1` keeps the same strict behavior through the prod gate.

Finding:

- The release matrix gap is now operational rather than prose-only: each
  desktop/session run can produce comparable JSON evidence. This machine still
  contributes only its current desktop/session; GNOME/KDE and X11/Wayland-like
  coverage across multiple environments remains a release gate.

## 2026-05-25 Real Grocery Dogfood Probe Pass

Environment:

- Added `scripts/real_grocery_dogfood_probe.js` as the guarded entrypoint for
  future logged-in grocery dogfood.
- The probe defaults to plan-only mode and writes reports under
  `target/real-grocery-dogfood/`.

Verified:

- `node --check scripts/real_grocery_dogfood_probe.js` and
  `scripts/real_grocery_dogfood_probe.js` passed in plan-only mode, producing
  `target/real-grocery-dogfood/20260525T165716Z.json`.
- `scripts/prod_readiness_smoke.sh` passed after the probe was wired into the
  broad gate, producing
  `target/real-grocery-dogfood/20260525T165838Z.json` in plan-only mode.
- Plan-only mode starts a clean/default `mcp --headless`, calls
  `mcp_task_plan` with real-grocery-shaped inputs, asserts that cart mutation
  is not approved by default, asserts checkout/account change approval is not
  approved by default, then asserts that explicit cart approval plus final cart
  review approves only cart mutation while checkout remains blocked behind
  `explicit_checkout_approval`.
- Real browser mode requires `REAL_GROCERY_DOGFOOD=1`, `GROCERY_TARGET_URL`,
  `GROCERY_USER_DATA_DIR`, and `GROCERY_PROFILE_IS_DISPOSABLE_COPY=1`; it
  refuses to run if `GROCERY_TARGET_URL` is not an HTTPS, non-local grocery
  site, or if `CHECKOUT_APPROVED=1` or `REAL_WORLD_ACTION_APPROVED=1`.
- Release-counting real browser mode now also requires
  `REAL_GROCERY_INTERACTION_MODE=cart-draft-approved`,
  `CART_MUTATION_APPROVED=1`, `FINAL_CART_REVIEWED=1`, and
  `GROCERY_CART_DRAFT_STEPS_JSON=/path/to/steps.json`. The step file must
  declare the workspace-local browser input actions and include at least one
  step marked as an explicit cart mutation, while checkout/order/account words
  are rejected from input-step safety labels.
- `scripts/collect_real_grocery_evidence.sh --print-cart-draft-steps-template`
  prints a starter step file, and `--validate-cart-draft-steps PATH` validates a
  site-specific step file without opening a browser through the same wrapper
  used by local and exported-bundle collection. The lower-level
  `scripts/real_grocery_dogfood_probe.js --preflight-real-grocery` checks the
  release-counting real run inputs without launching the browser, and
  `--self-test` covers the template plus rejection paths for checkout-like
  labels, unsupported actions, missing cart mutations, unexpected workspace
  input events, and the no-launch preflight.
- `scripts/collect_real_grocery_evidence.sh` is the guarded release wrapper for
  the real browser gate. `--preflight-only` prepares the disposable profile copy
  and validates the real URL, approved cart-draft step file, browser executable,
  and no-checkout authority without opening the site. It also writes a durable
  preflight artifact under `target/real-grocery-preflight/` by default, so the
  exact ready-to-run setup is available in the final review bundle before any
  live grocery browser is opened. The wrapper now also accepts
  `REAL_GROCERY_PROFILE_DIRECTORY` for Chrome/Chromium logins that live in a
  named profile such as `Profile 1`; it validates that directory in both the
  source and disposable copy, passes `--profile-directory=...` during the live
  run, and records the chosen profile directory in the preflight and live
  reports. `--run-real-browser` repeats that preflight and then opens the real
  grocery site only under `cart-draft-approved` mode.
- `mcp_task_plan` now exposes the same release-facing contract under
  `task_context.dogfood_requirements[]` for grocery plans, so MCP hosts and
  agents can discover the disposable-profile, validated-step-file,
  cart-approval, final-review, no-checkout-approval, declared-input evidence
  requirements without reading release scripts. The requirement status becomes
  `ready` only when the plan receives a real HTTPS non-local target,
  `profile_is_disposable_copy=true`, `cart_draft_steps_validated=true`,
  `cart_mutation_approved=true`, `final_cart_reviewed=true`, and no checkout or
  real-world approval.
- A refusal check with `CHECKOUT_APPROVED=1` failed before launching a browser,
  with the expected message telling the caller to unset checkout/real-world
  approval.
- `scripts/prod_readiness_smoke.sh` now runs the plan-only probe and syntax
  check as part of the broad local gate.

Finding:

- The live real-account grocery gap is smaller and safer: the repo now has a
  guarded probe that can open a disposable copied logged-in profile only after
  explicit setup and can prove approved cart-draft actions without checkout,
  order, or account mutation. A true real-account run still needs a disposable
  profile copy, user-selected grocery URL, and site-specific cart-draft step
  file.

## 2026-05-25 Natural Shopping Intent Planning Pass

Environment:

- Ran against the local repository build from
  `/home/avifenesh/projects/agent-workspace-linux`.
- The pass focused on routing ordinary user phrasing into the existing
  browser/shopping/grocery safety plan instead of requiring exact "grocery
  shopping" wording.

Verified:

- `normalize_task_intent` now treats natural shopping phrases such as "buy",
  "purchase", "cart", "checkout", "order", and "delivery" as `browser_task`
  intents.
- The unit test `natural_shopping_phrases_normalize_to_browser_task` covers
  examples like "buy milk and eggs", "add bananas to cart", and "order
  groceries for delivery".
- `scripts/mcp_clean_permissions_smoke.js` now calls `mcp_task_plan` with
  `intent="buy milk and eggs for delivery"` and verifies the clean/default MCP
  still produces the browser/shopping plan with required shopping inputs,
  `cart_mutation`, and `real_world_action` checkout/account boundaries.
- `cargo fmt --check`, `cargo build --locked`, `cargo test --locked`,
  `node scripts/mcp_clean_permissions_smoke.js`,
  `node scripts/mcp_permissions_smoke.js`,
  `node scripts/mcp_non_headless_viewer_smoke.js`,
  `node scripts/mcp_no_host_display_viewer_smoke.js`, and
  `scripts/integration_smoke.sh` passed after the change.

Finding:

- The agent UX now better matches how users actually ask for grocery work: a
  casual "buy/order/add to cart" request lands in the same safe browser plan
  with cart and checkout boundaries, rather than falling through to the generic
  unknown-intent plan.

## 2026-05-25 Viewer Doctor Readiness Pass

Environment:

- Ran against the local repository build from
  `/home/avifenesh/projects/agent-workspace-linux`.
- The pass focused on moving GPUI viewer prerequisites out of the handover and
  into the product-facing doctor/install surface.

Verified:

- `workspace_doctor` / `agent-workspace-linux doctor` now reports viewer
  readiness separately from hidden X11 workspace readiness.
- The new doctor payload keeps `ready_for_x11_workspace` for the workspace
  runtime and adds `ready_for_host_viewer`, `viewer.host_display`,
  `viewer.source_build_xkbcommon_x11`, `viewer.host_opener`, and
  `viewer_blockers`.
- On this GNOME Wayland session, the rebuilt local binary reported
  `ready_for_x11_workspace=true`, `ready_for_host_viewer=true`,
  `WAYLAND_DISPLAY=wayland-0`, `DISPLAY=:0`,
  `xkbcommon-x11: 1.13.1`, `/usr/bin/xdg-open`, and no workspace or viewer
  blockers.
- `workspace_open_viewer` now preflights `ready_for_host_viewer` before
  spawning the GPUI child process, so a non-headless MCP launched without a
  host display returns a clear doctor-backed refusal instead of reporting a
  child pid that immediately dies.
- `mcp_session_brief.doctor` now exposes `ready_for_host_viewer` and
  `viewer_blockers`, and its running-workspace viewer recommendation is only
  emitted when the MCP is not headless and the host viewer is ready.
- The README now documents the separate viewer source-build dependency:
  `pkg-config libxkbcommon-x11-dev`.
- `cargo fmt --check`, `cargo check --locked`, `cargo build --locked`,
  `cargo test --locked`,
  `node scripts/mcp_non_headless_viewer_smoke.js`,
  `node scripts/mcp_clean_permissions_smoke.js`, and
  `node scripts/mcp_permissions_smoke.js` passed after the change.
- `scripts/integration_smoke.sh` also passed after the doctor schema change,
  including the locked MCP smoke, non-headless viewer smoke, clean/headless MCP
  smoke, doctor phase, workspace/profile flows, browser-session paths, stale
  cleanup, and self-stop coverage.

Finding:

- The next Linux install gap is smaller: users and MCP hosts can now distinguish
  "hidden workspace can run" from "this session can open the host-visible GPUI
  viewer", and source builders get an explicit signal for the xkbcommon package
  that previously only appeared in handover notes.

## 2026-05-25 Non-Headless MCP Viewer Planning Pass

Environment:

- Ran a new clean/default JSON-RPC MCP smoke against the local repository build
  using `agent-workspace-linux mcp` without `--headless`.
- The pass focused on proving the default MCP server remains UI-capable until
  a host explicitly opts into `mcp --headless`, without opening an actual viewer
  window during the smoke.

Verified:

- `scripts/mcp_non_headless_viewer_smoke.js` now initializes a clean/default
  non-headless MCP server, checks `mcp_permissions.configured=false`, and
  verifies `mcp_session_brief.headless=false`.
- The smoke compares `tools/list` to `mcp_action_catalog`, proving the catalog
  covers the exposed MCP tools while keeping `workspace_open_viewer` classified
  as `headless_gated_open_world`.
- The smoke verifies the `workspace_open_viewer` tool description and action
  catalog document the host-visible child process, `--headless` boundary, and
  opt-in `always_on_top` parameter. It also verifies the tool is classified as
  idempotent because repeated calls reuse the registered workspace viewer instead
  of opening another host-visible window.
- Non-headless app-QA and complete grocery `mcp_task_plan` responses now have
  external JSON-RPC coverage for optional viewer steps:
  `open_viewer_when_project_runs` and `open_viewer_when_browser_runs` are
  offered only after their workspace run dependencies, carry
  `host_visible_ui` approval checkpoints, and do not introduce permission
  blockers in clean/default MCP mode.
- While live MCP control is `read_only` or `paused`, the non-headless smoke now
  proves real workspace/browser start steps are blocked by live-control
  checkpoints, but the optional `workspace_open_viewer` steps remain
  `host_visible_ui` approval steps without a live-control blocker. This keeps
  the floating viewer usable as an observation/recovery surface.
- The `mcp_action_catalog` text now explicitly says `workspace_open_viewer`
  remains available while live control is `read_only` or `paused`, and the
  `always_on_top` parameter note names the same behavior while preserving the
  `--headless` boundary.
- The existing clean/headless smoke catalog comparison helper was corrected and
  now actively verifies that `mcp_action_catalog` exactly matches `tools/list`.
- `cargo fmt --check`, `cargo build --locked`,
  `node scripts/mcp_non_headless_viewer_smoke.js`,
  `node scripts/mcp_clean_permissions_smoke.js`,
  `node scripts/mcp_permissions_smoke.js`, `bash -n scripts/integration_smoke.sh`,
  and `git diff --check` passed after the change. `scripts/integration_smoke.sh`
  now runs the non-headless viewer smoke between the locked-permissions and
  clean/headless MCP smokes.

Finding:

- The external MCP smoke coverage now proves both sides of the viewer boundary:
  `mcp --headless` suppresses host-visible viewer recommendations/refuses
  viewer opens, while default `mcp` remains UI-capable and exposes
  host-visible viewer checkpoints only as explicit open-world approval steps.

## 2026-05-25 Codex Desktop Open Viewer Bridge Pass

Environment:

- Patched `/home/avifenesh/projects/codex-desktop-linux` on branch
  `agent-workspace-linux-feature`.
- The pass focused on making Codex Desktop a thin launcher/status bridge for the
  GPUI viewer owned by this runtime repo.

Verified:

- The generated `linux-agent-workspace` main-process bridge now supports
  `workspaceOpenViewer`, builds `agent-workspace-linux viewer --id <id>`, and
  uses detached `child_process.spawn` with ignored stdio so the UI action does
  not hang waiting for the long-lived GPUI process.
- When Codex has an MCP `--permissions` file configured, the bridge prefixes
  the viewer launch with the same global `--permissions <path>` arguments used
  for other CLI workspace actions. Clean/default usage still adds no permission
  ceiling.
- The Agent Workspaces settings page now exposes `Open Viewer` for active,
  other running, and stopped workspaces with concrete workspace ids, keeping
  Codex Desktop as a launcher/status surface rather than a second serious
  workspace UI.
- A generated-settings VM render test now loads a mocked active workspace,
  renders the actual `Open Viewer` button, clicks it, and verifies the UI sends
  `{ action: "workspaceOpenViewer", workspaceId: "qa-live" }` without an
  `alwaysOnTop` field.
- `node --check linux-features/agent-workspace/patch.js` and
  `node --test linux-features/agent-workspace/test.js` passed in the desktop
  repo. The focused tests assert both halves of the bridge contract: clean/
  default Open Viewer spawns exactly `viewer --id qa-live` with no
  `--permissions` prefix and no topmost flag, while a locked-permissions launch
  spawns `--permissions <path> viewer --id default --always-on-top`. Both paths
  use detached ignored-stdio process handling and call `unref()`.
- In this runtime repo, `cargo run --locked -- viewer --help` confirmed the
  command contract is `viewer [--id ID] [--always-on-top]`, and
  `cargo test --locked viewer::tests` passed.

Finding:

- The desktop integration gap is now narrower: Codex Desktop can launch the
  canonical GPUI viewer without bypassing the MCP permission ceiling, while the
  viewer remains optional and headless hosts still depend on the runtime
  `mcp --headless` boundary.

## 2026-05-25 MCP Action Catalog Completeness Pass

Environment:

- Ran the clean/default and locked-permissions JSON-RPC MCP smokes against the
  local repository build.
- The pass focused on the clean/default contract where `mcp_action_catalog` is
  advisory classification rather than an MCP permission ceiling.

Verified:

- `node scripts/mcp_clean_permissions_smoke.js` now compares `tools/list`
  against `mcp_action_catalog` exactly: every exposed MCP tool must have a
  catalog entry, every catalog entry must name a real exposed tool, and catalog
  tool names must be unique.
- `node scripts/mcp_permissions_smoke.js` performs the same exact catalog /
  `tools/list` comparison while the MCP is spawned with an explicit
  permissions ceiling.
- Both smokes passed, preserving the existing checks that clean/default MCP
  reports `configured=false`, the catalog stays advisory, `workspace_open_viewer`
  is open-world/headless-gated, dry-run mutations document their preview
  behavior, `profile_export.output_path` is a host write, and
  `workspace_wait_app.kill_on_timeout` is a conditional termination.

Finding:

- The advisory action taxonomy now has direct drift protection against the
  exposed MCP tool surface, which is required before hosts can depend on it for
  read-only/mutating/destructive/open-world approval UX in clean/default mode.

## 2026-05-25 GPUI Viewer Topmost Boundary Pass

Environment:

- Ran `scripts/gpui_viewer_smoke.sh` against the local repository build on the
  current GNOME Wayland session using the viewer's X11/Xwayland backend.
- The pass focused on the default-vs-explicit topmost contract for the small
  GPUI monitor, not on native Wayland layer-shell compositor inspection.

Verified:

- The viewer smoke starts a hidden workspace, launches `xclock`, proves the
  active-window screenshot path, app-log path, and event-log artifact path, then
  opens the GPUI viewer with seeded compact size, position, live-refresh, and
  Task footer preferences.
- The default X11/Xwayland viewer advertises the expected
  `agent-workspace-linux-viewer` class, skip-taskbar and skip-pager state, and
  utility window type while explicitly not advertising `_NET_WM_STATE_ABOVE` or
  `_NET_WM_STATE_STICKY`.
- The smoke now launches a second viewer with `--always-on-top` and verifies
  that the opt-in path advertises `_NET_WM_STATE_ABOVE`,
  `_NET_WM_STATE_STICKY`, skip-taskbar, skip-pager, and notification/utility
  window types while still rendering a nonblank compact monitor.
- The X11 overlay hint task now reasserts the requested window-manager state
  during startup instead of exiting after the first successful write. This fixes
  a race where GPUI or the window manager could leave the opt-in topmost window
  with notification/utility type but without above/sticky state by the time the
  smoke inspected it.
- `scripts/gpui_viewer_smoke.sh`, `cargo test --locked viewer::tests`,
  `cargo fmt --check`, and `git diff --check` passed after the change.

Finding:

- The X11/Xwayland viewer now proves both halves of the product boundary:
  default is a gentle non-topmost monitor, while explicit `--always-on-top`
  requests topmost overlay behavior. Native Wayland layer-shell still needs
  compositor-level validation separate from X11 property inspection.

## 2026-05-25 Complete Grocery Input Planning Pass

Environment:

- Ran against the local repository build from
  `/home/avifenesh/projects/agent-workspace-linux`.
- The pass focused on the read-only MCP planning surface for grocery/browser
  tasks once the user has already supplied task details.

Verified:

- `mcp_task_plan` with `intent="grocery shopping"`, an explicit
  `user_data_dir`, `target_url`, `shopping_list`, `budget`, `fulfillment`, and
  `substitution_policy` moves those details into
  `task_context.provided_inputs` and leaves `task_context.missing_inputs`
  empty.
- The complete grocery plan no longer repeats stale `needs_user_input` prompts
  for the supplied grocery details, and its browser run / post-start boundary
  steps no longer list those supplied details as required input.
- The structured action boundaries mark navigation/search and item comparison
  ready without approval, mark draft cart changes ready but approval-gated as
  `cart_mutation`, and keep checkout/order/account changes blocked behind
  `explicit_checkout_approval`.
- The clean/default MCP JSON-RPC smoke still reports
  `mcp_permissions.configured=false`, finds no permission blockers, and keeps
  host-visible viewer checkpoints absent under `mcp --headless`.
- `cargo test --locked
  server::tests::grocery_plan_exposes_structured_action_boundaries` and
  `node scripts/mcp_clean_permissions_smoke.js` passed after the change.

Finding:

- Fully specified grocery input now behaves like task context, not a permission
  ceiling or a repeated prompt. The remaining shopping/grocery gap is live
  real-account dogfood, not planner schema shape.

## 2026-05-25 Restarted Codex MCP Control Pass

Environment:

- Dogfood ran through the restarted Codex app's installed Agent Workspace MCP,
  not the CLI wrapper path.
- `mcp_permissions` reported no configured MCP ceiling, with Codex session
  permissions owning the boundary after hidden-workspace approval.
- `workspace_doctor` reported the X11 runtime ready, and `workspace_list`
  started empty.

Verified:

- `workspace_start` with `acknowledge_hidden_workspace=true` created the hidden
  workspace on `:90` with purpose `Dogfood restarted MCP no-prompt-storm path`.
- `workspace_run_app` executed inside the hidden workspace, saw
  `DISPLAY=:90`, `AGENT_WORKSPACE_ID=default`, the workspace IPC socket, and
  `/usr/bin/xterm`.
- `workspace_launch_app --wait-window --screenshot-window` opened a real xterm
  window and returned `app-2092447`. A later title-based
  `workspace_paste_window` missed because xterm retitled itself to the shell
  cwd, while the same paste targeted by `app_id=app-2092447` succeeded.

Finding:

- Agents should prefer the returned `app_id` from launch responses for later
  window-targeted actions. Window titles are good discovery hints, but not a
  stable handle after app startup.

## 2026-05-25 MCP Task Context Pass

Environment:

- Ran against the local repository build from
  `/home/avifenesh/projects/agent-workspace-linux`.
- The pass focused on the read-only MCP planning surface, not on changing the
  runtime permission ceiling.

Verified:

- `mcp_task_plan` now returns a structured `task_context` alongside the
  existing step list and `approval_checkpoints`. The context names the
  normalized task kind, target workspace, provided inputs, missing inputs,
  safety boundaries, and approval kinds present in the plan.
- Browser/grocery plans expose missing `target_url`, `shopping_list`,
  `fulfillment`, `substitution_policy`, and `budget` as task input needs. These
  remain separate from MCP permission blockers: the clean/default MCP smoke
  still reports `mcp_permissions.configured=false`, keeps the action catalog
  advisory, and asserts that app-QA/browser plans do not invent permission
  blockers.
- Browser/grocery `task_context` now also includes structured action boundaries
  for observation, navigation/search, item comparison, cart mutation, and
  checkout/account changes. Cart changes are called out as their own approval
  class, while checkout/order/account changes remain separate real-world
  approvals.
- Live MCP control reactivation now requires `confirmed_user_request=true` when
  an MCP client switches from `read_only` or `paused` back to `active`.
  Live-control checkpoints include that exact required input, and
  `mcp_session_brief` also carries the latest control actor, timestamp, and
  reason so host UI can explain why the boundary changed.
- Viewer profile starts now follow the same boundary: clean/default usage does
  not add an MCP ceiling, while an explicit ceiling is validated before the
  viewer opens a profile-backed workspace.
- The locked-permissions MCP smoke now asserts that `task_context` also reports
  permission-ceiling, hidden-workspace, and real-world approval kinds when a
  restricted MCP ceiling actually blocks a browser-session profile.
- `cargo build --locked`, both MCP JSON-RPC smokes, `cargo test --locked`, and
  `scripts/integration_smoke.sh` passed after the change.

Finding:

- Host UI and agent loops can now render the agent's inferred user intent and
  missing grocery inputs directly from schema instead of scraping planner prose.

## 2026-05-25 GPUI Viewer Task Footer Pass

Environment:

- Ran against the local repository build and viewer unit tests.
- This pass focused on the compact GPUI monitor surface, not on adding another
  full management panel.

Verified:

- The viewer footer mode cycle now includes Activity, Task, Isolation, and Apps.
  Task mode infers app-QA, browser/shopping, observation, or stopped-workspace
  review from the selected workspace purpose/profile, active window, and app
  labels.
- The selected footer mode is now saved in `viewer.json`, so a user who leaves
  the monitor in Task, Isolation, or Apps context gets the same compact context
  when reopening the viewer. The GPUI viewer smoke seeds `footer_mode=task`
  alongside size and position preferences.
- `cargo test --locked viewer::tests` passed with coverage for the new footer
  cycle and browser/app-QA task inference labels.

Finding:

- The small floating monitor can now show what kind of work the agent appears
  to be doing without opening a larger details view, keeping it aligned with
  the gentle always-optional overlay direction.

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

## 2026-05-26 Viewer Still-Frame Pass

- The native GPUI viewer now preserves the last captured frame when the user
  pauses the screen stream while the workspace is still running. This keeps the
  floating window useful for observation without reintroducing continuous
  screenshot capture or accumulating screenshot files. Stopped workspaces still
  clear stale frames.
- Verification: `cargo fmt --check`, `cargo test --locked
  viewer::tests::paused_screen_stream_keeps_last_running_frame`, and
  `cargo test --locked viewer::tests` passed.

## 2026-05-26 Direct Live-Control Pass

- The native GPUI viewer now exposes direct live-control choices instead of one
  implicit cycling button. Active mode shows `RO` and `Pause`; read-only mode
  shows `Run` and `Pause`; paused mode shows `Run` and `RO`. This makes
  read-only, pause, and resume visible in the small floating control surface
  while preserving the MCP boundary semantics and avoiding a crowded header.
- The viewer minimum width is now 380px, and window dragging starts from the
  title area rather than the root surface. The focused GPUI smoke caught the
  old 360px clipping of the `Stop` control and now verifies the native
  X11/Xwayland viewer renders at 380x340 without requesting always-on-top state
  by default.
- Verification: `cargo test --locked viewer::tests`,
  `cargo test --locked control::tests`, `cargo clippy --locked -- -D warnings`,
  and `scripts/gpui_viewer_smoke.sh` passed.
