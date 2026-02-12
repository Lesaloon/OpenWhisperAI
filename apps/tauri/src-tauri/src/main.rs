mod ipc;
mod logging;
mod state;

use ipc::{
    ipc_get_logs, ipc_get_settings, ipc_get_state, ipc_send_event, ipc_set_settings,
    ipc_update_settings,
};
use logging::{attach_app_handle, init_logging};

fn main() {
    init_logging();

    let context = tauri::generate_context!();
    tauri::Builder::default()
        .setup(|app| {
            let settings_path = state::default_settings_path(app.path_resolver().app_config_dir());
            app.manage(state::AppState::new(settings_path));
            attach_app_handle(app.handle());
            log::info!("tauri backend initialized");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ipc_get_state,
            ipc_send_event,
            ipc_get_settings,
            ipc_update_settings,
            ipc_set_settings,
            ipc_get_logs
        ])
        .run(context)
        .expect("error while running tauri application");
}
