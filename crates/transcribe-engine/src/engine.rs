use crate::bindings::{BindingError, WhisperBindings, WhisperCppBindings};
use crate::model::{FsDownloader, ModelDownloader, ModelError, ModelId, ModelManager};
use std::marker::PhantomData;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TranscriptionResult {
    pub text: String,
}

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("model error: {0}")]
    Model(#[from] ModelError),
    #[error("binding error: {0}")]
    Binding(#[from] BindingError),
    #[error("audio buffer is empty")]
    EmptyAudio,
}

pub trait TranscriptionEngine {
    fn transcribe(&self, audio: &[f32]) -> Result<TranscriptionResult, EngineError>;
}

pub struct TranscriptionPipeline<
    B: WhisperBindings = WhisperCppBindings,
    D: ModelDownloader = FsDownloader,
> {
    manager: ModelManager,
    downloader: D,
    _marker: PhantomData<B>,
}

impl<B: WhisperBindings, D: ModelDownloader> TranscriptionPipeline<B, D> {
    pub fn new(manager: ModelManager, downloader: D) -> Self {
        Self {
            manager,
            downloader,
            _marker: PhantomData,
        }
    }

    pub fn transcribe(
        &self,
        model_id: ModelId,
        audio: &[f32],
    ) -> Result<TranscriptionResult, EngineError> {
        if audio.is_empty() {
            return Err(EngineError::EmptyAudio);
        }
        let model_path = self
            .manager
            .ensure_model_cached(&model_id, &self.downloader)?;
        let context = B::init_from_file(&model_path)?;
        let text = B::transcribe(&context, audio)?;
        Ok(TranscriptionResult { text })
    }
}

pub struct WhisperCppEngine<B: WhisperBindings = WhisperCppBindings> {
    _marker: PhantomData<B>,
    #[allow(dead_code)]
    model_id: ModelId,
    #[allow(dead_code)]
    context: B::Context,
}

impl WhisperCppEngine<WhisperCppBindings> {
    pub fn load(manager: &ModelManager, model_id: ModelId) -> Result<Self, EngineError> {
        Self::with_bindings(manager, model_id)
    }
}

impl<B: WhisperBindings> WhisperCppEngine<B> {
    pub fn with_bindings(manager: &ModelManager, model_id: ModelId) -> Result<Self, EngineError> {
        let model_path = manager.ensure_model_available(&model_id)?;
        let context = B::init_from_file(&model_path)?;
        Ok(Self {
            _marker: PhantomData,
            model_id,
            context,
        })
    }
}

impl<B: WhisperBindings> TranscriptionEngine for WhisperCppEngine<B> {
    fn transcribe(&self, audio: &[f32]) -> Result<TranscriptionResult, EngineError> {
        if audio.is_empty() {
            return Err(EngineError::EmptyAudio);
        }
        let text = B::transcribe(&self.context, audio)?;
        Ok(TranscriptionResult { text })
    }
}

pub struct TranscriptionWrapper<B: WhisperBindings = WhisperCppBindings> {
    engine: Option<WhisperCppEngine<B>>,
}

impl TranscriptionWrapper<WhisperCppBindings> {
    pub fn load(manager: &ModelManager, model_id: ModelId) -> Result<Self, EngineError> {
        Self::with_bindings(manager, model_id)
    }
}

impl<B: WhisperBindings> TranscriptionWrapper<B> {
    pub fn with_bindings(manager: &ModelManager, model_id: ModelId) -> Result<Self, EngineError> {
        let engine = match WhisperCppEngine::<B>::with_bindings(manager, model_id) {
            Ok(engine) => Some(engine),
            Err(EngineError::Binding(BindingError::Unavailable)) => None,
            Err(err) => return Err(err),
        };
        Ok(Self { engine })
    }

    pub fn bindings_available(&self) -> bool {
        self.engine.is_some()
    }
}

impl<B: WhisperBindings> TranscriptionEngine for TranscriptionWrapper<B> {
    fn transcribe(&self, audio: &[f32]) -> Result<TranscriptionResult, EngineError> {
        if audio.is_empty() {
            return Err(EngineError::EmptyAudio);
        }
        let empty_result = || {
            Ok(TranscriptionResult {
                text: String::new(),
            })
        };
        match &self.engine {
            Some(engine) => match engine.transcribe(audio) {
                Err(EngineError::Binding(BindingError::Unavailable)) => empty_result(),
                other => other,
            },
            None => empty_result(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bindings::BindingError;
    use crate::model::{ModelDownloader, ModelError, ModelManager, ModelSpec};
    use std::cell::Cell;

    struct MockBindings;

    struct MockContext {
        _path: std::path::PathBuf,
    }

    struct MockDownloader {
        bytes: Vec<u8>,
        calls: Cell<usize>,
    }

    impl MockDownloader {
        fn new(bytes: Vec<u8>) -> Self {
            Self {
                bytes,
                calls: Cell::new(0),
            }
        }
    }

    impl ModelDownloader for MockDownloader {
        fn download(&self, _url: &str) -> Result<Vec<u8>, ModelError> {
            self.calls.set(self.calls.get() + 1);
            Ok(self.bytes.clone())
        }
    }

    impl WhisperBindings for MockBindings {
        type Context = MockContext;

        fn init_from_file(path: &std::path::Path) -> Result<Self::Context, BindingError> {
            Ok(MockContext {
                _path: path.to_path_buf(),
            })
        }

        fn transcribe(_context: &Self::Context, _audio: &[f32]) -> Result<String, BindingError> {
            Ok("mock transcript".to_string())
        }
    }

    #[test]
    fn engine_loads_with_mock_bindings() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let mut manager = ModelManager::new(dir.path());
        let spec = ModelSpec::new(ModelId::Custom("mock".to_string()), "mock.bin").with_size(1);
        manager.register_model(spec);
        manager
            .write_model_bytes(&ModelId::Custom("mock".to_string()), &[0u8])
            .expect("write model");

        let engine = WhisperCppEngine::<MockBindings>::with_bindings(
            &manager,
            ModelId::Custom("mock".to_string()),
        );
        assert!(engine.is_ok());
    }

    #[test]
    fn engine_rejects_empty_audio() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let mut manager = ModelManager::new(dir.path());
        let spec = ModelSpec::new(ModelId::Custom("mock".to_string()), "mock.bin").with_size(1);
        manager.register_model(spec);
        manager
            .write_model_bytes(&ModelId::Custom("mock".to_string()), &[0u8])
            .expect("write model");

        let engine = WhisperCppEngine::<MockBindings>::with_bindings(
            &manager,
            ModelId::Custom("mock".to_string()),
        )
        .expect("engine loads");
        let result = engine.transcribe(&[]);
        assert!(matches!(result, Err(EngineError::EmptyAudio)));
    }

    #[test]
    fn engine_uses_bindings_for_transcription() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let mut manager = ModelManager::new(dir.path());
        let spec = ModelSpec::new(ModelId::Custom("mock".to_string()), "mock.bin").with_size(1);
        manager.register_model(spec);
        manager
            .write_model_bytes(&ModelId::Custom("mock".to_string()), &[0u8])
            .expect("write model");

        let engine = WhisperCppEngine::<MockBindings>::with_bindings(
            &manager,
            ModelId::Custom("mock".to_string()),
        )
        .expect("engine loads");
        let result = engine.transcribe(&[0.1, 0.2, 0.3]).expect("transcribe");
        assert_eq!(result.text, "mock transcript");
    }

    #[test]
    fn engine_returns_binding_errors() {
        struct ErrorBindings;

        impl WhisperBindings for ErrorBindings {
            type Context = MockContext;

            fn init_from_file(path: &std::path::Path) -> Result<Self::Context, BindingError> {
                Ok(MockContext {
                    _path: path.to_path_buf(),
                })
            }

            fn transcribe(
                _context: &Self::Context,
                _audio: &[f32],
            ) -> Result<String, BindingError> {
                Err(BindingError::Unavailable)
            }
        }

        let dir = tempfile::tempdir().expect("create tempdir");
        let mut manager = ModelManager::new(dir.path());
        let spec = ModelSpec::new(ModelId::Custom("mock".to_string()), "mock.bin").with_size(1);
        manager.register_model(spec);
        manager
            .write_model_bytes(&ModelId::Custom("mock".to_string()), &[0u8])
            .expect("write model");

        let engine = WhisperCppEngine::<ErrorBindings>::with_bindings(
            &manager,
            ModelId::Custom("mock".to_string()),
        )
        .expect("engine loads");
        let result = engine.transcribe(&[0.1]);
        assert!(matches!(
            result,
            Err(EngineError::Binding(BindingError::Unavailable))
        ));
    }

    #[test]
    fn pipeline_downloads_model_and_transcribes() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let mut manager = ModelManager::new(dir.path());
        let spec = ModelSpec::new(ModelId::Custom("pipeline".to_string()), "pipeline.bin")
            .with_download_url("file://mock")
            .with_size(4);
        manager.register_model(spec);

        let downloader = MockDownloader::new(vec![0u8; 4]);
        let pipeline = TranscriptionPipeline::<MockBindings, _>::new(manager, downloader);
        let result = pipeline
            .transcribe(ModelId::Custom("pipeline".to_string()), &[0.1])
            .expect("transcribe");
        assert_eq!(result.text, "mock transcript");
    }

    #[test]
    fn pipeline_reuses_cached_model() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let mut manager = ModelManager::new(dir.path());
        let spec = ModelSpec::new(ModelId::Custom("cached".to_string()), "cached.bin")
            .with_download_url("file://mock")
            .with_size(2);
        manager.register_model(spec);

        let downloader = MockDownloader::new(vec![1u8, 2u8]);
        let pipeline = TranscriptionPipeline::<MockBindings, _>::new(manager, downloader);
        let _ = pipeline
            .transcribe(ModelId::Custom("cached".to_string()), &[0.1])
            .expect("transcribe");
        let _ = pipeline
            .transcribe(ModelId::Custom("cached".to_string()), &[0.2])
            .expect("transcribe again");
        assert_eq!(pipeline.downloader.calls.get(), 1);
    }

    #[test]
    fn wrapper_falls_back_when_bindings_unavailable_at_load() {
        struct UnavailableBindings;

        impl WhisperBindings for UnavailableBindings {
            type Context = MockContext;

            fn init_from_file(_path: &std::path::Path) -> Result<Self::Context, BindingError> {
                Err(BindingError::Unavailable)
            }

            fn transcribe(
                _context: &Self::Context,
                _audio: &[f32],
            ) -> Result<String, BindingError> {
                Err(BindingError::Unavailable)
            }
        }

        let dir = tempfile::tempdir().expect("create tempdir");
        let mut manager = ModelManager::new(dir.path());
        let spec =
            ModelSpec::new(ModelId::Custom("wrapper".to_string()), "wrapper.bin").with_size(1);
        manager.register_model(spec);
        manager
            .write_model_bytes(&ModelId::Custom("wrapper".to_string()), &[0u8])
            .expect("write model");

        let engine = TranscriptionWrapper::<UnavailableBindings>::with_bindings(
            &manager,
            ModelId::Custom("wrapper".to_string()),
        )
        .expect("wrapper loads");
        assert!(!engine.bindings_available());
        let result = engine.transcribe(&[0.1, 0.2]).expect("fallback transcribe");
        assert_eq!(result.text, "");
    }

    #[test]
    fn wrapper_falls_back_when_transcribe_unavailable() {
        struct TranscribeUnavailableBindings;

        impl WhisperBindings for TranscribeUnavailableBindings {
            type Context = MockContext;

            fn init_from_file(path: &std::path::Path) -> Result<Self::Context, BindingError> {
                Ok(MockContext {
                    _path: path.to_path_buf(),
                })
            }

            fn transcribe(
                _context: &Self::Context,
                _audio: &[f32],
            ) -> Result<String, BindingError> {
                Err(BindingError::Unavailable)
            }
        }

        let dir = tempfile::tempdir().expect("create tempdir");
        let mut manager = ModelManager::new(dir.path());
        let spec =
            ModelSpec::new(ModelId::Custom("wrapper".to_string()), "wrapper.bin").with_size(1);
        manager.register_model(spec);
        manager
            .write_model_bytes(&ModelId::Custom("wrapper".to_string()), &[0u8])
            .expect("write model");

        let engine = TranscriptionWrapper::<TranscribeUnavailableBindings>::with_bindings(
            &manager,
            ModelId::Custom("wrapper".to_string()),
        )
        .expect("wrapper loads");
        assert!(engine.bindings_available());
        let result = engine.transcribe(&[0.1]).expect("fallback transcribe");
        assert_eq!(result.text, "");
    }

    #[test]
    fn wrapper_propagates_non_unavailable_errors() {
        struct InitFailedBindings;

        impl WhisperBindings for InitFailedBindings {
            type Context = MockContext;

            fn init_from_file(_path: &std::path::Path) -> Result<Self::Context, BindingError> {
                Err(BindingError::InitFailed)
            }

            fn transcribe(
                _context: &Self::Context,
                _audio: &[f32],
            ) -> Result<String, BindingError> {
                Ok("should-not-run".to_string())
            }
        }

        let dir = tempfile::tempdir().expect("create tempdir");
        let mut manager = ModelManager::new(dir.path());
        let spec =
            ModelSpec::new(ModelId::Custom("wrapper".to_string()), "wrapper.bin").with_size(1);
        manager.register_model(spec);
        manager
            .write_model_bytes(&ModelId::Custom("wrapper".to_string()), &[0u8])
            .expect("write model");

        let result = TranscriptionWrapper::<InitFailedBindings>::with_bindings(
            &manager,
            ModelId::Custom("wrapper".to_string()),
        );
        assert!(matches!(
            result,
            Err(EngineError::Binding(BindingError::InitFailed))
        ));
    }
}
