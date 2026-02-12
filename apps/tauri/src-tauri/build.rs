use std::path::PathBuf;

const ICON_PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 4, 0,
    0, 0, 181, 28, 12, 2, 0, 0, 0, 11, 73, 68, 65, 84, 120, 218, 99, 252, 255, 31, 0, 3, 3, 1, 0,
    10, 186, 201, 155, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

fn ensure_icon() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| "".into());
    let icon_dir = PathBuf::from(manifest_dir).join("icons");
    let icon_path = icon_dir.join("icon.png");

    if icon_path.exists() {
        return;
    }

    if let Err(err) = std::fs::create_dir_all(&icon_dir) {
        panic!("failed to create icons directory: {err}");
    }

    if let Err(err) = std::fs::write(&icon_path, ICON_PNG) {
        panic!("failed to write icon: {err}");
    }
}

fn main() {
    ensure_icon();
    tauri_build::build();
}
