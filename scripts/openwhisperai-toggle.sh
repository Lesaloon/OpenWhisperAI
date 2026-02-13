#!/usr/bin/env bash
set -euo pipefail

CONTROL_URL="http://127.0.0.1:1422/toggle"
PID_FILE="$HOME/.local/share/com.openwhisperai.app/openwhisperai.pid"

if command -v curl >/dev/null 2>&1; then
  if curl -fsS "$CONTROL_URL" >/dev/null 2>&1; then
    exit 0
  fi
fi

if [[ -f "$PID_FILE" ]]; then
  pid=$(cat "$PID_FILE" 2>/dev/null || true)
else
  pid=$(pgrep -f "openwhisperai-shell" | head -n 1 || true)
fi

if [[ -z "$pid" ]]; then
  echo "OpenWhisperAI is not running" >&2
  exit 1
fi

echo "sending SIGUSR1 to pid $pid" >&2
kill -USR1 "$pid"
exit 0
