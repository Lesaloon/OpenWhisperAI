use log::{info, warn};
use std::env;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;

const WHISPER_CPP_VERSION: &str = "v1.8.3";

pub fn ensure_whisper_cli(app_data_dir: PathBuf) {
    thread::spawn(move || {
        if let Err(err) = ensure_whisper_cli_sync(&app_data_dir) {
            warn!("whisper auto-install failed: {err}");
        }
    });
}

fn ensure_whisper_cli_sync(app_data_dir: &Path) -> Result<(), String> {
    if let Some(bin) = env::var_os("WHISPER_CPP_BIN") {
        let path = PathBuf::from(bin);
        if path.exists() {
            info!("whisper cli configured: {}", path.display());
            return Ok(());
        }
        warn!("WHISPER_CPP_BIN set but missing: {}", path.display());
    }
    if let Some(existing) = find_existing_bin(app_data_dir) {
        if !cfg!(windows) {
            let filename = existing
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("");
            if filename == "whisper" {
                warn!("deprecated whisper cli found; rebuilding to get whisper-cli");
            } else {
                env::set_var("WHISPER_CPP_BIN", &existing);
                info!("whisper cli found: {}", existing.display());
                return Ok(());
            }
        } else {
            env::set_var("WHISPER_CPP_BIN", &existing);
            info!("whisper cli found: {}", existing.display());
            return Ok(());
        }
    }

    let bin_path = default_bin_path(app_data_dir);

    if cfg!(target_os = "linux") {
        return build_from_source(app_data_dir, &bin_path);
    }

    let asset = select_asset_name().ok_or_else(|| "unsupported platform".to_string())?;
    let url = format!(
        "https://github.com/ggml-org/whisper.cpp/releases/download/{}/{}",
        WHISPER_CPP_VERSION, asset
    );
    info!("downloading whisper cli from {url}");

    let bytes = download_bytes(&url)?;
    let extracted = extract_cli(&bytes, &bin_path)?;
    if !extracted {
        return Err("whisper cli not found in archive".to_string());
    }
    env::set_var("WHISPER_CPP_BIN", &bin_path);
    info!("whisper cli installed: {}", bin_path.display());
    Ok(())
}

fn select_asset_name() -> Option<&'static str> {
    let arch = env::consts::ARCH;
    let os = env::consts::OS;
    match (os, arch) {
        ("windows", "x86_64") => Some("whisper-bin-x64.zip"),
        ("windows", "x86") => Some("whisper-bin-Win32.zip"),
        ("linux", "x86_64") => Some("whisper-bin-x64.zip"),
        ("macos", "x86_64") => Some("whisper-bin-x64.zip"),
        ("macos", "aarch64") => Some("whisper-bin-x64.zip"),
        _ => None,
    }
}

fn default_bin_path(app_data_dir: &Path) -> PathBuf {
    let bin_dir = app_data_dir.join("bin");
    if cfg!(windows) {
        bin_dir.join("whisper.exe")
    } else {
        bin_dir.join("whisper-cli")
    }
}

fn find_existing_bin(app_data_dir: &Path) -> Option<PathBuf> {
    let bin_dir = app_data_dir.join("bin");
    let candidates = if cfg!(windows) {
        vec![bin_dir.join("whisper.exe")]
    } else {
        vec![
            bin_dir.join("whisper-whisper"),
            bin_dir.join("whisper-cli"),
            bin_dir.join("whisper"),
        ]
    };
    candidates.into_iter().find(|path| path.exists())
}

fn build_from_source(app_data_dir: &Path, bin_path: &Path) -> Result<(), String> {
    let mut missing = Vec::new();
    if !command_exists("git") {
        missing.push("git");
    }
    if !command_exists("cmake") {
        missing.push("cmake");
    }
    if !command_exists("make") {
        missing.push("make");
    }
    if !command_exists("c++") && !command_exists("g++") {
        missing.push("c++ (or g++)");
    }
    if !missing.is_empty() {
        return Err(format!("missing build tools: {}", missing.join(", ")));
    }

    let src_dir = app_data_dir.join("whisper.cpp");
    if !src_dir.exists() {
        info!("cloning whisper.cpp source");
        let status = Command::new("git")
            .arg("clone")
            .arg("--depth")
            .arg("1")
            .arg("--branch")
            .arg(WHISPER_CPP_VERSION)
            .arg("https://github.com/ggml-org/whisper.cpp.git")
            .arg(&src_dir)
            .status()
            .map_err(|err| format!("failed to run git: {err}"))?;
        if !status.success() {
            return Err("git clone failed".to_string());
        }
    }

    info!("building whisper.cpp from source");
    let jobs = std::thread::available_parallelism()
        .map(|value| value.get().to_string())
        .unwrap_or_else(|_| "4".to_string());
    let status = Command::new("make")
        .arg("-j")
        .arg(jobs)
        .current_dir(&src_dir)
        .status()
        .map_err(|err| format!("failed to run make: {err}"))?;
    if !status.success() {
        return Err("make failed".to_string());
    }

    let candidates = [
        src_dir.join("whisper-whisper"),
        src_dir.join("whisper-cli"),
        src_dir.join("whisper"),
        src_dir.join("main"),
        src_dir.join("build").join("bin").join("whisper-whisper"),
        src_dir.join("build").join("bin").join("whisper-cli"),
        src_dir.join("build").join("bin").join("whisper"),
        src_dir.join("build").join("bin").join("main"),
    ];
    let built_bin = candidates
        .iter()
        .find(|path| path.exists())
        .ok_or_else(|| "whisper binary not found after build".to_string())?;
    info!("using built whisper cli: {}", built_bin.display());

    if let Some(parent) = bin_path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    std::fs::copy(built_bin, bin_path).map_err(|err| err.to_string())?;
    if let Some(bin_dir) = bin_path.parent() {
        copy_shared_libs(&src_dir.join("build"), bin_dir)?;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(bin_path, perms).map_err(|err| err.to_string())?;
    }

    env::set_var("WHISPER_CPP_BIN", bin_path);
    info!("whisper cli built: {}", bin_path.display());
    Ok(())
}

fn copy_shared_libs(build_dir: &Path, bin_dir: &Path) -> Result<(), String> {
    let mut stack = vec![build_dir.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let entries = std::fs::read_dir(&dir).map_err(|err| err.to_string())?;
        for entry in entries {
            let entry = entry.map_err(|err| err.to_string())?;
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            let name = path
                .file_name()
                .and_then(|value| value.to_str())
                .unwrap_or("");
            if name.ends_with(".so") {
                let dest = bin_dir.join(name);
                let _ = std::fs::copy(&path, &dest).map_err(|err| err.to_string())?;
                info!("copied shared lib: {}", dest.display());
            }
        }
    }
    Ok(())
}

fn command_exists(cmd: &str) -> bool {
    Command::new(cmd).arg("--version").status().is_ok()
}

fn download_bytes(url: &str) -> Result<Vec<u8>, String> {
    let response = ureq::get(url)
        .call()
        .map_err(|err| format!("download failed: {err}"))?;
    let mut reader = response.into_reader();
    let mut bytes = Vec::new();
    reader
        .read_to_end(&mut bytes)
        .map_err(|err| format!("download failed: {err}"))?;
    Ok(bytes)
}

fn extract_cli(bytes: &[u8], bin_path: &Path) -> Result<bool, String> {
    let reader = std::io::Cursor::new(bytes);
    let mut archive = zip::ZipArchive::new(reader).map_err(|err| err.to_string())?;
    let mut candidate = None;

    for i in 0..archive.len() {
        let file = archive.by_index(i).map_err(|err| err.to_string())?;
        if file.is_dir() {
            continue;
        }
        let name = file.name().replace('\\', "/");
        let filename = Path::new(&name)
            .file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("");
        if filename == "whisper-whisper"
            || filename == "whisper-cli"
            || filename == "whisper"
            || filename == "whisper.exe"
            || filename == "main"
        {
            candidate = Some((i, filename.to_string()));
            break;
        }
    }

    let Some((index, filename)) = candidate else {
        return Ok(false);
    };
    if !cfg!(windows) && filename.ends_with(".exe") {
        return Err("downloaded windows whisper.exe; no linux binary in release".to_string());
    }
    let mut file = archive.by_index(index).map_err(|err| err.to_string())?;
    let mut buffer = Vec::new();
    file.read_to_end(&mut buffer)
        .map_err(|err| err.to_string())?;

    if let Some(parent) = bin_path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    std::fs::write(bin_path, buffer).map_err(|err| err.to_string())?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let perms = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(bin_path, perms).map_err(|err| err.to_string())?;
    }

    Ok(true)
}
