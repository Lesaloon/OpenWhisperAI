mod control_server;
mod ipc;
mod logging;
mod ptt;
mod state;
mod ui_server;
mod whisper_cli;

use ipc::{
    ipc_get_last_transcript, ipc_get_logs, ipc_get_models, ipc_get_settings, ipc_get_state,
    ipc_hello, ipc_model_download, ipc_model_select, ipc_ptt_get_state, ipc_ptt_set_hotkey,
    ipc_ptt_start, ipc_ptt_stop, ipc_ptt_toggle_recording, ipc_send_event, ipc_set_models,
    ipc_set_settings, ipc_update_settings, BACKEND_STATE_EVENT, MODEL_STATUS_EVENT,
};
use logging::{attach_app_handle, init_logging};
use ptt::PTT_STATE_EVENT;
use signal_hook::consts::signal::SIGUSR1;
use signal_hook::iterator::Signals;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};
use tauri::{
    CustomMenuItem, Icon, Manager, SystemTray, SystemTrayEvent, SystemTrayMenu, WindowEvent,
};

fn main() {
    init_logging();

    if std::env::var("OPENWHISPERAI_HEADLESS")
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        run_headless();
        return;
    }

    let context = tauri::generate_context!();
    let tray_menu = SystemTrayMenu::new()
        .add_item(CustomMenuItem::new("show", "Show"))
        .add_item(CustomMenuItem::new("quit", "Quit"));
    let tray_icon = Icon::File(std::path::PathBuf::from("icons/icon.png"));
    let tray = SystemTray::new().with_menu(tray_menu).with_icon(tray_icon);
    tauri::Builder::default()
        .system_tray(tray)
        .setup(|app| {
            ui_server::maybe_start();
            let settings_path = state::default_settings_path(app.path_resolver().app_config_dir());
            let model_root = app
                .path_resolver()
                .app_data_dir()
                .unwrap_or_else(|| std::env::temp_dir())
                .join("models");
            log::info!("model root: {}", model_root.display());
            if let Some(app_data_dir) = app.path_resolver().app_data_dir() {
                whisper_cli::ensure_whisper_cli(app_data_dir);
            } else {
                log::warn!("app data dir unavailable; whisper auto-install skipped");
            }
            app.manage(state::AppState::new(settings_path, model_root));
            attach_app_handle(app.handle());
            let app_state = app.state::<state::AppState>();
            spawn_signal_listener(app_state.ptt_handle());
            control_server::start(app_state.ptt_handle());
            if let Some(dir) = app.path_resolver().app_data_dir() {
                write_pid_file(&dir);
            }
            let auto_start = std::env::var("OPENWHISPERAI_PTT_AUTOSTART")
                .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
                .unwrap_or(false);
            if auto_start {
                let settings = app_state.lock_orchestrator().settings();
                let active_model = app_state.lock_models().snapshot().active_model;
                if let Err(err) = app_state.ptt_handle().start(settings, active_model) {
                    log::warn!("failed to auto-start ptt: {err}");
                } else {
                    log::info!("ptt auto-started");
                }
            } else {
                log::info!("ptt auto-start disabled");
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
        .on_system_tray_event(|app, event| match event {
            SystemTrayEvent::LeftClick { .. } => {
                if let Some(window) = app.get_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            SystemTrayEvent::MenuItemClick { id, .. } => match id.as_str() {
                "show" => {
                    if let Some(window) = app.get_window("main") {
                        let _ = window.show();
                        let _ = window.set_focus();
                    }
                }
                "quit" => {
                    app.exit(0);
                }
                _ => {}
            },
            _ => {}
        })
        .on_window_event(|event| {
            if let WindowEvent::CloseRequested { api, .. } = event.event() {
                let _ = event.window().hide();
                api.prevent_close();
            }
        })
        .invoke_handler(tauri::generate_handler![
            ipc_get_state,
            ipc_send_event,
            ipc_get_settings,
            ipc_update_settings,
            ipc_set_settings,
            ipc_get_logs,
            ipc_get_models,
            ipc_get_last_transcript,
            ipc_set_models,
            ipc_model_select,
            ipc_model_download,
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

fn run_headless() {
    let app_data_dir = resolve_app_data_dir();
    let config_dir = resolve_config_dir();
    let settings_path = state::default_settings_path(Some(config_dir));
    let model_root = app_data_dir.join("models");
    log::info!("headless model root: {}", model_root.display());

    whisper_cli::ensure_whisper_cli(app_data_dir.clone());
    write_pid_file(&app_data_dir);

    let app_state = state::AppState::new(settings_path, model_root);
    spawn_signal_listener(app_state.ptt_handle());
    control_server::start(app_state.ptt_handle());

    let auto_start = std::env::var("OPENWHISPERAI_PTT_AUTOSTART")
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false);
    if auto_start {
        let settings = app_state.lock_orchestrator().settings();
        let active_model = app_state.lock_models().snapshot().active_model;
        if let Err(err) = app_state.ptt_handle().start(settings, active_model) {
            log::warn!("failed to auto-start ptt: {err}");
        } else {
            log::info!("ptt auto-started");
        }
    } else {
        log::info!("ptt auto-start disabled");
    }

    log::info!("headless mode running");
    loop {
        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

fn resolve_app_data_dir() -> std::path::PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        return std::path::PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("com.openwhisperai.app");
    }
    std::env::temp_dir().join("openwhisperai")
}

fn resolve_config_dir() -> std::path::PathBuf {
    if let Ok(home) = std::env::var("HOME") {
        return std::path::PathBuf::from(home)
            .join(".config")
            .join("com.openwhisperai.app");
    }
    std::env::temp_dir().join("openwhisperai")
}

fn write_pid_file(app_data_dir: &std::path::Path) {
    if std::fs::create_dir_all(app_data_dir).is_err() {
        return;
    }
    let pid_path = app_data_dir.join("openwhisperai.pid");
    if std::fs::write(&pid_path, std::process::id().to_string()).is_ok() {
        log::info!("wrote pid file: {}", pid_path.display());
    }
}

fn spawn_signal_listener(handle: ptt::PttHandle) {
    if std::env::var("OPENWHISPERAI_DISABLE_SIGNAL_TOGGLE")
        .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
    {
        log::warn!("signal toggle disabled by env");
        return;
    }
    static SIG_TOGGLE_PENDING: AtomicBool = AtomicBool::new(false);
    static LAST_TOGGLE_MS: AtomicU64 = AtomicU64::new(0);
    let worker_handle = handle.clone();

    thread::spawn(move || loop {
        if SIG_TOGGLE_PENDING.swap(false, Ordering::Relaxed) {
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            let last = LAST_TOGGLE_MS.load(Ordering::Relaxed);
            if now.saturating_sub(last) < 400 {
                log::warn!("signal worker: debounce skip");
            } else {
                LAST_TOGGLE_MS.store(now, Ordering::Relaxed);
                match worker_handle.manual_toggle() {
                    Ok(state) => log::info!("signal worker: toggle ok -> {:?}", state),
                    Err(err) => log::warn!("signal worker: toggle failed: {err}"),
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    });

    thread::spawn(move || {
        let mut signals = match Signals::new([SIGUSR1]) {
            Ok(signals) => signals,
            Err(err) => {
                log::warn!("failed to register signal handler: {err}");
                return;
            }
        };
        for _ in signals.forever() {
            eprintln!("[signal] SIGUSR1 received");
            log::info!("signal: SIGUSR1 received");
            SIG_TOGGLE_PENDING.store(true, Ordering::Relaxed);
        }
    });
}
