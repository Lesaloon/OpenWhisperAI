use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ModelId {
    Tiny,
    Base,
    Small,
    Medium,
    Large,
    Custom(String),
}

impl ModelId {
    pub fn display_name(&self) -> String {
        match self {
            ModelId::Tiny => "tiny".to_string(),
            ModelId::Base => "base".to_string(),
            ModelId::Small => "small".to_string(),
            ModelId::Medium => "medium".to_string(),
            ModelId::Large => "large".to_string(),
            ModelId::Custom(name) => name.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ModelSpec {
    pub id: ModelId,
    pub filename: String,
    pub sha256: Option<String>,
    pub size_bytes: Option<u64>,
}

impl ModelSpec {
    pub fn new(id: ModelId, filename: impl Into<String>) -> Self {
        Self {
            id,
            filename: filename.into(),
            sha256: None,
            size_bytes: None,
        }
    }

    pub fn with_sha256(mut self, sha256: impl Into<String>) -> Self {
        self.sha256 = Some(sha256.into());
        self
    }

    pub fn with_size(mut self, size_bytes: u64) -> Self {
        self.size_bytes = Some(size_bytes);
        self
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ModelError {
    #[error("model '{0}' is not registered")]
    UnregisteredModel(String),
    #[error("model filename is invalid: {0}")]
    InvalidFilename(String),
    #[error("model file not found at {0}")]
    MissingFile(String),
    #[error("model size mismatch: expected {expected} bytes, got {actual} bytes")]
    SizeMismatch { expected: u64, actual: u64 },
    #[error("model checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch { expected: String, actual: String },
    #[error("io error while handling model file")]
    Io(#[from] std::io::Error),
}

pub struct ModelManager {
    root: PathBuf,
    registry: HashMap<ModelId, ModelSpec>,
}

impl ModelManager {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self {
            root: root.into(),
            registry: HashMap::new(),
        }
    }

    pub fn register_model(&mut self, spec: ModelSpec) {
        self.registry.insert(spec.id.clone(), spec);
    }

    pub fn model_path(&self, id: &ModelId) -> Result<PathBuf, ModelError> {
        let spec = self
            .registry
            .get(id)
            .ok_or_else(|| ModelError::UnregisteredModel(id.display_name()))?;
        validate_model_filename(&spec.filename)?;
        Ok(self.root.join(&spec.filename))
    }

    pub fn ensure_model_available(&self, id: &ModelId) -> Result<PathBuf, ModelError> {
        let spec = self
            .registry
            .get(id)
            .ok_or_else(|| ModelError::UnregisteredModel(id.display_name()))?;
        validate_model_filename(&spec.filename)?;
        let path = self.root.join(&spec.filename);
        if !path.exists() {
            return Err(ModelError::MissingFile(path.display().to_string()));
        }
        let metadata = path.metadata()?;
        if let Some(expected) = spec.size_bytes {
            let actual = metadata.len();
            if actual != expected {
                return Err(ModelError::SizeMismatch { expected, actual });
            }
        }
        if let Some(expected) = spec.sha256.as_ref() {
            let actual = sha256_hex_from_file(&path)?;
            if !expected.eq_ignore_ascii_case(&actual) {
                return Err(ModelError::ChecksumMismatch {
                    expected: expected.clone(),
                    actual,
                });
            }
        }
        Ok(path)
    }

    pub fn write_model_bytes(&self, id: &ModelId, bytes: &[u8]) -> Result<PathBuf, ModelError> {
        let path = self.model_path(id)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut file = File::create(&path)?;
        file.write_all(bytes)?;
        Ok(path)
    }
}

fn validate_model_filename(filename: &str) -> Result<(), ModelError> {
    let path = Path::new(filename);
    if filename.is_empty() {
        return Err(ModelError::InvalidFilename(filename.to_string()));
    }
    if path.is_absolute() {
        return Err(ModelError::InvalidFilename(filename.to_string()));
    }
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir | Component::Prefix(_)))
    {
        return Err(ModelError::InvalidFilename(filename.to_string()));
    }
    Ok(())
}

fn sha256_hex_from_file(path: &Path) -> Result<String, ModelError> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 8192];
    loop {
        let read_bytes = file.read(&mut buffer)?;
        if read_bytes == 0 {
            break;
        }
        hasher.update(&buffer[..read_bytes]);
    }
    Ok(hex::encode(hasher.finalize()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sha256_hex(bytes: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        hex::encode(hasher.finalize())
    }

    #[test]
    fn model_manager_resolves_registered_path() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let mut manager = ModelManager::new(dir.path());
        let spec = ModelSpec::new(ModelId::Custom("sample".to_string()), "sample.bin");
        manager.register_model(spec);

        let path = manager
            .model_path(&ModelId::Custom("sample".to_string()))
            .expect("path should resolve");
        assert!(path.ends_with("sample.bin"));
    }

    #[test]
    fn model_manager_validates_checksum_and_size() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let mut manager = ModelManager::new(dir.path());
        let bytes = b"hello whisper";
        let checksum = sha256_hex(bytes);
        let spec = ModelSpec::new(ModelId::Custom("checksum".to_string()), "checksum.bin")
            .with_sha256(checksum)
            .with_size(bytes.len() as u64);
        manager.register_model(spec);
        manager
            .write_model_bytes(&ModelId::Custom("checksum".to_string()), bytes)
            .expect("write model");

        let path = manager
            .ensure_model_available(&ModelId::Custom("checksum".to_string()))
            .expect("model should be valid");
        assert!(path.exists());
    }

    #[test]
    fn model_manager_rejects_wrong_checksum() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let mut manager = ModelManager::new(dir.path());
        let bytes = b"hello whisper";
        let spec = ModelSpec::new(ModelId::Custom("bad".to_string()), "bad.bin")
            .with_sha256("deadbeef".to_string());
        manager.register_model(spec);
        manager
            .write_model_bytes(&ModelId::Custom("bad".to_string()), bytes)
            .expect("write model");

        let result = manager.ensure_model_available(&ModelId::Custom("bad".to_string()));
        assert!(matches!(result, Err(ModelError::ChecksumMismatch { .. })));
    }

    #[test]
    fn model_manager_rejects_path_traversal_filenames() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let mut manager = ModelManager::new(dir.path());
        let spec = ModelSpec::new(ModelId::Custom("bad".to_string()), "../bad.bin");
        manager.register_model(spec);

        let result = manager.model_path(&ModelId::Custom("bad".to_string()));
        assert!(matches!(result, Err(ModelError::InvalidFilename(_))));
    }

    #[test]
    fn model_manager_rejects_absolute_filenames() {
        let dir = tempfile::tempdir().expect("create tempdir");
        let mut manager = ModelManager::new(dir.path());
        let spec = ModelSpec::new(ModelId::Custom("abs".to_string()), "/tmp/abs.bin");
        manager.register_model(spec);

        let result = manager.model_path(&ModelId::Custom("abs".to_string()));
        assert!(matches!(result, Err(ModelError::InvalidFilename(_))));
    }
}
