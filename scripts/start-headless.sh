#!/usr/bin/env bash
set -euo pipefail

export OPENWHISPERAI_HEADLESS=1
export OPENWHISPERAI_UI_SERVER=0

cd "$(dirname "$0")/../apps/tauri/src-tauri"

cargo run
