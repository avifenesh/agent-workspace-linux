#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN="${AGENT_WORKSPACE_BIN:-${BIN:-$ROOT_DIR/target/debug/agent-workspace-linux}}"

need() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required command: $1" >&2
    exit 1
  fi
}

need jq
need python3

BROWSER_BIN="${BROWSER_BIN:-}"
if [[ -z "$BROWSER_BIN" ]]; then
  BROWSER_BIN="$(command -v google-chrome || command -v google-chrome-stable || command -v chromium || command -v chromium-browser || true)"
fi

if [[ -z "$BROWSER_BIN" ]]; then
  echo "grocery browser workflow smoke skipped: Chrome/Chromium not found"
  exit 0
fi

SMOKE_DIR="$(mktemp -d)"
CONFIG_DIR="$SMOKE_DIR/config"
RUNTIME_DIR="$SMOKE_DIR/runtime"
WORKSPACE_ID="grocery-browser-smoke-$$"
mkdir -p "$CONFIG_DIR" "$RUNTIME_DIR"

run_awl() {
  XDG_CONFIG_HOME="$CONFIG_DIR" XDG_RUNTIME_DIR="$RUNTIME_DIR" "$BIN" "$@"
}

cleanup() {
  exit_code=$?
  run_awl workspace stop --id "$WORKSPACE_ID" >/dev/null 2>&1 || true
  if [[ "$exit_code" -eq 0 ]]; then
    rm -rf "$SMOKE_DIR"
  else
    echo "grocery browser workflow smoke failed; preserved temp dir: $SMOKE_DIR" >&2
  fi
}
trap cleanup EXIT

assert_json() {
  local filter="$1"
  local file="$2"
  shift 2
  jq -e "$@" "$filter" "$file" >/dev/null
}

GROCERY_URL="$(python3 - <<'PY'
from urllib.parse import quote

html = """<!doctype html>
<meta charset="utf-8">
<title>Grocery Dogfood Ready</title>
<style>
  body { font-family: system-ui, sans-serif; margin: 32px; color: #202124; }
  input, button { font: inherit; padding: 10px 12px; margin: 4px 0; }
  input { width: min(520px, 90vw); }
  #cart { margin-top: 16px; padding: 12px; border: 1px solid #ccd0d5; }
</style>
<h1>Grocery Dogfood</h1>
<p>Draft a cart, but keep checkout locked unless a separate approval exists.</p>
<label>Shopping list
  <input id="items" autofocus aria-label="shopping list" placeholder="milk 2L, eggs 12">
</label>
<br>
<button id="add">Add draft cart</button>
<button id="checkout" disabled>Checkout locked</button>
<div id="cart">Cart is empty</div>
<script>
const items = document.getElementById("items");
const cart = document.getElementById("cart");
const add = document.getElementById("add");

function listItems() {
  return items.value.split(",").map((item) => item.trim()).filter(Boolean);
}

function setDraftTitle() {
  document.title = items.value ? `draft:${items.value}` : "Grocery Dogfood Ready";
}

function addDraftCart() {
  const drafted = listItems();
  cart.textContent = drafted.length
    ? `Draft cart only: ${drafted.join(" | ")}`
    : "Cart is empty";
  document.title = `cart:${drafted.length}:checkout-locked`;
}

items.addEventListener("input", setDraftTitle);
items.addEventListener("keydown", (event) => {
  if (event.key === "Enter") {
    event.preventDefault();
    addDraftCart();
  }
});
add.addEventListener("click", addDraftCart);
setTimeout(() => items.focus(), 100);
</script>
"""

print("data:text/html;charset=utf-8," + quote(html))
PY
)"

run_awl workspace start --ack-hidden-workspace --id "$WORKSPACE_ID" --purpose "Grocery browser dogfood smoke" > "$SMOKE_DIR/start.json"
run_awl workspace launch \
  --id "$WORKSPACE_ID" \
  --name grocery-browser \
  --wait-window \
  --screenshot-window \
  --window-timeout-ms 15000 \
  -- "$BROWSER_BIN" "--user-data-dir=$SMOKE_DIR/browser-profile" --no-sandbox --disable-dev-shm-usage --no-first-run --no-default-browser-check --ozone-platform=x11 --new-window about:blank \
  > "$SMOKE_DIR/launch.json"
assert_json '.ok == true and (.screenshot.bytes > 0) and ((.windows | length) > 0) and .apps[0].running == true' "$SMOKE_DIR/launch.json"
GROCERY_APP_ID="$(jq -r '.apps[0].id' "$SMOKE_DIR/launch.json")"

run_awl workspace key-window --id "$WORKSPACE_ID" --app "$GROCERY_APP_ID" --timeout-ms 5000 ctrl+l > "$SMOKE_DIR/address-key.json"
run_awl workspace paste-window --id "$WORKSPACE_ID" --app "$GROCERY_APP_ID" --timeout-ms 5000 "$GROCERY_URL" > "$SMOKE_DIR/address-paste.json"
run_awl workspace key-window --id "$WORKSPACE_ID" --app "$GROCERY_APP_ID" --timeout-ms 5000 Return > "$SMOKE_DIR/address-return.json"
run_awl workspace wait-window --id "$WORKSPACE_ID" --app "$GROCERY_APP_ID" --title "Grocery Dogfood Ready" --timeout-ms 10000 > "$SMOKE_DIR/wait-ready.json"
assert_json '.ok == true and (.windows[0].title | contains("Grocery Dogfood Ready"))' "$SMOKE_DIR/wait-ready.json"

run_awl workspace type-window --id "$WORKSPACE_ID" --app "$GROCERY_APP_ID" --timeout-ms 5000 "milk 2L, eggs 12, bananas 1kg" > "$SMOKE_DIR/type-list.json"
run_awl workspace wait-window --id "$WORKSPACE_ID" --app "$GROCERY_APP_ID" --title "draft:milk 2L" --timeout-ms 10000 > "$SMOKE_DIR/wait-draft.json"
assert_json '.ok == true and (.windows[0].title | contains("draft:milk 2L"))' "$SMOKE_DIR/wait-draft.json"

run_awl workspace key-window --id "$WORKSPACE_ID" --app "$GROCERY_APP_ID" --timeout-ms 5000 Return > "$SMOKE_DIR/add-cart.json"
run_awl workspace wait-window --id "$WORKSPACE_ID" --app "$GROCERY_APP_ID" --title "cart:3:checkout-locked" --timeout-ms 10000 > "$SMOKE_DIR/wait-cart.json"
assert_json '.ok == true and (.windows[0].title | contains("cart:3:checkout-locked"))' "$SMOKE_DIR/wait-cart.json"

run_awl workspace observe --id "$WORKSPACE_ID" --screenshot --events --events-tail 30 > "$SMOKE_DIR/observe.json"
assert_json '(.screenshot.bytes > 0) and (.active_window.title | contains("cart:3:checkout-locked")) and (.events | length > 0)' "$SMOKE_DIR/observe.json"
if jq -e '.active_window.title | contains("order-submitted")' "$SMOKE_DIR/observe.json" >/dev/null; then
  echo "grocery smoke crossed checkout boundary unexpectedly" >&2
  exit 1
fi

run_awl workspace stop --id "$WORKSPACE_ID" > "$SMOKE_DIR/stop.json"
assert_json '.ok == true and (.apps[] | select(.name == "grocery-browser" and .running == false))' "$SMOKE_DIR/stop.json"

echo "grocery browser workflow smoke passed"
