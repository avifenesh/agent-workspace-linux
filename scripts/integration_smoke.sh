#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${BIN:-$ROOT_DIR/target/debug/agent-workspace-linux}"

need() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required command: $1" >&2
    exit 1
  fi
}

need jq
need python3

cargo build --manifest-path "$ROOT_DIR/Cargo.toml" >/dev/null

SMOKE_DIR="$(mktemp -d)"
CONFIG_DIR="$SMOKE_DIR/config"
RUNTIME_DIR="$SMOKE_DIR/runtime"
mkdir -p "$CONFIG_DIR" "$RUNTIME_DIR"

WORKSPACE_IDS=()

run_awl() {
  XDG_CONFIG_HOME="$CONFIG_DIR" XDG_RUNTIME_DIR="$RUNTIME_DIR" "$BIN" "$@"
}

cleanup() {
  exit_code=$?
  for workspace_id in "${WORKSPACE_IDS[@]:-}"; do
    run_awl workspace stop --id "$workspace_id" >/dev/null 2>&1 || true
  done
  if [[ "$exit_code" -eq 0 ]]; then
    rm -rf "$SMOKE_DIR"
  else
    echo "integration smoke failed; preserved temp dir: $SMOKE_DIR" >&2
  fi
}
trap cleanup EXIT

assert_json() {
  local filter="$1"
  local file="$2"
  shift 2
  jq -e "$@" "$filter" "$file" >/dev/null
}

echo "== doctor =="
run_awl doctor > "$SMOKE_DIR/doctor.json"
assert_json '.ready_for_x11_workspace == true' "$SMOKE_DIR/doctor.json"

echo "== profile import/export =="
IMPORT_PROFILE="$SMOKE_DIR/import-profile.json"
jq -n '{
  id: "import-smoke",
  cwd: "/tmp",
  env: [{name:"ONE", value:"1"}],
  startup_apps: [{name:"noop", command:["/bin/true"], cwd:"/tmp"}]
}' > "$IMPORT_PROFILE"
run_awl profile import --json "$IMPORT_PROFILE" --dry-run > "$SMOKE_DIR/import-dry.json"
assert_json '.dry_run == true and .would_create == true and .saved == false' "$SMOKE_DIR/import-dry.json"
run_awl profile import --json "$IMPORT_PROFILE" > "$SMOKE_DIR/import.json"
assert_json '.created == true and .saved == true' "$SMOKE_DIR/import.json"
if run_awl profile import --json "$IMPORT_PROFILE" > "$SMOKE_DIR/import-duplicate.json" 2> "$SMOKE_DIR/import-duplicate.err"; then
  echo "duplicate profile import unexpectedly succeeded" >&2
  exit 1
fi
grep -q "already exists" "$SMOKE_DIR/import-duplicate.err"
mkdir -p "$SMOKE_DIR/export-dir"
run_awl profile export import-smoke --output "$SMOKE_DIR/export-dir" > "$SMOKE_DIR/export.json"
assert_json '.wrote == true and .output_path == $path' "$SMOKE_DIR/export.json" --arg path "$SMOKE_DIR/export-dir/import-smoke.json"
test -f "$SMOKE_DIR/export-dir/import-smoke.json"

echo "== open-profile dry-run =="
OPEN_PROFILE="$SMOKE_DIR/open-profile.json"
jq -n '{
  id: "open-preview",
  width: 800,
  height: 600,
  cwd: "/tmp",
  setup_commands: [{name:"noop-setup", command:["/bin/true"]}],
  startup_apps: [{name:"noop-startup", command:["/bin/true"]}]
}' > "$OPEN_PROFILE"
run_awl profile import --json "$OPEN_PROFILE" > /dev/null
OPEN_ID="open-preview-smoke-$$"
run_awl workspace open-profile --dry-run --profile open-preview --id "$OPEN_ID" --setup --startup-wait-window > "$SMOKE_DIR/open-noack.json"
assert_json '.would_open == false and .approval.missing_acknowledgements[0].id == "hidden_workspace"' "$SMOKE_DIR/open-noack.json"
run_awl workspace open-profile --dry-run --ack-hidden-workspace --profile open-preview --id "$OPEN_ID" --setup --startup-wait-window > "$SMOKE_DIR/open-ack.json"
assert_json '.would_open == true and .setup.command_count == 1 and .startup.app_count == 1 and .approval.approved == true' "$SMOKE_DIR/open-ack.json"
test ! -d "$RUNTIME_DIR/agent-workspace-linux/$OPEN_ID"

echo "== local-only workspace =="
LOCAL_PROFILE="$SMOKE_DIR/local-profile.json"
jq -n '{
  id: "local-only",
  network: {mode:"local_only", allow_hosts:["localhost:3000", "127.0.0.1:5173"]}
}' > "$LOCAL_PROFILE"
run_awl profile import --json "$LOCAL_PROFILE" > /dev/null
LOCAL_ID="local-only-smoke-$$"
WORKSPACE_IDS+=("$LOCAL_ID")
run_awl workspace start --ack-hidden-workspace --profile local-only --id "$LOCAL_ID" --purpose "Integration smoke" > "$SMOKE_DIR/local-start.json"
run_awl workspace status --id "$LOCAL_ID" > "$SMOKE_DIR/local-status.json"
assert_json '.applied_policy.enforcement.network.enforced == true and .applied_policy.enforcement.network.backend == "bubblewrap_loopback_only" and .user_acknowledged_unenforced_policy == false' "$SMOKE_DIR/local-status.json"
SESSION_ID="$(jq -r '.session_id' "$SMOKE_DIR/local-status.json")"

NETWORK_PROBE="$SMOKE_DIR/network_probe.py"
cat > "$NETWORK_PROBE" <<'PY'
import socket
import threading

s = socket.socket()
s.bind(("127.0.0.1", 0))
s.listen(1)
port = s.getsockname()[1]

def serve():
    conn, _ = s.accept()
    conn.recv(16)
    conn.sendall(b"loopback-ok")
    conn.close()

t = threading.Thread(target=serve)
t.start()
c = socket.create_connection(("127.0.0.1", port), timeout=2)
c.sendall(b"ping")
print(c.recv(64).decode())
c.close()
t.join(2)

try:
    socket.create_connection(("1.1.1.1", 80), timeout=2)
except OSError:
    print("external-blocked")
else:
    raise SystemExit("external network unexpectedly reachable")
PY
run_awl workspace run --id "$LOCAL_ID" --timeout-ms 8000 --tail-bytes 4000 -- python3 "$NETWORK_PROBE" > "$SMOKE_DIR/local-run.json"
assert_json '.succeeded == true and (.stdout.content | contains("loopback-ok")) and (.stdout.content | contains("external-blocked")) and .launch.apps[0].network_isolation == "bubblewrap_loopback_only"' "$SMOKE_DIR/local-run.json"
run_awl workspace env --id "$LOCAL_ID" > "$SMOKE_DIR/local-env.json"
assert_json '.environment.session_id == $sid and (.environment.variables[] | select(.name == "AGENT_WORKSPACE_SESSION_ID" and .value == $sid))' "$SMOKE_DIR/local-env.json" --arg sid "$SESSION_ID"
run_awl workspace events --id "$LOCAL_ID" --tail 20 > "$SMOKE_DIR/local-events.json"
assert_json '.events[] | select(.kind == "workspace_start" and .detail.session_id == $sid)' "$SMOKE_DIR/local-events.json" --arg sid "$SESSION_ID"
run_awl workspace stop --id "$LOCAL_ID" > /dev/null
WORKSPACE_IDS=()
run_awl workspace manifest --id "$LOCAL_ID" > "$SMOKE_DIR/local-stopped-manifest.json"
assert_json '.manifest.session_id == $sid and .manifest.ready == false and .manifest.stopped_at_unix != null' "$SMOKE_DIR/local-stopped-manifest.json" --arg sid "$SESSION_ID"

echo "== self-stop from workspace app =="
SELF_STOP_ID="self-stop-smoke-$$"
WORKSPACE_IDS+=("$SELF_STOP_ID")
run_awl workspace start --ack-hidden-workspace --id "$SELF_STOP_ID" --purpose "Self-stop smoke" > "$SMOKE_DIR/self-stop-start.json"
run_awl workspace launch --id "$SELF_STOP_ID" --name self-stop-client -- bash -lc '"$1" workspace stop --id "$2" >/tmp/agent-workspace-self-stop-smoke.out 2>/tmp/agent-workspace-self-stop-smoke.err; sleep 10' _ "$BIN" "$SELF_STOP_ID" > "$SMOKE_DIR/self-stop-launch.json"
sleep 2
run_awl workspace list > "$SMOKE_DIR/self-stop-list.json"
assert_json '.workspaces[] | select(.id == $id and .running == false and .manifest.ready == false and .manifest.stopped_at_unix != null)' "$SMOKE_DIR/self-stop-list.json" --arg id "$SELF_STOP_ID"

echo "integration smoke passed"
