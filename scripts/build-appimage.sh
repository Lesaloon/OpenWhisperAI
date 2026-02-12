#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'USAGE'
Usage: build-appimage.sh --name NAME --version VERSION --bin PATH --icon PATH [options]

Options:
  --app-id ID       Desktop file and icon ID (default: NAME)
  --output DIR      Output directory (default: dist/artifacts)
USAGE
}

APP_NAME=""
VERSION=""
APP_BIN=""
APP_ICON=""
APP_ID=""
OUTPUT_DIR="dist/artifacts"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --name)
      APP_NAME="$2"
      shift 2
      ;;
    --version)
      VERSION="$2"
      shift 2
      ;;
    --bin)
      APP_BIN="$2"
      shift 2
      ;;
    --icon)
      APP_ICON="$2"
      shift 2
      ;;
    --app-id)
      APP_ID="$2"
      shift 2
      ;;
    --output)
      OUTPUT_DIR="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      usage
      exit 2
      ;;
  esac
done

if [[ -z "$APP_NAME" || -z "$VERSION" || -z "$APP_BIN" || -z "$APP_ICON" ]]; then
  usage
  exit 2
fi

if [[ -z "$APP_ID" ]]; then
  APP_ID="$APP_NAME"
fi

if [[ ! -f "$APP_BIN" ]]; then
  echo "Binary not found: $APP_BIN" >&2
  exit 1
fi

if [[ ! -f "$APP_ICON" ]]; then
  echo "Icon not found: $APP_ICON" >&2
  exit 1
fi

if ! command -v appimagetool >/dev/null 2>&1; then
  echo "appimagetool is required on PATH" >&2
  exit 1
fi

APPDIR="$(mktemp -d)"
cleanup() {
  rm -rf "$APPDIR"
}
trap cleanup EXIT

mkdir -p "$APPDIR/usr/bin" \
  "$APPDIR/usr/share/applications" \
  "$APPDIR/usr/share/icons/hicolor/256x256/apps"

cp "$APP_BIN" "$APPDIR/usr/bin/$APP_NAME"
chmod +x "$APPDIR/usr/bin/$APP_NAME"

ICON_NAME="$APP_ID"
cp "$APP_ICON" "$APPDIR/usr/share/icons/hicolor/256x256/apps/${ICON_NAME}.png"

DESKTOP_FILE="$APPDIR/usr/share/applications/${APP_ID}.desktop"
cat > "$DESKTOP_FILE" <<EOF
[Desktop Entry]
Type=Application
Name=$APP_NAME
Exec=$APP_NAME
Icon=$ICON_NAME
Categories=Utility;
Terminal=false
EOF

ln -sf "usr/bin/$APP_NAME" "$APPDIR/AppRun"

if command -v linuxdeploy >/dev/null 2>&1; then
  linuxdeploy \
    --appdir "$APPDIR" \
    --executable "$APPDIR/usr/bin/$APP_NAME" \
    --desktop-file "$DESKTOP_FILE" \
    --icon-file "$APP_ICON"
fi

mkdir -p "$OUTPUT_DIR"
ARCH="$(uname -m)"
APPIMAGE_NAME="${APP_NAME}-${VERSION}-${ARCH}.AppImage"
export VERSION

appimagetool "$APPDIR" "$OUTPUT_DIR/$APPIMAGE_NAME"
echo "Created $OUTPUT_DIR/$APPIMAGE_NAME"
