# OpenWhisperAI

OpenWhisperAI is a desktop-first workspace built on a Rust core and a Tauri shell.
This repository starts with a Rust workspace, a minimal Tauri app shell, and a
shared types crate for cross-package contracts.

## Workspace layout

- `apps/tauri/src-tauri`: Tauri shell (Rust)
- `crates/shared-types`: shared Rust types and serde contracts

## Prerequisites

- Rust (stable)
- Tauri system dependencies (see Tauri docs for your OS)

## Development

```bash
cargo test -p shared-types
```

```bash
cargo build -p openwhisperai-shell
```

## Notes

- The Tauri frontend assets are expected in `apps/tauri/dist` when wired up.
- Update `apps/tauri/src-tauri/tauri.conf.json` as the UI stack evolves.
