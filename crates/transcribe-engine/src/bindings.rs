use std::path::Path;

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
        Ok(WhisperContext { ctx })
    }

    fn transcribe(_context: &Self::Context, _audio: &[f32]) -> Result<String, BindingError> {
        Err(BindingError::Unavailable)
    }
}

#[cfg(not(feature = "whisper-ffi"))]
pub struct WhisperContext;

#[cfg(not(feature = "whisper-ffi"))]
impl WhisperBindings for WhisperCppBindings {
    type Context = WhisperContext;

    fn init_from_file(_path: &Path) -> Result<Self::Context, BindingError> {
        Err(BindingError::Unavailable)
    }

    fn transcribe(_context: &Self::Context, _audio: &[f32]) -> Result<String, BindingError> {
        Err(BindingError::Unavailable)
    }
}
