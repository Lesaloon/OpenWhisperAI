use crate::logging::{logger, LogEntry};
use crate::ptt::{
    build_model_status_payload, model_id_from_name, register_standard_models, PttHotkeyPayload,
};
use crate::state::AppState;
use shared_types::{
    AppSettings, BackendEvent, BackendState, ModelInstallStatus, ModelStatusPayload, PttState,
    SettingsUpdate,
};
use std::thread;
use tauri::Manager;
use transcribe_engine::{HttpDownloader, ModelManager};

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
    state.ptt_handle().update_settings(next.clone());
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
    state.ptt_handle().update_settings(next.clone());
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
pub fn ipc_get_last_transcript(state: tauri::State<AppState>) -> Option<String> {
    let models = state.lock_models();
    models.last_transcript()
}

#[tauri::command]
pub fn ipc_model_select(
    model: String,
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
) -> Result<ModelStatusPayload, String> {
    let model_name = model.trim().to_string();
    let payload = {
        let mut models = state.lock_models();
        let active_model = if model_name.is_empty() {
            None
        } else {
            Some(model_name.clone())
        };
        let overrides = models.overrides_snapshot();
        let payload =
            build_model_status_payload(&state.model_root(), active_model.as_deref(), &overrides);
        let _ = models.set_models(payload.models.clone());
        let _ = models.set_active_model(payload.active_model.clone());
        payload
    };
    state
        .ptt_handle()
        .set_active_model(payload.active_model.clone());
    let _ = app.emit_all(MODEL_STATUS_EVENT, payload.clone());
    Ok(payload)
}

#[tauri::command]
pub fn ipc_model_download(
    model: String,
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
) -> Result<ModelStatusPayload, String> {
    let model_name = model.trim().to_string();
    if model_name.is_empty() {
        return Err("model name required".to_string());
    }
    let model_root = state.model_root();
    let models_handle = state.models.clone();
    let payload = {
        let mut models = state.lock_models();
        models.set_override(model_name.clone(), ModelInstallStatus::Downloading);
        let overrides = models.overrides_snapshot();
        let active = models.active_model();
        let payload = build_model_status_payload(&model_root, active.as_deref(), &overrides);
        let _ = models.set_models(payload.models.clone());
        let _ = models.set_active_model(payload.active_model.clone());
        payload
    };
    let _ = app.emit_all(MODEL_STATUS_EVENT, payload.clone());

    let app_handle = app.clone();
    thread::spawn(move || {
        let result = (|| {
            let mut manager = ModelManager::new(model_root.clone());
            register_standard_models(&mut manager);
            let model_id = model_id_from_name(Some(&model_name));
            if matches!(model_id, transcribe_engine::ModelId::Custom(_)) {
                return Err("custom model download not supported".to_string());
            }
            let downloader = HttpDownloader;
            manager
                .ensure_model_cached(&model_id, &downloader)
                .map(|_| ())
                .map_err(|err| err.to_string())
        })();

        let payload = {
            let mut models = models_handle
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            match result {
                Ok(()) => models.clear_override(&model_name),
                Err(_) => models.set_override(model_name.clone(), ModelInstallStatus::Failed),
            }
            let overrides = models.overrides_snapshot();
            let active = models.active_model();
            let payload = build_model_status_payload(&model_root, active.as_deref(), &overrides);
            let _ = models.set_models(payload.models.clone());
            let _ = models.set_active_model(payload.active_model.clone());
            payload
        };

        if let Err(err) = &result {
            log::warn!("model download failed: {err}");
        }
        let _ = app_handle.emit_all(MODEL_STATUS_EVENT, payload);
    });

    Ok(payload)
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
    state
        .ptt_handle()
        .set_active_model(next.active_model.clone());
    let _ = app.emit_all(MODEL_STATUS_EVENT, next.clone());
    Ok(next)
}

#[tauri::command]
pub fn ipc_ptt_start(state: tauri::State<AppState>) -> Result<PttState, String> {
    let settings = state.lock_orchestrator().settings();
    let active_model = state.lock_models().snapshot().active_model;
    let handle = state.ptt_handle();
    let result = handle.start(settings, active_model);
    if let Ok(next) = &result {
        log::info!("ptt start -> {next:?}");
    } else if let Err(err) = &result {
        log::warn!("ptt start failed: {err}");
    }
    result
}

#[tauri::command]
pub fn ipc_ptt_stop(state: tauri::State<AppState>) -> Result<PttState, String> {
    let result = state.ptt_handle().stop();
    if let Ok(next) = &result {
        log::info!("ptt stop -> {next:?}");
    } else if let Err(err) = &result {
        log::warn!("ptt stop failed: {err}");
    }
    result
}

#[tauri::command]
pub fn ipc_ptt_toggle_recording(state: tauri::State<AppState>) -> Result<PttState, String> {
    let result = state.ptt_handle().manual_toggle();
    if let Ok(next) = &result {
        log::info!("ptt manual toggle -> {next:?}");
    } else if let Err(err) = &result {
        log::warn!("ptt manual toggle failed: {err}");
    }
    result
}

#[tauri::command]
pub fn ipc_ptt_set_hotkey(
    payload: PttHotkeyPayload,
    state: tauri::State<AppState>,
) -> Result<PttHotkeyPayload, String> {
    state.ptt_handle().set_hotkey(payload)
}

#[tauri::command]
pub fn ipc_ptt_get_state(state: tauri::State<AppState>) -> PttState {
    state.ptt_state()
}

#[tauri::command]
pub fn ipc_hello() -> String {
    println!("hello from UI");
    eprintln!("hello from UI");
    log::info!("hello from UI");
    "hello from backend".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipc_hello_returns_message() {
        assert_eq!(ipc_hello(), "hello from backend");
    }
}
