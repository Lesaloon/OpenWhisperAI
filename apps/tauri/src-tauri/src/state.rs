use shared_types::{AppSettings, BackendEvent, BackendState, SettingsUpdate};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, MutexGuard},
};

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
}

impl BackendOrchestrator {
    pub fn new(settings_path: PathBuf) -> Self {
        Self {
            machine: StateMachine::new(),
            settings: SettingsStore::new(settings_path),
        }
    }

    pub fn current_state(&self) -> BackendState {
        self.machine.current()
    }

    pub fn apply_event(&mut self, event: BackendEvent) -> Result<BackendState, String> {
        self.machine.apply(event)
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
}

impl AppState {
    pub fn new(settings_path: PathBuf) -> Self {
        Self {
            orchestrator: Mutex::new(BackendOrchestrator::new(settings_path)),
        }
    }

    pub fn lock_orchestrator(&self) -> MutexGuard<'_, BackendOrchestrator> {
        self.orchestrator
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

pub fn default_settings_path(config_dir: Option<PathBuf>) -> PathBuf {
    let base = config_dir
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| std::env::temp_dir()));
    base.join("settings.json")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    fn temp_settings_path() -> PathBuf {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        std::env::temp_dir().join(format!("openwhisperai-settings-{stamp}.json"))
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
    fn lock_orchestrator_recovers_from_poison() {
        let path = temp_settings_path();
        let state = Arc::new(AppState::new(path));
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

        let reloaded = SettingsStore::new(path).settings();
        assert_eq!(updated, reloaded);
        let _ = fs::remove_file(&path);
    }
}
