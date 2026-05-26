#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PROFILE_COPY_BASE="${AGENT_WORKSPACE_PROFILE_COPY_BASE:-${XDG_RUNTIME_DIR:-${TMPDIR:-/tmp}}/agent-workspace-linux}"
PROFILE_COPY_DIR="${GROCERY_PROFILE_COPY_DIR:-$PROFILE_COPY_BASE/grocery-profile-copy}"
PREFLIGHT_REPORT_DIR="${REAL_GROCERY_PREFLIGHT_DIR:-$ROOT_DIR/target/real-grocery-preflight}"
REPLACE_PROFILE_COPY="${REPLACE_GROCERY_PROFILE_COPY:-0}"

usage() {
  cat <<'EOF'
collect_real_grocery_evidence.sh

Usage:
  REAL_BROWSER_PROFILE=/path/to/browser-profile \
  REAL_GROCERY_URL=https://www.kroger.com \
  REAL_GROCERY_CART_DRAFT_STEPS=/path/to/cart-draft-steps.json \
    scripts/collect_real_grocery_evidence.sh --preflight-only

  REAL_BROWSER_PROFILE=/path/to/browser-profile \
  REAL_GROCERY_URL=https://www.kroger.com \
  REAL_GROCERY_CART_DRAFT_STEPS=/path/to/cart-draft-steps.json \
    scripts/collect_real_grocery_evidence.sh --run-real-browser

Options:
  --print-cart-draft-steps-template
                         Print a starter cart-draft step JSON file.
  --validate-cart-draft-steps PATH
                         Validate a cart-draft step JSON file without opening a browser.
  --preflight-only       Prepare the disposable profile copy and validate inputs without opening the grocery site.
  --run-real-browser     Prepare, preflight, then open the real site and run the approved cart-draft probe.
  --replace-profile-copy Recreate GROCERY_PROFILE_COPY_DIR if it already exists.
  --self-test            Exercise the preflight wrapper with a synthetic profile and step file.

Required environment:
  REAL_BROWSER_PROFILE              Source browser user-data directory to copy.
  REAL_GROCERY_URL                  Actual HTTPS non-local grocery site; replace
                                    the example URL above with the user's site.
  REAL_GROCERY_CART_DRAFT_STEPS     Site-specific approved cart-draft step JSON.

Optional environment:
  REAL_GROCERY_PROFILE_DIRECTORY    Chrome/Chromium profile directory inside
                                    the copied user-data dir, for example
                                    "Default" or "Profile 1".
  REAL_GROCERY_PRESERVE_WORKSPACE=1 Keep the stopped workspace runtime after a
                                    real-browser run for debugging. Evidence
                                    collected with this flag is not release
                                    eligible.
  REAL_GROCERY_OPEN_VIEWER=1        Open the repo-owned GPUI viewer for the live
                                    workspace so the user can watch/control the
                                    run. The viewer is not always-on-top.
  GROCERY_PROFILE_COPY_DIR          Destination for the disposable copied browser
                                    profile. Defaults outside the repo target
                                    tree under $XDG_RUNTIME_DIR or /tmp.
  AGENT_WORKSPACE_PROFILE_COPY_BASE Base directory for the default disposable
                                    copied browser profile.

Safety:
  This script always refuses checkout/order/account authority. Do not set
  CHECKOUT_APPROVED=1 or REAL_WORLD_ACTION_APPROVED=1 for this release gate.
  Real-browser runs stop and clean their workspace runtime by default so
  screenshots/logs do not accumulate outside the JSON evidence report.
  Real-browser reports must prove the grocery page was discovered and read
  through workspace_browser_targets and workspace_browser_snapshot on the
  workspace Chrome/Chromium app's loopback DevTools endpoint, not through the
  user's host Chrome bridge.
  --preflight-only writes a durable JSON report under
  target/real-grocery-preflight/ by default; set REAL_GROCERY_PREFLIGHT_DIR to
  choose another destination.
EOF
}

fail() {
  echo "error: $*" >&2
  exit 1
}

need() {
  command -v "$1" >/dev/null 2>&1 || fail "missing required command: $1"
}

require_env() {
  local name="$1"
  [[ -n "${!name:-}" ]] || fail "$name is required"
}

probe_env() {
  REAL_GROCERY_DOGFOOD=1 \
  REAL_GROCERY_INTERACTION_MODE=cart-draft-approved \
  CART_MUTATION_APPROVED=1 \
  FINAL_CART_REVIEWED=1 \
  GROCERY_TARGET_URL="$REAL_GROCERY_URL" \
  GROCERY_USER_DATA_DIR="$PROFILE_COPY_DIR" \
  GROCERY_PROFILE_DIRECTORY="${REAL_GROCERY_PROFILE_DIRECTORY:-${GROCERY_PROFILE_DIRECTORY:-}}" \
  GROCERY_PROFILE_IS_DISPOSABLE_COPY=1 \
  GROCERY_CART_DRAFT_STEPS_JSON="$REAL_GROCERY_CART_DRAFT_STEPS" \
  "$@"
}

prepare_profile_copy() {
  mkdir -p "$(dirname "$PROFILE_COPY_DIR")"
  local args=(
    "$ROOT_DIR/scripts/prepare_grocery_profile_copy.js"
    --source "$REAL_BROWSER_PROFILE"
    --dest "$PROFILE_COPY_DIR"
  )
  local profile_dir="${REAL_GROCERY_PROFILE_DIRECTORY:-${GROCERY_PROFILE_DIRECTORY:-}}"
  if [[ -n "$profile_dir" ]]; then
    args+=(--profile-directory "$profile_dir")
  fi
  if [[ "$REPLACE_PROFILE_COPY" == "1" ]]; then
    args+=(--replace)
  fi
  "${args[@]}" >/dev/null
}

validate_required_inputs() {
  need node
  require_env REAL_BROWSER_PROFILE
  require_env REAL_GROCERY_URL
  require_env REAL_GROCERY_CART_DRAFT_STEPS
  [[ "${CHECKOUT_APPROVED:-}" != "1" ]] || fail "unset CHECKOUT_APPROVED; checkout/order/account authority is not part of this gate"
  [[ "${REAL_WORLD_ACTION_APPROVED:-}" != "1" ]] || fail "unset REAL_WORLD_ACTION_APPROVED; checkout/order/account authority is not part of this gate"
  [[ -d "$REAL_BROWSER_PROFILE" ]] || fail "REAL_BROWSER_PROFILE is not a directory: $REAL_BROWSER_PROFILE"
  [[ -f "$REAL_GROCERY_CART_DRAFT_STEPS" ]] || fail "REAL_GROCERY_CART_DRAFT_STEPS is not a file: $REAL_GROCERY_CART_DRAFT_STEPS"
  local profile_dir="${REAL_GROCERY_PROFILE_DIRECTORY:-${GROCERY_PROFILE_DIRECTORY:-}}"
  if [[ -n "$profile_dir" ]]; then
    [[ "$profile_dir" != "." && "$profile_dir" != ".." && "$profile_dir" != */* && "$profile_dir" != *\\* ]] \
      || fail "REAL_GROCERY_PROFILE_DIRECTORY must be a single Chrome profile directory name, such as Default or Profile 1"
    [[ -d "$REAL_BROWSER_PROFILE/$profile_dir" ]] \
      || fail "REAL_GROCERY_PROFILE_DIRECTORY does not exist under REAL_BROWSER_PROFILE: $REAL_BROWSER_PROFILE/$profile_dir"
  fi
}

run_preflight() {
  "$ROOT_DIR/scripts/real_grocery_dogfood_probe.js" --validate-cart-draft-steps "$REAL_GROCERY_CART_DRAFT_STEPS" >/dev/null
  probe_env "$ROOT_DIR/scripts/real_grocery_dogfood_probe.js" --preflight-real-grocery
}

write_preflight_report() {
  local report stamp
  mkdir -p "$PREFLIGHT_REPORT_DIR"
  stamp="$(date -u +%Y%m%dT%H%M%SZ)"
  report="$PREFLIGHT_REPORT_DIR/$stamp.json"
  run_preflight | tee "$report"
  echo "real grocery preflight report: $report" >&2
}

run_real_browser() {
  write_preflight_report >/dev/null
  probe_env "$ROOT_DIR/scripts/real_grocery_dogfood_probe.js"
}

run_self_test() {
  local temp source_profile steps_path profile_copy
  temp="$(mktemp -d "${TMPDIR:-/tmp}/agent-workspace-real-grocery-wrapper-test-XXXXXX")"
  trap 'rm -rf "$temp"' RETURN
  source_profile="$temp/source-profile"
  steps_path="$temp/cart-draft-steps.json"
  profile_copy="$temp/runtime/agent-workspace-linux/grocery-profile-copy"
  mkdir -p "$source_profile/Default" "$source_profile/Profile 1"
  printf '{}\n' >"$source_profile/Default/Preferences"
  printf '{}\n' >"$source_profile/Profile 1/Preferences"
  "$0" --print-cart-draft-steps-template >"$steps_path"
  "$0" --validate-cart-draft-steps "$steps_path" >/dev/null
  REAL_BROWSER_PROFILE="$source_profile" \
  REAL_GROCERY_URL="https://www.kroger.com" \
  REAL_GROCERY_CART_DRAFT_STEPS="$steps_path" \
  REAL_GROCERY_PROFILE_DIRECTORY="Profile 1" \
  XDG_RUNTIME_DIR="$temp/runtime" \
  REAL_GROCERY_PREFLIGHT_DIR="$temp/preflight" \
  BROWSER_BIN="${BROWSER_BIN:-node}" \
  "$0" --preflight-only >/dev/null
  [[ -f "$profile_copy/.agent-workspace-grocery-profile-copy.json" ]] || fail "self-test did not write profile copy manifest"
  [[ -d "$profile_copy/Profile 1" ]] || fail "self-test did not copy requested profile directory"
  [[ "$(find "$temp/preflight" -maxdepth 1 -type f -name '*.json' | wc -l)" -eq 1 ]] || fail "self-test did not write exactly one preflight report"
  echo "real grocery evidence wrapper self-test passed"
}

mode=""
while [[ "$#" -gt 0 ]]; do
  case "$1" in
    --help|-h)
      usage
      exit 0
      ;;
    --preflight-only)
      mode="preflight"
      ;;
    --print-cart-draft-steps-template)
      need node
      "$ROOT_DIR/scripts/real_grocery_dogfood_probe.js" --print-cart-draft-steps-template
      exit 0
      ;;
    --validate-cart-draft-steps)
      shift
      [[ "$#" -gt 0 ]] || fail "--validate-cart-draft-steps requires a path"
      need node
      "$ROOT_DIR/scripts/real_grocery_dogfood_probe.js" --validate-cart-draft-steps "$1"
      exit 0
      ;;
    --run-real-browser)
      mode="run"
      ;;
    --replace-profile-copy)
      REPLACE_PROFILE_COPY=1
      ;;
    --self-test)
      run_self_test
      exit 0
      ;;
    *)
      fail "unknown option: $1"
      ;;
  esac
  shift
done

[[ -n "$mode" ]] || {
  usage >&2
  exit 1
}

validate_required_inputs
prepare_profile_copy

if [[ -n "${REAL_GROCERY_PROFILE_DIRECTORY:-${GROCERY_PROFILE_DIRECTORY:-}}" ]]; then
  profile_dir="${REAL_GROCERY_PROFILE_DIRECTORY:-${GROCERY_PROFILE_DIRECTORY:-}}"
  [[ -d "$PROFILE_COPY_DIR/$profile_dir" ]] \
    || fail "prepared profile copy is missing requested profile directory: $PROFILE_COPY_DIR/$profile_dir"
fi

case "$mode" in
  preflight)
    write_preflight_report
    ;;
  run)
    run_real_browser
    ;;
esac
