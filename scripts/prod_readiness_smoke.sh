#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DESKTOP_REPO="${CODEX_DESKTOP_LINUX_REPO:-$ROOT_DIR/../codex-desktop-linux}"
REQUIRE_GUI_SMOKE="${REQUIRE_GUI_SMOKE:-0}"
REQUIRE_DESKTOP_SMOKE="${REQUIRE_DESKTOP_SMOKE:-0}"
REPORT_RETENTION="${AGENT_WORKSPACE_REPORT_RETENTION:-25}"
NO_NEW_VIEWER="${AGENT_WORKSPACE_NO_NEW_VIEWER:-0}"

export PYTHONDONTWRITEBYTECODE=1

need() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required command: $1" >&2
    exit 1
  fi
}

have_all() {
  local command_name
  for command_name in "$@"; do
    command -v "$command_name" >/dev/null 2>&1 || return 1
  done
}

run() {
  echo "== $* =="
  "$@"
}

check_python_syntax() {
  local script_path
  for script_path in "$@"; do
    echo "== python syntax $script_path =="
    python3 - "$script_path" <<'PY'
import pathlib
import sys

path = pathlib.Path(sys.argv[1])
compile(path.read_text(encoding="utf-8"), str(path), "exec")
PY
  done
}

need cargo
need git
need jq
need node
need python3

cd "$ROOT_DIR"

run cargo fmt --check
run cargo build --locked
run cargo clippy --locked -- -D warnings
run cargo test --locked

check_python_syntax \
  scripts/release_gate_audit.py \
  scripts/final_review_bundle.py \
  scripts/import_release_evidence.py \
  scripts/export_release_evidence_bundle.py \
  scripts/release_next_steps.py \
  scripts/create_human_review_marker.py \
  scripts/objective_completion_audit.py \
  scripts/prune_evidence_reports.py
run scripts/release_gate_audit.py --self-test
run scripts/final_review_bundle.py --self-test
run scripts/import_release_evidence.py --self-test
run scripts/export_release_evidence_bundle.py --self-test
run scripts/release_next_steps.py --self-test
run scripts/create_human_review_marker.py --self-test
run scripts/objective_completion_audit.py --self-test
run scripts/prune_evidence_reports.py --self-test
run node --check scripts/github_explore_dogfood_probe.js
run node --check scripts/prepare_grocery_profile_copy.js
run node --check scripts/mcp_clean_permissions_smoke.js
run node --check scripts/mcp_permissions_smoke.js
run node --check scripts/mcp_non_headless_viewer_smoke.js
run node --check scripts/mcp_no_host_display_viewer_smoke.js
run node --check scripts/mcp_viewer_lifecycle_smoke.js
run node --check scripts/lib/chrome_cdp.js
run node --check scripts/mcp_workspace_browser_cdp_smoke.js
run node --check scripts/real_grocery_dogfood_probe.js
run node scripts/github_explore_dogfood_probe.js --self-test
run node scripts/prepare_grocery_profile_copy.js --self-test
run node scripts/real_grocery_dogfood_probe.js --self-test
echo "== viewer native Wayland claim validation rejects X11 spoof =="
if env \
  RUN_VIEWER_SMOKE=0 \
  REQUIRE_VIEWER_SMOKE=0 \
  XDG_SESSION_TYPE=x11 \
  XDG_CURRENT_DESKTOP=KDE \
  NATIVE_WAYLAND_LAYER_SHELL_OBSERVED=1 \
  NATIVE_WAYLAND_LAYER_SHELL_NOTES="Observed layer-shell top-layer behavior." \
  OUTPUT_DIR="$ROOT_DIR/target/viewer-native-validation-self-test" \
  scripts/viewer_desktop_matrix_probe.sh >/tmp/agent-workspace-viewer-native-x11-spoof.log 2>&1; then
  echo "viewer native Wayland claim validation accepted an X11 spoof" >&2
  exit 1
fi
echo "== viewer native Wayland claim validation rejects negative notes =="
if env \
  RUN_VIEWER_SMOKE=0 \
  REQUIRE_VIEWER_SMOKE=0 \
  XDG_SESSION_TYPE=wayland \
  XDG_CURRENT_DESKTOP=KDE \
  NATIVE_WAYLAND_LAYER_SHELL_OBSERVED=1 \
  NATIVE_WAYLAND_LAYER_SHELL_NOTES="Observed a normal resizable Xwayland toplevel, not layer-shell." \
  OUTPUT_DIR="$ROOT_DIR/target/viewer-native-validation-self-test" \
  scripts/viewer_desktop_matrix_probe.sh >/tmp/agent-workspace-viewer-native-negative-notes.log 2>&1; then
  echo "viewer native Wayland claim validation accepted negative notes" >&2
  exit 1
fi

run node scripts/mcp_clean_permissions_smoke.js
run node scripts/mcp_permissions_smoke.js
run node scripts/mcp_non_headless_viewer_smoke.js
run node scripts/mcp_no_host_display_viewer_smoke.js
if [[ "$NO_NEW_VIEWER" == "1" ]]; then
  echo "== node scripts/mcp_viewer_lifecycle_smoke.js =="
  echo "skipped: AGENT_WORKSPACE_NO_NEW_VIEWER=1 keeps the existing visible GPUI viewer undisturbed"
else
  run node scripts/mcp_viewer_lifecycle_smoke.js
fi
run node scripts/mcp_workspace_browser_cdp_smoke.js

run scripts/app_qa_dogfood_smoke.sh
if [[ "$NO_NEW_VIEWER" == "1" ]]; then
  run env GITHUB_EXPLORE_OPEN_VIEWER=0 scripts/github_explore_dogfood_probe.js
else
  run scripts/github_explore_dogfood_probe.js
fi
run scripts/grocery_browser_workflow_smoke.sh
run scripts/integration_smoke.sh

if [[ "$NO_NEW_VIEWER" == "1" ]]; then
  run env RUN_VIEWER_SMOKE=0 REQUIRE_VIEWER_SMOKE=0 scripts/viewer_desktop_matrix_probe.sh
elif [[ -n "${DISPLAY:-}" ]] && have_all xclock xdotool xwininfo xprop xwd convert identify; then
  run env REQUIRE_VIEWER_SMOKE=1 scripts/viewer_desktop_matrix_probe.sh
else
  message="viewer desktop matrix probe skipped: needs DISPLAY plus xclock, xdotool, xwininfo, xprop, xwd, convert, and identify"
  if [[ "$REQUIRE_GUI_SMOKE" == "1" ]]; then
    echo "$message" >&2
    exit 1
  fi
  echo "$message"
fi

run git diff --check

if [[ -f "$DESKTOP_REPO/linux-features/agent-workspace/test.js" ]]; then
  (
    cd "$DESKTOP_REPO"
    run node --check linux-features/agent-workspace/patch.js
    run node --test linux-features/agent-workspace/test.js
    run git diff --check
  )
else
  message="Codex Desktop agent-workspace tests skipped: $DESKTOP_REPO not found"
  if [[ "$REQUIRE_DESKTOP_SMOKE" == "1" ]]; then
    echo "$message" >&2
    exit 1
  fi
  echo "$message"
fi

if [[ "${REQUIRE_RELEASE_GATES:-0}" == "1" ]]; then
  run scripts/release_gate_audit.py --require-all --require-clean-source
else
  run scripts/release_gate_audit.py
fi

run scripts/export_release_evidence_bundle.py
run scripts/final_review_bundle.py
run scripts/release_next_steps.py

run python3 - "$ROOT_DIR" "$DESKTOP_REPO" <<'PY'
import datetime as dt
import json
import os
import sys
from pathlib import Path

root = Path(sys.argv[1])
desktop_repo = Path(sys.argv[2])
sys.dont_write_bytecode = True
sys.path.insert(0, str(root / "scripts"))

from release_gate_audit import compute_review_scope_identity
from release_gate_audit import compute_source_identity


def latest_file(directory: Path, pattern: str) -> str | None:
    if not directory.exists():
        return None
    files = sorted(directory.glob(pattern))
    return str(files[-1]) if files else None


now = dt.datetime.now(dt.timezone.utc)
no_new_viewer = os.environ.get("AGENT_WORKSPACE_NO_NEW_VIEWER") == "1"
output_dir = root / "target" / "prod-readiness-smoke"
output_dir.mkdir(parents=True, exist_ok=True)
report_path = output_dir / f"{now.strftime('%Y%m%dT%H%M%SZ')}.json"
completed_check_ids = [
    "cargo_fmt",
    "cargo_build",
    "cargo_clippy",
    "cargo_test",
    "python_syntax",
    "release_gate_audit_self_test",
    "final_review_bundle_self_test",
    "import_release_evidence_self_test",
    "export_release_evidence_bundle_self_test",
    "release_next_steps_self_test",
    "create_human_review_marker_self_test",
    "objective_completion_audit_self_test",
    "prune_evidence_reports_self_test",
    "github_explore_dogfood_self_test",
    "grocery_profile_copy_self_test",
    "real_grocery_probe_self_test",
    "mcp_clean_permissions",
    "mcp_permissions",
    "mcp_non_headless_viewer",
    "mcp_no_host_display_viewer",
    "direct_mcp_workspace_browser_cdp",
    "app_qa_dogfood",
    "github_explore_dogfood",
    "grocery_browser_workflow",
    "integration_smoke",
    "git_diff_check",
    "codex_desktop_agent_workspace_tests",
    "release_gate_audit",
    "release_evidence_source_bundle",
    "final_review_bundle",
    "release_next_steps",
]
if no_new_viewer:
    completed_check_ids.append("direct_mcp_viewer_lifecycle_skipped_no_new_viewer")
    completed_check_ids.append("viewer_desktop_matrix_metadata_only")
else:
    completed_check_ids.append("direct_mcp_viewer_lifecycle")
    completed_check_ids.append("viewer_desktop_matrix_visual_smoke")
report = {
    "schema": "agent-workspace-linux.prod_readiness_smoke.v1",
    "created_at_utc": now.isoformat(),
    "status": "passed",
    "completed_check_ids": completed_check_ids,
    "visible_viewer_smoke": {
        "mode": "metadata-only-no-new-viewer" if no_new_viewer else "visual-smoke-required",
        "no_new_viewer": no_new_viewer,
        "note": "AGENT_WORKSPACE_NO_NEW_VIEWER=1 skips viewer-spawning lifecycle and visual-smoke checks so an existing live viewer is not disturbed."
        if no_new_viewer
        else "Visual viewer lifecycle and GPUI smoke checks were allowed to launch temporary viewers.",
    },
    "source_identity": compute_source_identity(root, desktop_repo=desktop_repo),
    "review_scope_identity": compute_review_scope_identity(root, desktop_repo=desktop_repo),
    "evidence": {
        "release_gate_audit": latest_file(root / "target" / "release-gate-audit", "*.json"),
        "final_review_bundle": latest_file(root / "target" / "final-review-bundle", "*.json"),
        "source_bundle": latest_file(root / "target" / "release-evidence-source-bundle", "*.tar.gz"),
        "app_qa_dogfood": latest_file(root / "target" / "app-qa-dogfood", "*.json"),
        "viewer_desktop_matrix": latest_file(root / "target" / "viewer-desktop-matrix", "*.json"),
        "github_explore_dogfood": latest_file(root / "target" / "github-explore-dogfood", "*.json"),
    },
}
report_path.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
print(f"prod readiness smoke report: {report_path}")
PY

if [[ "${REQUIRE_RELEASE_GATES:-0}" == "1" ]]; then
  run scripts/objective_completion_audit.py --require-complete
else
  run scripts/objective_completion_audit.py
fi

if [[ "$REPORT_RETENTION" == "0" ]]; then
  echo "evidence report retention skipped: AGENT_WORKSPACE_REPORT_RETENTION=0"
else
  run scripts/prune_evidence_reports.py --keep "$REPORT_RETENTION"
fi

python3 - "$ROOT_DIR" <<'PY'
import json
import sys
from pathlib import Path

root = Path(sys.argv[1])
reports = sorted((root / "target" / "release-gate-audit").glob("*.json"))
print("prod readiness smoke passed")
if not reports:
    print("\nRelease gate summary unavailable: no release-gate audit report found.")
    raise SystemExit(0)

report = json.loads(reports[-1].read_text(encoding="utf-8"))
pending = [
    gate
    for gate in report.get("gates", [])
    if gate.get("status") != "passed"
]
if not pending:
    print("\nAll release gates passed.")
else:
    print("\nRelease gates still pending:")
    for gate in pending:
        missing = "; ".join(str(item) for item in gate.get("missing") or [])
        print(f"- {gate.get('id')}: {missing}")
PY
