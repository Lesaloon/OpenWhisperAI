use crate::logging::{logger, LogEntry};
use crate::state::AppState;
use shared_types::{AppSettings, BackendEvent, BackendState, SettingsUpdate};

#[tauri::command]
pub fn ipc_get_state(state: tauri::State<AppState>) -> BackendState {
    let orchestrator = state.lock_orchestrator();
    orchestrator.current_state()
}

#[tauri::command]
pub fn ipc_send_event(
    event: BackendEvent,
    state: tauri::State<AppState>,
) -> Result<BackendState, String> {
    let mut orchestrator = state.lock_orchestrator();
    let next = orchestrator.apply_event(event.clone())?;
    log::info!("state transition: {:?} -> {:?}", event, next);
    Ok(next)
}

#[tauri::command]
pub fn ipc_get_settings(state: tauri::State<AppState>) -> AppSettings {
    let orchestrator = state.lock_orchestrator();
    orchestrator.settings()
}

#[tauri::command]
pub fn ipc_update_settings(
    update: SettingsUpdate,
    state: tauri::State<AppState>,
) -> Result<AppSettings, String> {
    let mut orchestrator = state.lock_orchestrator();
    let next = orchestrator.update_settings(update)?;
    log::info!("settings updated");
    Ok(next)
}

#[tauri::command]
pub fn ipc_set_settings(
    settings: AppSettings,
    state: tauri::State<AppState>,
) -> Result<AppSettings, String> {
    let mut orchestrator = state.lock_orchestrator();
    let next = orchestrator.set_settings(settings)?;
    log::info!("settings replaced");
    Ok(next)
}

#[tauri::command]
pub fn ipc_get_logs() -> Vec<LogEntry> {
    logger().entries()
}
