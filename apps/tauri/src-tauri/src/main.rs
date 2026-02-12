mod ipc;
mod logging;
mod state;

use ipc::{ipc_get_logs, ipc_get_state, ipc_send_event};
use logging::{attach_app_handle, init_logging};

fn main() {
    init_logging();

    tauri::Builder::default()
        .manage(state::AppState::new())
        .setup(|app| {
            attach_app_handle(app.handle());
            log::info!("tauri backend initialized");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            ipc_get_state,
            ipc_send_event,
            ipc_get_logs
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
