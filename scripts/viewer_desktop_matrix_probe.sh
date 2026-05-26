#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUTPUT_DIR="${OUTPUT_DIR:-$ROOT_DIR/target/viewer-desktop-matrix}"
DESKTOP_REPO="${CODEX_DESKTOP_LINUX_REPO:-$ROOT_DIR/../codex-desktop-linux}"
RUN_VIEWER_SMOKE="${RUN_VIEWER_SMOKE:-1}"
REQUIRE_VIEWER_SMOKE="${REQUIRE_VIEWER_SMOKE:-0}"

fail() {
  echo "$*" >&2
  exit 1
}

need() {
  if ! command -v "$1" >/dev/null 2>&1; then
    fail "missing required command: $1"
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

validate_native_wayland_claim() {
  [[ "${NATIVE_WAYLAND_LAYER_SHELL_OBSERVED:-0}" == "1" ]] || return 0

  local notes="${NATIVE_WAYLAND_LAYER_SHELL_NOTES:-}"
  local notes_lower="${notes,,}"
  local session_type="${XDG_SESSION_TYPE:-}"
  local session_type_lower="${session_type,,}"
  local desktop_labels="${XDG_CURRENT_DESKTOP:-} ${DESKTOP_SESSION:-}"
  local desktop_labels_lower="${desktop_labels,,}"
  local viewer_backend="${AGENT_WORKSPACE_VIEWER_BACKEND:-}"
  local viewer_backend_lower="${viewer_backend,,}"

  [[ -n "$notes" ]] || fail "NATIVE_WAYLAND_LAYER_SHELL_OBSERVED=1 requires NATIVE_WAYLAND_LAYER_SHELL_NOTES"
  [[ "$session_type_lower" == "wayland" ]] \
    || fail "NATIVE_WAYLAND_LAYER_SHELL_OBSERVED=1 must be collected from XDG_SESSION_TYPE=wayland, got ${session_type:-<unset>}"
  [[ "$desktop_labels_lower" != *gnome* ]] \
    || fail "NATIVE_WAYLAND_LAYER_SHELL_OBSERVED=1 is not accepted from GNOME/Xwayland fallback sessions"
  [[ "$viewer_backend_lower" != *x11* && "$viewer_backend_lower" != *xwayland* ]] \
    || fail "NATIVE_WAYLAND_LAYER_SHELL_OBSERVED=1 must not force an X11/Xwayland viewer backend"
  [[ "$notes_lower" =~ (layer-shell|layer[[:space:]]shell|top-layer|top[[:space:]]layer|overlay[[:space:]]layer|layer::overlay) ]] \
    || fail "NATIVE_WAYLAND_LAYER_SHELL_NOTES must make a positive layer-shell/top-layer claim"
  [[ ! "$notes_lower" =~ (not[[:space:]-]+layer[[:space:]-]+shell|not[[:space:]]+a[[:space:]]+compositor[[:space:]]+layer|normal[[:space:]]+resizable[[:space:]]+xwayland|xwayland[[:space:]]+toplevel|x11/xwayland|x11[[:space:]]+wm[[:space:]]+state|x11[[:space:]]+window) ]] \
    || fail "NATIVE_WAYLAND_LAYER_SHELL_NOTES describes an X11/Xwayland or negative layer-shell observation"
}

validate_bundle_source_manifest
validate_native_wayland_claim

mkdir -p "$OUTPUT_DIR"
STAMP="$(date -u +%Y%m%dT%H%M%SZ)"
REPORT_PATH="$OUTPUT_DIR/${STAMP}.json"
SMOKE_LOG_PATH="$OUTPUT_DIR/${STAMP}-gpui-viewer-smoke.log"
SMOKE_SUMMARY_PATH="$OUTPUT_DIR/${STAMP}-gpui-viewer-smoke-summary.json"
VIEWER_SMOKE_TOOLS=(xclock xdotool xwininfo xprop xwd convert identify)
MISSING_TOOLS=()

for tool in "${VIEWER_SMOKE_TOOLS[@]}"; do
  if ! command -v "$tool" >/dev/null 2>&1; then
    MISSING_TOOLS+=("$tool")
  fi
done

SMOKE_STATUS="skipped"
SMOKE_REASON=""
SMOKE_EXIT_CODE=""

if [[ "$RUN_VIEWER_SMOKE" == "1" ]]; then
  if [[ -z "${DISPLAY:-}" ]]; then
    SMOKE_REASON="DISPLAY is not set"
  elif [[ "${#MISSING_TOOLS[@]}" -gt 0 ]]; then
    SMOKE_REASON="missing viewer smoke tools: ${MISSING_TOOLS[*]}"
  else
    set +e
    VIEWER_SMOKE_SUMMARY_PATH="$SMOKE_SUMMARY_PATH" "$ROOT_DIR/scripts/gpui_viewer_smoke.sh" >"$SMOKE_LOG_PATH" 2>&1
    SMOKE_EXIT_CODE="$?"
    set -e
    if [[ "$SMOKE_EXIT_CODE" == "0" ]]; then
      SMOKE_STATUS="passed"
    else
      SMOKE_STATUS="failed"
      SMOKE_REASON="scripts/gpui_viewer_smoke.sh failed; see $SMOKE_LOG_PATH"
    fi
  fi
else
  SMOKE_REASON="RUN_VIEWER_SMOKE=0"
fi

if [[ "$SMOKE_STATUS" == "skipped" && "$REQUIRE_VIEWER_SMOKE" == "1" ]]; then
  SMOKE_STATUS="failed"
  SMOKE_EXIT_CODE="1"
  if [[ -z "$SMOKE_REASON" ]]; then
    SMOKE_REASON="viewer smoke was required but did not run"
  fi
fi

MISSING_COMMANDS="$(printf '%s\n' "${MISSING_TOOLS[@]}")"
export REPORT_PATH SMOKE_LOG_PATH SMOKE_SUMMARY_PATH SMOKE_STATUS SMOKE_REASON SMOKE_EXIT_CODE MISSING_COMMANDS ROOT_DIR DESKTOP_REPO

python3 - <<'PY'
import datetime
import json
import os
import platform
import re
import shutil
import socket
import subprocess
import sys
from pathlib import Path

ROOT = Path(os.environ["ROOT_DIR"])
DESKTOP_REPO = Path(os.environ["DESKTOP_REPO"])
REPORT = Path(os.environ["REPORT_PATH"])
SMOKE_LOG = Path(os.environ["SMOKE_LOG_PATH"])
SMOKE_SUMMARY = Path(os.environ["SMOKE_SUMMARY_PATH"])
SMOKE_TOOLS = ["xclock", "xdotool", "xwininfo", "xprop", "xwd", "convert", "identify"]
sys.dont_write_bytecode = True
sys.path.insert(0, str(ROOT / "scripts"))
from release_gate_audit import compute_source_identity
from release_gate_audit import validate_bundle_manifest_source_contents


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


def run_command(command, timeout=3):
    try:
        completed = subprocess.run(
            command,
            check=False,
            capture_output=True,
            text=True,
            timeout=timeout,
        )
        return {
            "ok": completed.returncode == 0,
            "exit_code": completed.returncode,
            "stdout": completed.stdout.strip(),
            "stderr": completed.stderr.strip(),
        }
    except Exception as error:
        return {"ok": False, "error": str(error)}


def parse_os_release():
    path = Path("/etc/os-release")
    data = {}
    if not path.exists():
        return data
    for line in path.read_text(encoding="utf-8", errors="replace").splitlines():
        if "=" not in line or line.startswith("#"):
            continue
        key, value = line.split("=", 1)
        data[key] = value.strip().strip('"')
    return data


def loginctl_session():
    session = os.environ.get("XDG_SESSION_ID")
    if not session or not shutil.which("loginctl"):
        return None
    result = run_command(
        [
            "loginctl",
            "show-session",
            session,
            "--property=Type",
            "--property=Desktop",
            "--property=Name",
            "--property=Class",
            "--property=Remote",
            "--property=State",
        ]
    )
    properties = {}
    if result.get("ok"):
        for line in result.get("stdout", "").splitlines():
            if "=" in line:
                key, value = line.split("=", 1)
                properties[key] = value
    return {"id": session, "result": result, "properties": properties}


def command_map(commands):
    return {
        command: {
            "available": shutil.which(command) is not None,
            "path": shutil.which(command),
        }
        for command in commands
    }


def load_viewer_smoke_summary():
    if not SMOKE_SUMMARY.exists():
        return None
    try:
        return json.loads(SMOKE_SUMMARY.read_text(encoding="utf-8"))
    except Exception as error:
        return {"error": str(error), "path": str(SMOKE_SUMMARY)}


def optional_x11_probe():
    if not os.environ.get("DISPLAY") or not shutil.which("xprop"):
        return None
    return {
        "root_window": run_command(
            [
                "xprop",
                "-root",
                "_NET_SUPPORTING_WM_CHECK",
                "_NET_CURRENT_DESKTOP",
                "_NET_DESKTOP_NAMES",
            ]
        )
    }


def session_consistency(loginctl):
    problems = []
    warnings = []
    env_session_type = (os.environ.get("XDG_SESSION_TYPE") or "").strip().lower()
    env_desktop_text = " ".join(
        value
        for value in [
            os.environ.get("XDG_CURRENT_DESKTOP"),
            os.environ.get("DESKTOP_SESSION"),
        ]
        if value
    ).lower()
    properties = (loginctl or {}).get("properties") or {}
    loginctl_type = (properties.get("Type") or "").strip().lower()
    loginctl_desktop = (properties.get("Desktop") or "").strip().lower()

    if loginctl_type and env_session_type and loginctl_type != env_session_type:
        problems.append(
            f"XDG_SESSION_TYPE={env_session_type!r} conflicts with loginctl Type={loginctl_type!r}"
        )
    if loginctl_desktop and env_desktop_text and loginctl_desktop not in env_desktop_text:
        warnings.append(
            f"loginctl Desktop={loginctl_desktop!r} is not present in XDG desktop labels {env_desktop_text!r}"
        )
    if not loginctl_type:
        warnings.append("loginctl session Type was unavailable; relying on environment session type")
    if not loginctl_desktop:
        warnings.append("loginctl Desktop was unavailable; relying on environment desktop labels")

    return {
        "release_eligible": not problems,
        "problems": problems,
        "warnings": warnings,
        "session_type_source": "loginctl" if loginctl_type else "environment",
        "desktop_source": "loginctl+environment" if loginctl_desktop else "environment",
        "loginctl_type": loginctl_type or None,
        "loginctl_desktop": loginctl_desktop or None,
    }


def display_socket_candidates():
    candidates = []
    xdg_runtime_dir = os.environ.get("XDG_RUNTIME_DIR")
    wayland_display = os.environ.get("WAYLAND_DISPLAY")
    if wayland_display:
        wayland_path = Path(wayland_display)
        if not wayland_path.is_absolute() and xdg_runtime_dir:
            wayland_path = Path(xdg_runtime_dir) / wayland_display
        candidates.append({"kind": "wayland", "path": str(wayland_path)})

    display = os.environ.get("DISPLAY") or ""
    if display:
        match = re.match(r"^(?:(?P<host>[^:]*):)?(?P<number>\d+)(?:\.\d+)?$", display)
        if match:
            host = match.group("host") or ""
            candidates.append(
                {
                    "kind": "x11",
                    "display": display,
                    "host": host,
                    "path": f"/tmp/.X11-unix/X{match.group('number')}",
                }
            )
        else:
            candidates.append({"kind": "x11", "display": display, "path": None})
    return candidates


def process_args(pid):
    result = run_command(["ps", "-p", str(pid), "-o", "args="])
    return result.get("stdout", "") if result.get("ok") else ""


def lsof_processes(path):
    if not path or not shutil.which("lsof"):
        return []
    result = run_command(["lsof", path], timeout=5)
    if not result.get("ok"):
        return []
    processes = []
    seen_pids = set()
    for line in result.get("stdout", "").splitlines()[1:]:
        parts = line.split()
        if len(parts) < 2 or not parts[1].isdigit():
            continue
        pid = int(parts[1])
        if pid in seen_pids:
            continue
        seen_pids.add(pid)
        processes.append(
            {
                "command": parts[0],
                "pid": pid,
                "args": process_args(pid),
            }
        )
    return processes


def display_attestation(loginctl):
    problems = []
    warnings = []
    candidates = display_socket_candidates()
    if not candidates:
        problems.append("no host display socket was discoverable")

    known_nested_or_headless = []
    socket_reports = []
    bad_process_re = re.compile(
        r"\b(xvfb|xvfb-run|xvnc|xpra|xephyr|x11vnc|xorg\.dummy)\b",
        re.IGNORECASE,
    )
    for candidate in candidates:
        path_value = candidate.get("path")
        exists = bool(path_value and Path(path_value).exists())
        if candidate.get("kind") == "x11" and candidate.get("host") not in {"", "unix", "localhost"}:
            problems.append(f"DISPLAY={candidate.get('display')!r} appears to use remote X forwarding")
        if path_value and not exists:
            warnings.append(f"{candidate.get('kind')} display socket was not found at {path_value}")
        processes = lsof_processes(path_value) if exists else []
        if exists and shutil.which("lsof") and not processes:
            problems.append(f"no display server process was found for {path_value}")
        for process in processes:
            command_text = f"{process.get('command') or ''} {process.get('args') or ''}"
            if bad_process_re.search(command_text) or (
                "weston" in command_text.lower() and "headless" in command_text.lower()
            ):
                known_nested_or_headless.append(process)
        socket_reports.append({**candidate, "exists": exists, "processes": processes})

    if not shutil.which("lsof"):
        problems.append("lsof is required for release display-server process attestation")
    if known_nested_or_headless:
        problems.append("display server appears nested or headless")

    return {
        "release_eligible": not problems,
        "problems": problems,
        "warnings": warnings,
        "display_protocols": sorted({candidate["kind"] for candidate in candidates}),
        "sockets": socket_reports,
        "known_nested_or_headless_processes": known_nested_or_headless,
        "loginctl_session": (loginctl or {}).get("id"),
        "lsof_available": shutil.which("lsof") is not None,
    }


loginctl = loginctl_session()
consistency = session_consistency(loginctl)
display = display_attestation(loginctl)
viewer_smoke_summary = load_viewer_smoke_summary()
desktop_label = " / ".join(
    value
    for value in [
        os.environ.get("XDG_CURRENT_DESKTOP"),
        os.environ.get("XDG_SESSION_TYPE"),
        os.environ.get("DESKTOP_SESSION"),
    ]
    if value
) or "unknown"

report = {
    "schema": "agent-workspace-linux.viewer_desktop_matrix.v1",
    "created_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
    "repo": str(ROOT),
    "source_identity": source_identity(),
    "evidence_boundary": {
        "collector": "agent-workspace-linux",
        "collector_script": "scripts/viewer_desktop_matrix_probe.sh",
        "repo_owned_runtime": True,
        "codex_app_mcp_used": False,
        "computer_use_mcp_used": False,
        "codex_desktop_bridge_used": False,
        "playwright_mcp_used": False,
        "viewer_smoke_script": "scripts/gpui_viewer_smoke.sh",
    },
    "host": {
        "hostname": socket.gethostname(),
        "kernel": platform.release(),
        "platform": platform.platform(),
        "machine": platform.machine(),
        "os_release": parse_os_release(),
    },
    "session": {
        "xdg_session_type": os.environ.get("XDG_SESSION_TYPE"),
        "xdg_current_desktop": os.environ.get("XDG_CURRENT_DESKTOP"),
        "desktop_session": os.environ.get("DESKTOP_SESSION"),
        "wayland_display": os.environ.get("WAYLAND_DISPLAY"),
        "display": os.environ.get("DISPLAY"),
        "gdk_backend": os.environ.get("GDK_BACKEND"),
        "qt_qpa_platform": os.environ.get("QT_QPA_PLATFORM"),
        "agent_workspace_viewer_backend": os.environ.get("AGENT_WORKSPACE_VIEWER_BACKEND"),
        "loginctl": loginctl,
        "x11": optional_x11_probe(),
    },
    "commands": command_map(
        [
            "cargo",
            "jq",
            "node",
            "python3",
            "loginctl",
            "xprop",
            "lsof",
            *SMOKE_TOOLS,
        ]
    ),
    "viewer_smoke": {
        "status": os.environ["SMOKE_STATUS"],
        "reason": os.environ.get("SMOKE_REASON") or None,
        "exit_code": int(os.environ["SMOKE_EXIT_CODE"])
        if os.environ.get("SMOKE_EXIT_CODE")
        else None,
        "log_path": str(SMOKE_LOG) if SMOKE_LOG.exists() else None,
        "summary_path": str(SMOKE_SUMMARY) if SMOKE_SUMMARY.exists() else None,
        "summary": viewer_smoke_summary,
        "missing_tools": [
            item
            for item in os.environ.get("MISSING_COMMANDS", "").splitlines()
            if item
        ],
    },
    "matrix_result": {
        "desktop_label": desktop_label,
        "counts_for_release_matrix": os.environ["SMOKE_STATUS"] == "passed" and consistency["release_eligible"] and display["release_eligible"],
        "session_consistency": consistency,
        "display_attestation": display,
        "x11_xwayland_viewer_protocol_observed": bool(
            isinstance(viewer_smoke_summary, dict)
            and viewer_smoke_summary.get("x11_xwayland_window_observed") is True
        ),
        "native_wayland_layer_shell_observed": os.environ.get("NATIVE_WAYLAND_LAYER_SHELL_OBSERVED") == "1",
        "native_wayland_layer_shell_notes": os.environ.get("NATIVE_WAYLAND_LAYER_SHELL_NOTES") or None,
        "remaining_manual_scope": [
            "Run this probe on at least GNOME and KDE.",
            "Run this probe in X11 and Wayland-like sessions where available.",
            "For native Wayland layer-shell behavior, add compositor-level observation beyond X11 property checks.",
        ],
    },
}

REPORT.write_text(json.dumps(report, indent=2, sort_keys=True) + "\n", encoding="utf-8")
PY

echo "viewer desktop matrix report: $REPORT_PATH"
if [[ "$SMOKE_STATUS" == "passed" ]]; then
  echo "viewer desktop matrix probe passed"
  exit 0
fi

if [[ "$SMOKE_STATUS" == "skipped" ]]; then
  echo "viewer desktop matrix probe skipped: $SMOKE_REASON"
  exit 0
fi

echo "viewer desktop matrix probe failed: $SMOKE_REASON" >&2
exit "${SMOKE_EXIT_CODE:-1}"
