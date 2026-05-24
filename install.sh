#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BIN_NAME="agent-workspace-linux"

PREFIX="${PREFIX:-$HOME/.local}"
BINDIR="${BINDIR:-$PREFIX/bin}"
CODEX_HOME="${CODEX_HOME:-$HOME/.codex}"
CODEX_CONFIG="${CODEX_CONFIG:-$CODEX_HOME/config.toml}"
CODEX_MCP_SERVER_NAME="${CODEX_MCP_SERVER_NAME:-agent-workspace-linux}"

DRY_RUN=0
SKIP_BUILD=0
CONFIGURE_CODEX=1
RUN_DOCTOR=1

usage() {
  cat <<USAGE
Usage: ./install.sh [options]

Build and install agent-workspace-linux, then register its MCP server in Codex.

Options:
  --dry-run              Show what would happen without writing files.
  --skip-build           Install an already-built target/release binary.
  --no-codex-config      Do not edit the Codex MCP config.
  --no-doctor            Do not run agent-workspace-linux doctor after install.
  --prefix PATH          Install under PATH (default: ~/.local).
  --bindir PATH          Install binary into PATH (default: PREFIX/bin).
  --codex-home PATH      Use this Codex home (default: ~/.codex).
  --codex-config PATH    Use this Codex config file.
  -h, --help             Show this help.
USAGE
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --dry-run)
      DRY_RUN=1
      ;;
    --skip-build)
      SKIP_BUILD=1
      ;;
    --no-codex-config)
      CONFIGURE_CODEX=0
      ;;
    --no-doctor)
      RUN_DOCTOR=0
      ;;
    --prefix)
      PREFIX="${2:?missing value for --prefix}"
      BINDIR="$PREFIX/bin"
      shift
      ;;
    --bindir)
      BINDIR="${2:?missing value for --bindir}"
      shift
      ;;
    --codex-home)
      CODEX_HOME="${2:?missing value for --codex-home}"
      CODEX_CONFIG="$CODEX_HOME/config.toml"
      shift
      ;;
    --codex-config)
      CODEX_CONFIG="${2:?missing value for --codex-config}"
      shift
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      echo "unknown option: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
  shift
done

SOURCE_BIN="$ROOT_DIR/target/release/$BIN_NAME"
DEST_BIN="$BINDIR/$BIN_NAME"

toml_escape() {
  printf '%s' "$1" | sed 's/\\/\\\\/g; s/"/\\"/g'
}

desired_mcp_block() {
  local escaped_command
  escaped_command="$(toml_escape "$DEST_BIN")"
  printf '[mcp_servers.%s]\n' "$CODEX_MCP_SERVER_NAME"
  printf 'command = "%s"\n' "$escaped_command"
  printf 'args = ["mcp"]\n'
}

write_codex_config() {
  local config_dir tmp desired section backup
  config_dir="$(dirname "$CODEX_CONFIG")"
  section="[mcp_servers.$CODEX_MCP_SERVER_NAME]"
  desired="$(desired_mcp_block)"

  if [ "$DRY_RUN" -eq 1 ]; then
    echo "Would register Codex MCP server in $CODEX_CONFIG:"
    printf '%s\n' "$desired"
    return
  fi

  mkdir -p "$config_dir"
  tmp="$(mktemp "$config_dir/config.toml.tmp.XXXXXX")"

  if [ -f "$CODEX_CONFIG" ]; then
    awk -v section="$section" '
      $0 == section { skip = 1; next }
      skip && /^\[/ { skip = 0 }
      !skip { lines[++n] = $0 }
      END {
        while (n > 0 && lines[n] == "") {
          n--
        }
        for (i = 1; i <= n; i++) {
          print lines[i]
        }
      }
    ' "$CODEX_CONFIG" >"$tmp"
  else
    : >"$tmp"
  fi

  if [ -s "$tmp" ]; then
    printf '\n' >>"$tmp"
  fi
  printf '%s\n' "$desired" >>"$tmp"

  if [ -f "$CODEX_CONFIG" ] && cmp -s "$tmp" "$CODEX_CONFIG"; then
    rm -f "$tmp"
    echo "Codex MCP config already up to date: $CODEX_CONFIG"
    return
  fi

  if [ -f "$CODEX_CONFIG" ]; then
    backup="$CODEX_CONFIG.bak-agent-workspace-$(date +%Y%m%d%H%M%S)-$$"
    cp -p "$CODEX_CONFIG" "$backup"
    echo "Backed up Codex config to $backup"
  fi

  mv "$tmp" "$CODEX_CONFIG"
  chmod 600 "$CODEX_CONFIG" 2>/dev/null || true
  echo "Registered Codex MCP server '$CODEX_MCP_SERVER_NAME' in $CODEX_CONFIG"
}

if [ "$SKIP_BUILD" -eq 0 ]; then
  if [ "$DRY_RUN" -eq 1 ]; then
    echo "Would build release binary with cargo."
  else
    cargo build --manifest-path "$ROOT_DIR/Cargo.toml" --release
  fi
fi

if [ "$DRY_RUN" -eq 1 ]; then
  echo "Would install $SOURCE_BIN to $DEST_BIN"
else
  if [ ! -x "$SOURCE_BIN" ]; then
    echo "missing release binary: $SOURCE_BIN" >&2
    echo "Run without --skip-build, or build it first." >&2
    exit 1
  fi
  install -Dm755 "$SOURCE_BIN" "$DEST_BIN"
  echo "Installed $DEST_BIN"
fi

if [ "$CONFIGURE_CODEX" -eq 1 ]; then
  write_codex_config
fi

if [ "$RUN_DOCTOR" -eq 1 ]; then
  if [ "$DRY_RUN" -eq 1 ]; then
    echo "Would run $DEST_BIN doctor"
  else
    "$DEST_BIN" doctor
  fi
fi

if [ "$CONFIGURE_CODEX" -eq 1 ]; then
  echo "Restart Codex or reload MCP servers so the new workspace_* tools become available."
fi
