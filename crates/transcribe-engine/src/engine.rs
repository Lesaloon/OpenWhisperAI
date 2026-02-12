use crate::bindings::{BindingError, WhisperBindings, WhisperCppBindings};
use crate::model::{ModelError, ModelId, ModelManager};
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
        Ok(TranscriptionResult {
            text: String::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bindings::BindingError;
    use crate::model::{ModelManager, ModelSpec};

    struct MockBindings;

    struct MockContext {
        _path: std::path::PathBuf,
    }

    impl WhisperBindings for MockBindings {
        type Context = MockContext;

        fn init_from_file(path: &std::path::Path) -> Result<Self::Context, BindingError> {
            Ok(MockContext {
                _path: path.to_path_buf(),
            })
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
}
