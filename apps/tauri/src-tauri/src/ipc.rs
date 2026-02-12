use crate::logging::{logger, LogEntry};
use crate::state::{AppState, BackendEvent, BackendState};

#[tauri::command]
pub fn ipc_get_state(state: tauri::State<AppState>) -> BackendState {
    let machine = state.machine.lock().expect("state lock poisoned");
    machine.current()
}

#[tauri::command]
pub fn ipc_send_event(
    event: BackendEvent,
    state: tauri::State<AppState>,
) -> Result<BackendState, String> {
    let mut machine = state.machine.lock().expect("state lock poisoned");
    let next = machine.apply(event.clone())?;
    log::info!("state transition: {:?} -> {:?}", event, next);
    Ok(next)
}

#[tauri::command]
pub fn ipc_get_logs() -> Vec<LogEntry> {
    logger().entries()
}
