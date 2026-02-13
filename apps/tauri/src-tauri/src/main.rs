mod ipc;
mod logging;
mod ptt;
mod state;
mod ui_server;

use ipc::{
    ipc_get_logs, ipc_get_models, ipc_get_settings, ipc_get_state, ipc_hello, ipc_ptt_get_state,
    ipc_ptt_set_hotkey, ipc_ptt_start, ipc_ptt_stop, ipc_ptt_toggle_recording, ipc_send_event,
    ipc_set_models, ipc_set_settings, ipc_update_settings, BACKEND_STATE_EVENT, MODEL_STATUS_EVENT,
};
use logging::{attach_app_handle, init_logging};
use ptt::PTT_STATE_EVENT;
use tauri::Manager;

fn main() {
    init_logging();

    let context = tauri::generate_context!();
    tauri::Builder::default()
        .setup(|app| {
            ui_server::maybe_start();
            let settings_path = state::default_settings_path(app.path_resolver().app_config_dir());
            let model_root = app
                .path_resolver()
                .app_data_dir()
                .unwrap_or_else(|| std::env::temp_dir())
                .join("models");
            log::info!("model root: {}", model_root.display());
            app.manage(state::AppState::new(settings_path, model_root));
            attach_app_handle(app.handle());
            let app_state = app.state::<state::AppState>();
            let settings = app_state.lock_orchestrator().settings();
            let active_model = app_state.lock_models().snapshot().active_model;
            if let Err(err) = app_state.ptt_handle().start(settings, active_model) {
                log::warn!("failed to auto-start ptt: {err}");
            } else {
                log::info!("ptt auto-started");
            }
            let backend_state = app_state.lock_orchestrator().current_state();
            let models = app_state.lock_models().snapshot();
            let ptt_state = app_state.ptt_state();
            let _ = app.emit_all(BACKEND_STATE_EVENT, backend_state);
            let _ = app.emit_all(MODEL_STATUS_EVENT, models);
            let _ = app.emit_all(PTT_STATE_EVENT, ptt_state);
            log::info!("tauri backend initialized");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ipc_get_state,
            ipc_send_event,
            ipc_get_settings,
            ipc_update_settings,
            ipc_set_settings,
            ipc_get_logs,
            ipc_get_models,
            ipc_set_models,
            ipc_ptt_start,
            ipc_ptt_stop,
            ipc_ptt_toggle_recording,
            ipc_ptt_set_hotkey,
            ipc_ptt_get_state,
            ipc_hello
        ])
        .run(context)
        .expect("error while running tauri application");
}
