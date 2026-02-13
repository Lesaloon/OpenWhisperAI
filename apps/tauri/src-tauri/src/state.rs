use crate::{logging::emit_app_event, ptt::PttHandle};
use shared_types::{
    AppSettings, BackendEvent, BackendState, ModelInstallStatus, ModelStatusItem,
    ModelStatusPayload, PttState, SettingsUpdate,
};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, MutexGuard},
};

const BACKEND_STATE_EVENT: &str = "backend-state";

pub struct StateMachine {
    state: BackendState,
}

impl StateMachine {
    pub fn new() -> Self {
        Self {
            state: BackendState::Idle,
        }
    }

    pub fn current(&self) -> BackendState {
        self.state.clone()
    }

    pub fn apply(&mut self, event: BackendEvent) -> Result<BackendState, String> {
        let next = match (&self.state, event) {
            (BackendState::Idle, BackendEvent::StartRecording) => BackendState::Recording,
            (BackendState::Idle, BackendEvent::StartProcessing) => BackendState::Processing,
            (BackendState::Recording, BackendEvent::StopRecording) => BackendState::Processing,
            (BackendState::Processing, BackendEvent::FinishProcessing) => BackendState::Idle,
            (_, BackendEvent::Fail { message }) => BackendState::Error { message },
            (_, BackendEvent::Reset) => BackendState::Idle,
            (state, event) => {
                return Err(format!(
                    "invalid transition from {:?} with {:?}",
                    state, event
                ))
            }
        };

        self.state = next;
        Ok(self.state.clone())
    }
}

pub trait BackendStateEmitter: Send + Sync {
    fn emit_state(&self, state: &BackendState);
}

#[derive(Clone, Default)]
pub struct AppStateEmitter;

impl BackendStateEmitter for AppStateEmitter {
    fn emit_state(&self, state: &BackendState) {
        emit_app_event(BACKEND_STATE_EVENT, state);
    }
}

pub struct SettingsStore {
    path: PathBuf,
    settings: AppSettings,
}

impl SettingsStore {
    pub fn new(path: PathBuf) -> Self {
        let settings = load_settings(&path).unwrap_or_default();
        Self { path, settings }
    }

    pub fn settings(&self) -> AppSettings {
        self.settings.clone()
    }

    pub fn set(&mut self, settings: AppSettings) -> Result<AppSettings, String> {
        self.settings = settings;
        self.persist()?;
        Ok(self.settings.clone())
    }

    pub fn update(&mut self, update: SettingsUpdate) -> Result<AppSettings, String> {
        self.settings = self.settings.apply_update(update);
        self.persist()?;
        Ok(self.settings.clone())
    }

    fn persist(&self) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|err| err.to_string())?;
        }
        let payload = serde_json::to_vec_pretty(&self.settings).map_err(|err| err.to_string())?;
        fs::write(&self.path, payload).map_err(|err| err.to_string())
    }
}

fn load_settings(path: &Path) -> Result<AppSettings, String> {
    let payload = fs::read_to_string(path).map_err(|err| err.to_string())?;
    serde_json::from_str(&payload).map_err(|err| err.to_string())
}

pub struct BackendOrchestrator {
    machine: StateMachine,
    settings: SettingsStore,
    emitter: Option<Arc<dyn BackendStateEmitter>>,
}

impl BackendOrchestrator {
    pub fn new(settings_path: PathBuf) -> Self {
        Self {
            machine: StateMachine::new(),
            settings: SettingsStore::new(settings_path),
            emitter: Some(Arc::new(AppStateEmitter::default())),
        }
    }

    pub fn with_emitter(settings_path: PathBuf, emitter: Arc<dyn BackendStateEmitter>) -> Self {
        Self {
            machine: StateMachine::new(),
            settings: SettingsStore::new(settings_path),
            emitter: Some(emitter),
        }
    }

    pub fn current_state(&self) -> BackendState {
        self.machine.current()
    }

    pub fn apply_event(&mut self, event: BackendEvent) -> Result<BackendState, String> {
        let next = self.machine.apply(event)?;
        if let Some(emitter) = &self.emitter {
            emitter.emit_state(&next);
        }
        Ok(next)
    }

    pub fn settings(&self) -> AppSettings {
        self.settings.settings()
    }

    pub fn update_settings(&mut self, update: SettingsUpdate) -> Result<AppSettings, String> {
        self.settings.update(update)
    }

    pub fn set_settings(&mut self, settings: AppSettings) -> Result<AppSettings, String> {
        self.settings.set(settings)
    }
}

pub struct AppState {
    pub orchestrator: Mutex<BackendOrchestrator>,
    pub models: Arc<Mutex<ModelStore>>,
    pub ptt: PttHandle,
    model_root: PathBuf,
}

impl AppState {
    pub fn new(settings_path: PathBuf, model_root: PathBuf) -> Self {
        let models = Arc::new(Mutex::new(ModelStore::new()));
        if let Ok(mut store) = models.lock() {
            let payload =
                crate::ptt::build_model_status_payload(&model_root, None, &HashMap::new());
            let _ = store.set_models(payload.models);
            let _ = store.set_active_model(payload.active_model);
        }
        Self {
            orchestrator: Mutex::new(BackendOrchestrator::new(settings_path)),
            models: Arc::clone(&models),
            ptt: PttHandle::new(model_root.clone(), models),
            model_root,
        }
    }

    pub fn lock_orchestrator(&self) -> MutexGuard<'_, BackendOrchestrator> {
        self.orchestrator
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn lock_models(&self) -> MutexGuard<'_, ModelStore> {
        self.models
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    pub fn ptt_handle(&self) -> PttHandle {
        self.ptt.clone()
    }

    pub fn ptt_state(&self) -> PttState {
        self.ptt.state()
    }

    pub fn model_root(&self) -> PathBuf {
        self.model_root.clone()
    }
}

pub struct ModelStore {
    models: Vec<ModelStatusItem>,
    active_model: Option<String>,
    overrides: HashMap<String, ModelInstallStatus>,
    last_transcript: Option<String>,
}

impl ModelStore {
    pub fn new() -> Self {
        Self {
            models: Vec::new(),
            active_model: None,
            overrides: HashMap::new(),
            last_transcript: None,
        }
    }

    pub fn snapshot(&self) -> ModelStatusPayload {
        ModelStatusPayload {
            models: self.models.clone(),
            active_model: self.active_model.clone(),
            queue_count: queue_count(&self.models),
        }
    }

    pub fn set(&mut self, payload: ModelStatusPayload) -> ModelStatusPayload {
        self.models = payload.models;
        self.active_model = payload.active_model;
        self.snapshot()
    }

    pub fn set_models(&mut self, models: Vec<ModelStatusItem>) -> ModelStatusPayload {
        self.models = models;
        self.snapshot()
    }

    pub fn set_active_model(&mut self, active_model: Option<String>) -> ModelStatusPayload {
        self.active_model = active_model;
        self.snapshot()
    }

    pub fn set_override(&mut self, id: impl Into<String>, status: ModelInstallStatus) {
        self.overrides.insert(id.into(), status);
    }

    pub fn clear_override(&mut self, id: &str) {
        self.overrides.remove(id);
    }

    pub fn overrides_snapshot(&self) -> HashMap<String, ModelInstallStatus> {
        self.overrides.clone()
    }

    pub fn active_model(&self) -> Option<String> {
        self.active_model.clone()
    }

    pub fn set_last_transcript(&mut self, text: String) {
        self.last_transcript = Some(text);
    }

    pub fn last_transcript(&self) -> Option<String> {
        self.last_transcript.clone()
    }
}

fn queue_count(models: &[ModelStatusItem]) -> usize {
    models
        .iter()
        .filter(|model| {
            matches!(
                model.status,
                ModelInstallStatus::Downloading
                    | ModelInstallStatus::Queued
                    | ModelInstallStatus::Pending
            )
        })
        .count()
}

pub fn default_settings_path(config_dir: Option<PathBuf>) -> PathBuf {
    let base = config_dir
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| std::env::temp_dir()));
    base.join("settings.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    fn temp_settings_path() -> PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        std::env::temp_dir().join(format!("openwhisperai-settings-{stamp}.json"))
    }

    #[derive(Default)]
    struct TestEmitter {
        states: Mutex<Vec<BackendState>>,
    }

    impl TestEmitter {
        fn states(&self) -> Vec<BackendState> {
            self.states
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .clone()
        }
    }

    impl BackendStateEmitter for TestEmitter {
        fn emit_state(&self, state: &BackendState) {
            let mut guard = self
                .states
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            guard.push(state.clone());
        }
    }

    #[test]
    fn state_machine_happy_path() {
        let mut machine = StateMachine::new();

        assert_eq!(machine.current(), BackendState::Idle);
        assert_eq!(
            machine.apply(BackendEvent::StartRecording).unwrap(),
            BackendState::Recording
        );
        assert_eq!(
            machine.apply(BackendEvent::StopRecording).unwrap(),
            BackendState::Processing
        );
        assert_eq!(
            machine.apply(BackendEvent::FinishProcessing).unwrap(),
            BackendState::Idle
        );
    }

    #[test]
    fn state_machine_processing_from_idle() {
        let mut machine = StateMachine::new();

        assert_eq!(
            machine.apply(BackendEvent::StartProcessing).unwrap(),
            BackendState::Processing
        );
    }

    #[test]
    fn state_machine_error_and_reset() {
        let mut machine = StateMachine::new();

        assert_eq!(
            machine
                .apply(BackendEvent::Fail {
                    message: "boom".to_string(),
                })
                .unwrap(),
            BackendState::Error {
                message: "boom".to_string()
            }
        );
        assert_eq!(
            machine.apply(BackendEvent::Reset).unwrap(),
            BackendState::Idle
        );
    }

    #[test]
    fn reset_from_processing_is_allowed() {
        let mut machine = StateMachine::new();
        let _ = machine.apply(BackendEvent::StartProcessing).unwrap();
        assert_eq!(
            machine.apply(BackendEvent::Reset).unwrap(),
            BackendState::Idle
        );
    }

    #[test]
    fn invalid_transition_keeps_state() {
        let mut machine = StateMachine::new();

        let err = machine.apply(BackendEvent::FinishProcessing).unwrap_err();
        assert!(err.contains("invalid transition"));
        assert_eq!(machine.current(), BackendState::Idle);
    }

    #[test]
    fn orchestrator_emits_state_changes() {
        let path = temp_settings_path();
        let emitter = Arc::new(TestEmitter::default());
        let mut orchestrator = BackendOrchestrator::with_emitter(
            path,
            Arc::clone(&emitter) as Arc<dyn BackendStateEmitter>,
        );

        let next = orchestrator
            .apply_event(BackendEvent::StartRecording)
            .unwrap();

        assert_eq!(next, BackendState::Recording);
        assert_eq!(emitter.states(), vec![BackendState::Recording]);
    }

    #[test]
    fn lock_orchestrator_recovers_from_poison() {
        let path = temp_settings_path();
        let state = Arc::new(AppState::new(path, std::env::temp_dir()));
        let state_clone = Arc::clone(&state);

        let _ = std::thread::spawn(move || {
            let _guard = state_clone.orchestrator.lock().unwrap();
            panic!("poison lock");
        })
        .join();

        assert!(state.orchestrator.is_poisoned());
        let guard = state.lock_orchestrator();
        assert_eq!(guard.current_state(), BackendState::Idle);
    }

    #[test]
    fn settings_store_persists_updates() {
        let path = temp_settings_path();
        let mut store = SettingsStore::new(path.clone());

        let updated = store
            .update(SettingsUpdate {
                latency_ms: Some(850),
                auto_export: Some(false),
                ..SettingsUpdate::default()
            })
            .unwrap();

        let reloaded = SettingsStore::new(path.clone()).settings();
        assert_eq!(updated, reloaded);
        let _ = fs::remove_file(&path);
    }
}
