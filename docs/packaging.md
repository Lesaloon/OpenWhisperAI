# Packaging

This repo ships two packaging helpers:

- Linux AppImage via `scripts/build-appimage.sh`
- Windows MSI via `scripts/build-windows-installer.ps1` (WiX)

## Prerequisites

Linux:

- `appimagetool` on PATH
- optional: `linuxdeploy` on PATH for dependency bundling

Windows:

- WiX Toolset v3 (provides `candle.exe` and `light.exe` on PATH)
- PowerShell 5+

## Build the release binary

Use the Rust release build for packaging:

```bash
cargo build -p openwhisperai-shell --release
```

The binary will be under `target/release/`.

## Linux AppImage

```bash
./scripts/build-appimage.sh \
  --name OpenWhisperAI \
  --version 0.1.0 \
  --bin target/release/openwhisperai-shell \
  --icon assets/icon-256.png \
  --app-id openwhisperai \
  --output dist/artifacts
```

Output: `dist/artifacts/OpenWhisperAI-0.1.0-<arch>.AppImage`.

## Windows MSI (WiX)

Generate a stable upgrade code once (GUID) and reuse it for all releases.

```powershell
./scripts/build-windows-installer.ps1 `
  -Name OpenWhisperAI `
  -Version 0.1.0 `
  -SourceDir "target\release" `
  -ExeName "openwhisperai-shell.exe" `
  -UpgradeCode "{01234567-89AB-CDEF-0123-456789ABCDEF}" `
  -OutputDir "dist\artifacts" `
  -Manufacturer "OpenWhisperAI"
```

Output: `dist/artifacts/OpenWhisperAI-0.1.0-x64.msi`.
