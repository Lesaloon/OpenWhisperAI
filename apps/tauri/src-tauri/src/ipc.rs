use crate::logging::{logger, LogEntry};
use crate::state::AppState;
use shared_types::{AppSettings, BackendEvent, BackendState, ModelStatusPayload, SettingsUpdate};
use tauri::Manager;

pub const BACKEND_STATE_EVENT: &str = "backend-state";
pub const MODEL_STATUS_EVENT: &str = "model-download-status";

#[tauri::command]
pub fn ipc_get_state(state: tauri::State<AppState>) -> BackendState {
    let orchestrator = state.lock_orchestrator();
    orchestrator.current_state()
}

#[tauri::command]
pub fn ipc_send_event(
    event: BackendEvent,
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
) -> Result<BackendState, String> {
    let next = {
        let mut orchestrator = state.lock_orchestrator();
        orchestrator.apply_event(event.clone())?
    };
    log::info!("state transition: {:?} -> {:?}", event, next);
    let _ = app.emit_all(BACKEND_STATE_EVENT, next.clone());
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

#[tauri::command]
pub fn ipc_get_models(state: tauri::State<AppState>) -> ModelStatusPayload {
    let models = state.lock_models();
    models.snapshot()
}

#[tauri::command]
pub fn ipc_set_models(
    payload: ModelStatusPayload,
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
) -> Result<ModelStatusPayload, String> {
    let next = {
        let mut models = state.lock_models();
        models.set(payload)
    };
    let _ = app.emit_all(MODEL_STATUS_EVENT, next.clone());
    Ok(next)
}
