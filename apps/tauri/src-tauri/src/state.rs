use serde::{Deserialize, Serialize};
use std::sync::{Mutex, MutexGuard};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BackendState {
    Idle,
    Recording,
    Processing,
    Error { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BackendEvent {
    StartRecording,
    StopRecording,
    StartProcessing,
    FinishProcessing,
    Fail { message: String },
    Reset,
}

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
            (BackendState::Error { .. }, BackendEvent::Reset) => BackendState::Idle,
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

pub struct AppState {
    pub machine: Mutex<StateMachine>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            machine: Mutex::new(StateMachine::new()),
        }
    }

    pub fn lock_machine(&self) -> MutexGuard<'_, StateMachine> {
        self.machine
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

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
    fn invalid_transition_keeps_state() {
        let mut machine = StateMachine::new();

        let err = machine.apply(BackendEvent::FinishProcessing).unwrap_err();
        assert!(err.contains("invalid transition"));
        assert_eq!(machine.current(), BackendState::Idle);
    }

    #[test]
    fn lock_machine_recovers_from_poison() {
        let state = Arc::new(AppState::new());
        let state_clone = Arc::clone(&state);

        let _ = std::thread::spawn(move || {
            let _guard = state_clone.machine.lock().unwrap();
            panic!("poison lock");
        })
        .join();

        assert!(state.machine.is_poisoned());
        let guard = state.lock_machine();
        assert_eq!(guard.current(), BackendState::Idle);
    }
}
