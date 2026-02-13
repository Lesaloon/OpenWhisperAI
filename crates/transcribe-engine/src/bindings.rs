use log::warn;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, thiserror::Error)]
pub enum BindingError {
    #[error("whisper.cpp bindings unavailable")]
    Unavailable,
    #[error("failed to initialize whisper.cpp context")]
    InitFailed,
}

pub trait WhisperBindings {
    type Context;

    fn init_from_file(path: &Path) -> Result<Self::Context, BindingError>;
    fn transcribe(context: &Self::Context, audio: &[f32]) -> Result<String, BindingError>;
}

pub struct WhisperCppBindings;

const WHISPER_SAMPLE_RATE: u32 = 16_000;
const WHISPER_BITS_PER_SAMPLE: u16 = 16;

fn resolve_whisper_bin() -> std::ffi::OsString {
    std::env::var_os("WHISPER_CPP_BIN").unwrap_or_else(|| "whisper".into())
}

fn parse_cli_output(output: &str) -> String {
    let mut parts = Vec::new();
    for line in output.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        if line.starts_with('[') {
            if let Some((_, text)) = line.rsplit_once(']') {
                let text = text.trim();
                if !text.is_empty() {
                    parts.push(text.to_string());
                }
            }
            continue;
        }
        parts.push(line.to_string());
    }
    parts.join(" ").trim().to_string()
}

fn write_wav(path: &Path, audio: &[f32]) -> Result<(), BindingError> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: WHISPER_SAMPLE_RATE,
        bits_per_sample: WHISPER_BITS_PER_SAMPLE,
        sample_format: hound::SampleFormat::Int,
    };
    let mut writer = hound::WavWriter::create(path, spec).map_err(|_| BindingError::InitFailed)?;
    for sample in audio {
        let scaled = (sample.clamp(-1.0, 1.0) * i16::MAX as f32) as i16;
        writer
            .write_sample(scaled)
            .map_err(|_| BindingError::InitFailed)?;
    }
    writer.finalize().map_err(|_| BindingError::InitFailed)
}

fn run_whisper_cli_with_bin(
    bin: &std::ffi::OsStr,
    model_path: &Path,
    audio: &[f32],
) -> Result<String, BindingError> {
    let bin_path = Path::new(bin);
    let bin_dir = bin_path.parent();
    let temp_dir = tempfile::tempdir().map_err(|_| BindingError::InitFailed)?;
    let wav_path = temp_dir.path().join("audio.wav");
    write_wav(&wav_path, audio)?;
    let output_prefix = temp_dir.path().join("whisper-output");

    let mut command = Command::new(bin);
    if cfg!(target_os = "linux") {
        if let Some(dir) = bin_dir {
            let current = env::var_os("LD_LIBRARY_PATH").unwrap_or_default();
            let mut value = dir.as_os_str().to_os_string();
            if !current.is_empty() {
                value.push(":");
                value.push(current);
            }
            command.env("LD_LIBRARY_PATH", value);
        }
    }

    let output = command
        .arg("-m")
        .arg(model_path)
        .arg("-f")
        .arg(&wav_path)
        .arg("-l")
        .arg("auto")
        .arg("-otxt")
        .arg("-of")
        .arg(&output_prefix)
        .output()
        .map_err(|err| {
            if err.kind() == std::io::ErrorKind::NotFound {
                BindingError::Unavailable
            } else {
                BindingError::InitFailed
            }
        })?;

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        warn!(
            "whisper cli failed: status={} stdout='{}' stderr='{}'",
            output.status,
            stdout.trim(),
            stderr.trim()
        );
        return Err(BindingError::InitFailed);
    }

    let output_path = output_prefix.with_extension("txt");
    if let Ok(contents) = std::fs::read_to_string(&output_path) {
        let trimmed = contents.trim();
        if !trimmed.is_empty() {
            return Ok(trimmed.to_string());
        }
    }

    Ok(String::new())
}

fn transcribe_with_cli(model_path: &Path, audio: &[f32]) -> Result<String, BindingError> {
    let bin = resolve_whisper_bin();
    run_whisper_cli_with_bin(bin.as_os_str(), model_path, audio)
}

#[cfg(feature = "whisper-ffi")]
mod ffi {
    use std::os::raw::c_char;

    #[repr(C)]
    pub struct whisper_context {
        _private: [u8; 0],
    }

    extern "C" {
        pub fn whisper_init_from_file(path: *const c_char) -> *mut whisper_context;
        pub fn whisper_free(ctx: *mut whisper_context);
    }
}

#[cfg(feature = "whisper-ffi")]
pub struct WhisperContext {
    ctx: std::ptr::NonNull<ffi::whisper_context>,
    model_path: PathBuf,
}

#[cfg(feature = "whisper-ffi")]
impl Drop for WhisperContext {
    fn drop(&mut self) {
        unsafe {
            ffi::whisper_free(self.ctx.as_ptr());
        }
    }
}

#[cfg(feature = "whisper-ffi")]
impl WhisperBindings for WhisperCppBindings {
    type Context = WhisperContext;

    fn init_from_file(path: &Path) -> Result<Self::Context, BindingError> {
        let c_path = std::ffi::CString::new(path.to_string_lossy().as_bytes())
            .map_err(|_| BindingError::InitFailed)?;
        let ctx = unsafe { ffi::whisper_init_from_file(c_path.as_ptr()) };
        let ctx = std::ptr::NonNull::new(ctx).ok_or(BindingError::InitFailed)?;
        Ok(WhisperContext {
            ctx,
            model_path: path.to_path_buf(),
        })
    }

    fn transcribe(context: &Self::Context, audio: &[f32]) -> Result<String, BindingError> {
        transcribe_with_cli(&context.model_path, audio)
    }
}

#[cfg(not(feature = "whisper-ffi"))]
pub struct WhisperContext {
    model_path: PathBuf,
}

#[cfg(not(feature = "whisper-ffi"))]
impl WhisperBindings for WhisperCppBindings {
    type Context = WhisperContext;

    fn init_from_file(path: &Path) -> Result<Self::Context, BindingError> {
        Ok(WhisperContext {
            model_path: path.to_path_buf(),
        })
    }

    fn transcribe(context: &Self::Context, audio: &[f32]) -> Result<String, BindingError> {
        transcribe_with_cli(&context.model_path, audio)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn parse_cli_output_strips_timestamps() {
        let output = "[00:00.000 --> 00:01.000] Hello\n[00:01.000 --> 00:02.000] world";
        assert_eq!(parse_cli_output(output), "Hello world");
    }

    #[test]
    fn write_wav_encodes_pcm16() {
        let dir = tempfile::tempdir().expect("tempdir");
        let wav_path = dir.path().join("sample.wav");
        write_wav(&wav_path, &[0.0, 1.0]).expect("write wav");
        let bytes = fs::read(&wav_path).expect("read wav");
        assert!(bytes.starts_with(b"RIFF"));
        assert_eq!(bytes[8..12], *b"WAVE");
        assert_eq!(bytes[36..40], *b"data");
        assert_eq!(bytes.len(), 44 + 4);
    }

    #[test]
    fn run_whisper_cli_reads_output_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let bin_path = dir.path().join("whisper-mock");
        let script = "#!/bin/sh\n".to_string()
            + "out=\"\"\n"
            + "while [ \"$#\" -gt 0 ]; do\n"
            + "  if [ \"$1\" = \"-of\" ]; then\n"
            + "    shift\n"
            + "    out=\"$1\"\n"
            + "  fi\n"
            + "  shift\n"
            + "done\n"
            + "if [ -z \"$out\" ]; then\n"
            + "  exit 1\n"
            + "fi\n"
            + "printf \"%s\" \"mock transcript\" > \"${out}.txt\"\n"
            + "exit 0\n";
        fs::write(&bin_path, script).expect("write script");
        let mut perms = fs::metadata(&bin_path).expect("metadata").permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&bin_path, perms).expect("set perms");

        let model_path = dir.path().join("model.bin");
        fs::write(&model_path, "model").expect("write model");
        let result = run_whisper_cli_with_bin(bin_path.as_os_str(), &model_path, &[0.0, 0.1])
            .expect("transcribe");
        assert_eq!(result, "mock transcript");
    }
}
