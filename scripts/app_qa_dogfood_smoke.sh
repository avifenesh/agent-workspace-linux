#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${AGENT_WORKSPACE_BIN:-${BIN:-$ROOT_DIR/target/debug/agent-workspace-linux}}"
OUTPUT_DIR="${OUTPUT_DIR:-$ROOT_DIR/target/app-qa-dogfood}"
DESKTOP_REPO="${CODEX_DESKTOP_LINUX_REPO:-$ROOT_DIR/../codex-desktop-linux}"

need() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required command: $1" >&2
    exit 1
  fi
}

need python3

validate_bundle_source_manifest() {
  [[ -n "${AGENT_WORKSPACE_RELEASE_BUNDLE_MANIFEST:-}" ]] || return 0
  ROOT_DIR="$ROOT_DIR" DESKTOP_REPO="$DESKTOP_REPO" python3 - <<'PY'
import json
import os
import sys
from pathlib import Path

ROOT = Path(os.environ["ROOT_DIR"])
DESKTOP_REPO = Path(os.environ["DESKTOP_REPO"])
manifest_path = Path(os.environ["AGENT_WORKSPACE_RELEASE_BUNDLE_MANIFEST"])
sys.dont_write_bytecode = True
sys.path.insert(0, str(ROOT / "scripts"))
from release_gate_audit import validate_bundle_manifest_source_contents

validate_bundle_manifest_source_contents(
    json.loads(manifest_path.read_text(encoding="utf-8")),
    root=ROOT,
    desktop_repo=DESKTOP_REPO,
)
PY
}

validate_bundle_source_manifest
need jq
need xmessage

mkdir -p "$OUTPUT_DIR"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
REPORT_PATH="$OUTPUT_DIR/${STAMP}.json"
SMOKE_DIR="$(mktemp -d)"
CONFIG_DIR="$SMOKE_DIR/config"
RUNTIME_DIR="$SMOKE_DIR/runtime"
WORKSPACE_ID="app-qa-dogfood-$$"
mkdir -p "$CONFIG_DIR" "$RUNTIME_DIR"

run_awl() {
  XDG_CONFIG_HOME="$CONFIG_DIR" XDG_RUNTIME_DIR="$RUNTIME_DIR" "$BIN" "$@"
}

cleanup() {
  exit_code=$?
  run_awl workspace stop --id "$WORKSPACE_ID" --timeout-ms 10000 >/dev/null 2>&1 || true
  if [[ "$exit_code" -eq 0 ]]; then
    rm -rf "$SMOKE_DIR"
  else
    echo "app QA dogfood smoke failed; preserved temp dir: $SMOKE_DIR" >&2
  fi
}
trap cleanup EXIT

assert_json() {
  local filter="$1"
  local file="$2"
  shift 2
  jq -e "$@" "$filter" "$file" >/dev/null
}

run_awl workspace start \
  --ack-hidden-workspace \
  --id "$WORKSPACE_ID" \
  --purpose "App QA dogfood smoke" \
  --width 900 \
  --height 640 \
  > "$SMOKE_DIR/start.json"
assert_json '.ok == true and .status.ready == true' "$SMOKE_DIR/start.json"

run_awl workspace launch \
  --id "$WORKSPACE_ID" \
  --name app-qa-target \
  --wait-window \
  --screenshot-window \
  --window-timeout-ms 10000 \
  -- xmessage -name app-qa-target -title "App QA Dogfood Target" -buttons "QA complete:0" "Agent workspace app QA dogfood target" \
  > "$SMOKE_DIR/launch.json"
assert_json '.ok == true and (.windows | length) >= 1 and (.screenshot.bytes > 0) and .apps[0].running == true' "$SMOKE_DIR/launch.json"
APP_ID="$(jq -r '.apps[0].id' "$SMOKE_DIR/launch.json")"

run_awl workspace observe \
  --id "$WORKSPACE_ID" \
  --screenshot \
  --all-windows \
  --events \
  --events-tail 40 \
  > "$SMOKE_DIR/observe.json"
assert_json '(.screenshot.bytes > 0) and (.active_window.title | contains("App QA Dogfood Target")) and (.events | length > 0)' "$SMOKE_DIR/observe.json"

run_awl workspace logs \
  --id "$WORKSPACE_ID" \
  --stream stdout \
  "$APP_ID" \
  > "$SMOKE_DIR/logs.json"
assert_json '.ok == true and .app_log.stream == "stdout" and (.app_log.path | endswith(".stdout.log"))' "$SMOKE_DIR/logs.json"

run_awl workspace artifacts \
  --id "$WORKSPACE_ID" \
  --existing \
  > "$SMOKE_DIR/artifacts.json"
assert_json '.ok == true and any(.files[]; .kind == "event_log" and .exists == true)' "$SMOKE_DIR/artifacts.json"

run_awl workspace stop --id "$WORKSPACE_ID" --timeout-ms 10000 > "$SMOKE_DIR/stop.json"
assert_json '.ok == true and (.apps[] | select(.name == "app-qa-target" and .running == false))' "$SMOKE_DIR/stop.json"

export REPORT_PATH SMOKE_DIR ROOT_DIR DESKTOP_REPO WORKSPACE_ID APP_ID BIN
python3 - <<'PY'
import datetime as dt
import json
import os
import sys
from pathlib import Path

ROOT = Path(os.environ["ROOT_DIR"])
DESKTOP_REPO = Path(os.environ["DESKTOP_REPO"])
SMOKE_DIR = Path(os.environ["SMOKE_DIR"])
REPORT_PATH = Path(os.environ["REPORT_PATH"])
sys.dont_write_bytecode = True
sys.path.insert(0, str(ROOT / "scripts"))
from release_gate_audit import compute_source_identity
from release_gate_audit import validate_bundle_manifest_source_contents


def read_json(name):
    return json.loads((SMOKE_DIR / name).read_text(encoding="utf-8"))


def source_identity():
    manifest_path = os.environ.get("AGENT_WORKSPACE_RELEASE_BUNDLE_MANIFEST")
    if manifest_path:
        manifest = json.loads(Path(manifest_path).read_text(encoding="utf-8"))
        validate_bundle_manifest_source_contents(manifest, root=ROOT, desktop_repo=DESKTOP_REPO)
        identity = manifest.get("source_identity")
        if not isinstance(identity, dict) or not identity.get("source_hash"):
            raise RuntimeError(
                f"release bundle manifest does not contain source_identity: {manifest_path}"
            )
        return identity
    return compute_source_identity(ROOT, desktop_repo=DESKTOP_REPO)


launch = read_json("launch.json")
observe = read_json("observe.json")
logs = read_json("logs.json")
artifacts = read_json("artifacts.json")
stop = read_json("stop.json")
report = {
    "schema": "agent-workspace-linux.app_qa_dogfood.v1",
    "created_at_utc": dt.datetime.now(dt.timezone.utc).isoformat(),
    "source_identity": source_identity(),
    "evidence_boundary": {
        "collector": "agent-workspace-linux",
        "collector_script": "scripts/app_qa_dogfood_smoke.sh",
        "repo_owned_runtime": True,
        "codex_app_mcp_used": False,
        "computer_use_mcp_used": False,
        "codex_desktop_bridge_used": False,
        "playwright_mcp_used": False,
        "runtime_entrypoint": os.environ["BIN"],
    },
    "mode": "local-gui-app",
    "status": "passed",
    "inputs": {
        "task_intent": "app_qa",
        "target_app": "xmessage",
        "target_app_command": [
            "xmessage",
            "-name",
            "app-qa-target",
            "-title",
            "App QA Dogfood Target",
        ],
        "real_world_action_approved": False,
    },
    "safety_contract": {
        "hidden_workspace_acknowledged": True,
        "app_qa_only": True,
        "host_desktop_input_targeted": False,
        "real_world_or_account_mutation": False,
        "non_destructive_input_only": True,
    },
    "workspace": {
        "status": "passed",
        "workspace_id": os.environ["WORKSPACE_ID"],
        "app_id": os.environ["APP_ID"],
        "launch_ok": launch.get("ok") is True,
        "launch_window_count": len(launch.get("windows") or []),
        "launch_screenshot_bytes": ((launch.get("screenshot") or {}).get("bytes") or 0),
        "observe_screenshot_bytes": ((observe.get("screenshot") or {}).get("bytes") or 0),
        "active_window_title": ((observe.get("active_window") or {}).get("title") or ""),
        "event_count": len(observe.get("events") or []),
        "logs_ok": logs.get("ok") is True,
        "event_log_artifact_present": any(
            item.get("kind") == "event_log" and item.get("exists") is True
            for item in artifacts.get("files") or []
        ),
        "stopped_by_workspace_stop": stop.get("ok") is True
        and any(
            app.get("name") == "app-qa-target" and app.get("running") is False
            for app in stop.get("apps") or []
        ),
        "stop_ok": stop.get("ok") is True,
    },
}
REPORT_PATH.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY

echo "app QA dogfood report: $REPORT_PATH"
echo "app QA dogfood smoke passed"
