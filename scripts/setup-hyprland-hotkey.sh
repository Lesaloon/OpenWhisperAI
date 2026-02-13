#!/usr/bin/env bash
set -euo pipefail

HOTKEY_MODS=${HOTKEY_MODS:-"CTRL ALT"}
HOTKEY_KEY=${HOTKEY_KEY:-"SPACE"}

BIN_DIR="$HOME/.local/bin"
CONFIG_DIR="$HOME/.config/hypr"
BINDINGS_FILE="$CONFIG_DIR/bindings.conf"
TOGGLE_BIN="$BIN_DIR/openwhisperai-toggle"

mkdir -p "$BIN_DIR" "$CONFIG_DIR"

SCRIPT_DIR=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
cp "$SCRIPT_DIR/openwhisperai-toggle.sh" "$TOGGLE_BIN"
chmod +x "$TOGGLE_BIN"

if [[ -f "$BINDINGS_FILE" ]]; then
  cp "$BINDINGS_FILE" "$BINDINGS_FILE.bak.$(date +%s)"
fi

{
  echo ""
  echo "# OpenWhisperAI toggle hotkey"
  echo "unbind = ${HOTKEY_MODS}, ${HOTKEY_KEY}"
  echo "bind = ${HOTKEY_MODS}, ${HOTKEY_KEY}, exec, $TOGGLE_BIN"
} >> "$BINDINGS_FILE"

hyprctl reload
echo "Added OpenWhisperAI hotkey: ${HOTKEY_MODS} + ${HOTKEY_KEY}"
