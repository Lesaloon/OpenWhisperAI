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

fn sync_public_assets() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| "".into());
    let source_dir = PathBuf::from(&manifest_dir)
        .join("..")
        .join("..")
        .join("..")
        .join("tauri-app")
        .join("public");
    let target_dir = PathBuf::from(&manifest_dir).join("public");

    emit_rerun_if_changed(&source_dir);

    if !source_dir.exists() {
        panic!("missing source assets: {}", source_dir.display());
    }

    if target_dir.exists() {
        let _ = std::fs::remove_dir_all(&target_dir);
    }
    if let Err(err) = std::fs::create_dir_all(&target_dir) {
        panic!("failed to create public dir: {err}");
    }

    copy_dir_recursive(&source_dir, &target_dir);
}

fn emit_rerun_if_changed(source: &PathBuf) {
    if !source.exists() {
        return;
    }
    let mut stack = vec![source.clone()];
    while let Some(dir) = stack.pop() {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else {
                    println!("cargo:rerun-if-changed={}", path.display());
                }
            }
        }
    }
}

fn copy_dir_recursive(source: &PathBuf, target: &PathBuf) {
    let entries = match std::fs::read_dir(source) {
        Ok(entries) => entries,
        Err(err) => panic!("failed to read {}: {err}", source.display()),
    };

    for entry in entries.flatten() {
        let path = entry.path();
        let file_name = match path.file_name() {
            Some(name) => name,
            None => continue,
        };
        let target_path = target.join(file_name);
        if path.is_dir() {
            if let Err(err) = std::fs::create_dir_all(&target_path) {
                panic!("failed to create dir {}: {err}", target_path.display());
            }
            copy_dir_recursive(&path, &target_path);
        } else if let Err(err) = std::fs::copy(&path, &target_path) {
            panic!("failed to copy {}: {err}", path.display());
        }
    }
}

fn main() {
    ensure_icon();
    sync_public_assets();
    tauri_build::build();
}
