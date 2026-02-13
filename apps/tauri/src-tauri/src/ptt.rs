use crate::logging::emit_app_event;
use core_input::{
    AudioBackend, CpalAudioBackend, GlobalHotkeyListener, Hotkey, HotkeyActionEvent, HotkeyKey,
    HotkeyListenerHandle, HotkeyManager, HotkeyModifiers, HotkeyState, HotkeyTrigger, LevelReading,
    PttCaptureService,
};
use log::{info, warn};
use serde::{Deserialize, Serialize};
use shared_types::{
    AppSettings, ModelInstallStatus, ModelStatusItem, ModelStatusPayload, OutputMode, PttLevel,
    PttState,
};
use std::{
    collections::HashMap,
    io::ErrorKind,
    path::{Path, PathBuf},
    process::Command,
    sync::{mpsc, Arc, Mutex},
    time::Duration,
};
use transcribe_engine::{
    BindingError, ModelError, ModelId, ModelManager, ModelSpec, WhisperBindings, WhisperCppBindings,
};

pub const PTT_STATE_EVENT: &str = "ptt_state";
pub const PTT_LEVEL_EVENT: &str = "ptt_level";
pub const PTT_TRANSCRIPTION_EVENT: &str = "ptt_transcription";
pub const PTT_ERROR_EVENT: &str = "ptt_error";
const MODEL_STATUS_EVENT: &str = "model-download-status";
const TARGET_SAMPLE_RATE: u32 = 16_000;

#[derive(Clone)]
pub struct PttHandle {
    sender: mpsc::Sender<PttRuntimeCommand>,
    state: Arc<Mutex<PttState>>,
}

enum PttRuntimeCommand {
    Start {
        settings: AppSettings,
        active_model: Option<String>,
        respond: mpsc::Sender<Result<PttState, String>>,
    },
    Stop {
        respond: mpsc::Sender<Result<PttState, String>>,
    },
    SetHotkey {
        payload: PttHotkeyPayload,
        respond: mpsc::Sender<Result<PttHotkeyPayload, String>>,
    },
    UpdateSettings {
        settings: AppSettings,
    },
    SetActiveModel {
        active_model: Option<String>,
    },
    ManualToggle {
        respond: mpsc::Sender<Result<PttState, String>>,
    },
}

impl PttHandle {
    pub fn new(model_root: PathBuf, models: Arc<Mutex<crate::state::ModelStore>>) -> Self {
        let (sender, receiver) = mpsc::channel();
        let state = Arc::new(Mutex::new(PttState::Idle));
        let state_handle = Arc::clone(&state);
        let models_handle = Arc::clone(&models);

        std::thread::spawn(move || {
            let mut controller = SystemPttController::new(model_root, Arc::clone(&models_handle));
            controller.attach_state_store(Arc::clone(&state_handle));

            loop {
                match receiver.recv_timeout(Duration::from_millis(25)) {
                    Ok(command) => match command {
                        PttRuntimeCommand::Start {
                            settings,
                            active_model,
                            respond,
                        } => {
                            let result = controller.start(settings, active_model);
                            let _ = respond.send(result);
                        }
                        PttRuntimeCommand::Stop { respond } => {
                            let result = controller.stop();
                            let _ = respond.send(result);
                        }
                        PttRuntimeCommand::SetHotkey { payload, respond } => {
                            let result = controller.set_hotkey(payload);
                            let _ = respond.send(result);
                        }
                        PttRuntimeCommand::UpdateSettings { settings } => {
                            controller.update_settings(settings);
                        }
                        PttRuntimeCommand::SetActiveModel { active_model } => {
                            controller.set_active_model(active_model);
                        }
                        PttRuntimeCommand::ManualToggle { respond } => {
                            let result = controller.manual_toggle_recording();
                            let _ = respond.send(result);
                        }
                    },
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }

                controller.poll_hotkey_events();
                controller.poll_level_readings();
            }
        });

        Self { sender, state }
    }

    pub fn start(
        &self,
        settings: AppSettings,
        active_model: Option<String>,
    ) -> Result<PttState, String> {
        let (respond, receiver) = mpsc::channel();
        self.sender
            .send(PttRuntimeCommand::Start {
                settings,
                active_model,
                respond,
            })
            .map_err(|err| err.to_string())?;
        receiver.recv().map_err(|err| err.to_string())?
    }

    pub fn stop(&self) -> Result<PttState, String> {
        let (respond, receiver) = mpsc::channel();
        self.sender
            .send(PttRuntimeCommand::Stop { respond })
            .map_err(|err| err.to_string())?;
        receiver.recv().map_err(|err| err.to_string())?
    }

    pub fn set_hotkey(&self, payload: PttHotkeyPayload) -> Result<PttHotkeyPayload, String> {
        let (respond, receiver) = mpsc::channel();
        self.sender
            .send(PttRuntimeCommand::SetHotkey { payload, respond })
            .map_err(|err| err.to_string())?;
        receiver.recv().map_err(|err| err.to_string())?
    }

    pub fn update_settings(&self, settings: AppSettings) {
        let _ = self
            .sender
            .send(PttRuntimeCommand::UpdateSettings { settings });
    }

    pub fn set_active_model(&self, active_model: Option<String>) {
        let _ = self
            .sender
            .send(PttRuntimeCommand::SetActiveModel { active_model });
    }

    pub fn manual_toggle(&self) -> Result<PttState, String> {
        let (respond, receiver) = mpsc::channel();
        self.sender
            .send(PttRuntimeCommand::ManualToggle { respond })
            .map_err(|err| err.to_string())?;
        receiver.recv().map_err(|err| err.to_string())?
    }

    pub fn state(&self) -> PttState {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PttHotkeyPayload {
    pub key: String,
    pub modifiers: PttHotkeyModifiers,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PttHotkeyModifiers {
    #[serde(default)]
    pub ctrl: bool,
    #[serde(default)]
    pub alt: bool,
    #[serde(default)]
    pub shift: bool,
    #[serde(default)]
    pub meta: bool,
}

impl Default for PttHotkeyPayload {
    fn default() -> Self {
        Self {
            key: "space".to_string(),
            modifiers: PttHotkeyModifiers {
                ctrl: true,
                alt: true,
                shift: false,
                meta: false,
            },
        }
    }
}

impl PttHotkeyPayload {
    pub fn to_hotkey(&self) -> Result<Hotkey, String> {
        let key = parse_hotkey_key(&self.key)
            .ok_or_else(|| format!("unsupported hotkey key '{}'", self.key))?;
        Ok(Hotkey {
            key,
            modifiers: HotkeyModifiers {
                ctrl: self.modifiers.ctrl,
                alt: self.modifiers.alt,
                shift: self.modifiers.shift,
                meta: self.modifiers.meta,
            },
        })
    }
}

pub trait TextInjector: Send + Sync {
    fn inject(&self, text: &str) -> Result<(), String>;
}

pub struct ClipboardInjector;

impl TextInjector for ClipboardInjector {
    fn inject(&self, text: &str) -> Result<(), String> {
        let mut clipboard = arboard::Clipboard::new().map_err(|err| err.to_string())?;
        clipboard
            .set_text(text.to_string())
            .map_err(|err| err.to_string())?;

        paste_from_clipboard()
    }
}

pub struct ClipboardOnlyInjector;

impl TextInjector for ClipboardOnlyInjector {
    fn inject(&self, text: &str) -> Result<(), String> {
        let mut clipboard = arboard::Clipboard::new().map_err(|err| err.to_string())?;
        clipboard
            .set_text(text.to_string())
            .map_err(|err| err.to_string())?;
        Ok(())
    }
}

pub struct DirectWriteInjector;

impl TextInjector for DirectWriteInjector {
    fn inject(&self, text: &str) -> Result<(), String> {
        type_text(text)
    }
}

enum PasteCommandError {
    NotFound,
    Failed(String),
}

fn paste_from_clipboard() -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
        let mut missing = Vec::new();

        for (cmd, args) in paste_command_candidates(wayland) {
            match run_paste_command(cmd, args) {
                Ok(()) => return Ok(()),
                Err(PasteCommandError::NotFound) => missing.push(cmd),
                Err(PasteCommandError::Failed(message)) => return Err(message),
            }
        }

        let helper_hint = if wayland {
            "wtype (Wayland) or xdotool (X11)"
        } else {
            "xdotool (X11) or wtype (Wayland)"
        };
        Err(format!(
            "missing paste helper: install {} to enable text injection",
            helper_hint
        ))
    }

    #[cfg(not(target_os = "linux"))]
    {
        Err("text injection is only supported on Linux via xdotool or wtype".to_string())
    }
}

fn type_text(text: &str) -> Result<(), String> {
    #[cfg(target_os = "linux")]
    {
        let wayland = std::env::var_os("WAYLAND_DISPLAY").is_some();
        let mut missing = Vec::new();
        for (cmd, args) in type_command_candidates(wayland, text) {
            log::info!("direct write using {cmd}");
            match run_type_command(cmd, args) {
                Ok(()) => return Ok(()),
                Err(PasteCommandError::NotFound) => missing.push(cmd),
                Err(PasteCommandError::Failed(message)) => return Err(message),
            }
        }
        let helper_hint = if wayland {
            "wtype (Wayland) or xdotool (X11)"
        } else {
            "xdotool (X11) or wtype (Wayland)"
        };
        Err(format!(
            "missing typing helper: install {} to enable direct write",
            helper_hint
        ))
    }

    #[cfg(not(target_os = "linux"))]
    {
        Err("direct write is only supported on Linux via xdotool or wtype".to_string())
    }
}

#[cfg(target_os = "linux")]
fn type_command_candidates(wayland: bool, text: &str) -> Vec<(&'static str, Vec<String>)> {
    if wayland {
        vec![
            ("wtype", vec!["--".to_string(), text.to_string()]),
            (
                "xdotool",
                vec![
                    "type".to_string(),
                    "--clearmodifiers".to_string(),
                    text.to_string(),
                ],
            ),
        ]
    } else {
        vec![
            (
                "xdotool",
                vec![
                    "type".to_string(),
                    "--clearmodifiers".to_string(),
                    text.to_string(),
                ],
            ),
            ("wtype", vec!["--".to_string(), text.to_string()]),
        ]
    }
}

#[cfg(target_os = "linux")]
fn run_type_command(cmd: &str, args: Vec<String>) -> Result<(), PasteCommandError> {
    let output = Command::new(cmd).args(&args).output().map_err(|err| {
        if err.kind() == ErrorKind::NotFound {
            return PasteCommandError::NotFound;
        }
        PasteCommandError::Failed(err.to_string())
    })?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    Err(PasteCommandError::Failed(format!(
        "command `{}` exited with {}: {}",
        cmd,
        output.status,
        stderr.trim()
    )))
}

#[cfg(target_os = "linux")]
fn paste_command_candidates(wayland: bool) -> Vec<(&'static str, &'static [&'static str])> {
    if wayland {
        vec![
            ("wtype", &["-M", "ctrl", "-k", "v", "-m", "ctrl"]),
            ("xdotool", &["key", "--clearmodifiers", "ctrl+v"]),
        ]
    } else {
        vec![
            ("xdotool", &["key", "--clearmodifiers", "ctrl+v"]),
            ("wtype", &["-M", "ctrl", "-k", "v", "-m", "ctrl"]),
        ]
    }
}

fn run_paste_command(cmd: &str, args: &[&str]) -> Result<(), PasteCommandError> {
    let output = Command::new(cmd).args(args).output().map_err(|err| {
        if err.kind() == ErrorKind::NotFound {
            PasteCommandError::NotFound
        } else {
            PasteCommandError::Failed(format!("failed to run `{}`: {}", cmd, err))
        }
    })?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let message = stderr.trim();
    let details = if message.is_empty() {
        format!("command `{}` exited with {}", cmd, output.status)
    } else {
        format!("command `{}` failed: {}", cmd, message)
    };
    Err(PasteCommandError::Failed(details))
}

pub trait Transcriber: Send + Sync {
    fn transcribe(&self, audio: &[f32]) -> Result<String, String>;
}

pub struct LocalTranscriber {
    manager: ModelManager,
    model_id: ModelId,
}

impl LocalTranscriber {
    pub fn new(model_root: PathBuf, model_id: ModelId) -> Self {
        let mut manager = ModelManager::new(model_root.clone());
        register_standard_models(&mut manager);
        if let ModelId::Custom(name) = &model_id {
            register_custom_model(&mut manager, &model_root, name);
        }
        Self { manager, model_id }
    }
}

impl Transcriber for LocalTranscriber {
    fn transcribe(&self, audio: &[f32]) -> Result<String, String> {
        let model_path = self
            .manager
            .ensure_model_available(&self.model_id)
            .map_err(|err| match err {
                ModelError::MissingFile(_) => {
                    format!("model not downloaded: {}", self.model_id.display_name())
                }
                other => other.to_string(),
            })?;
        info!(
            "transcribe using model '{}' at {}",
            self.model_id.display_name(),
            model_path.display()
        );
        let context = WhisperCppBindings::init_from_file(&model_path).map_err(|err| match err {
            BindingError::Unavailable => {
                "whisper.cpp CLI not found; set WHISPER_CPP_BIN".to_string()
            }
            other => other.to_string(),
        })?;
        WhisperCppBindings::transcribe(&context, audio).map_err(|err| {
            let message = match err {
                BindingError::Unavailable => {
                    "whisper.cpp CLI not found; set WHISPER_CPP_BIN".to_string()
                }
                other => other.to_string(),
            };
            warn!("whisper transcribe failed: {message}");
            message
        })
    }
}

pub struct PttController<B: AudioBackend> {
    state: PttState,
    armed: bool,
    hotkey: Hotkey,
    hotkey_manager: Arc<Mutex<HotkeyManager>>,
    hotkey_listener: Option<HotkeyListenerHandle>,
    hotkey_receiver: Option<mpsc::Receiver<HotkeyActionEvent>>,
    allow_global_hotkeys: bool,
    runtime_started: bool,
    level_receiver: Option<mpsc::Receiver<LevelReading>>,
    capture: PttCaptureService<B>,
    transcriber: Arc<dyn Transcriber>,
    injector: Arc<dyn TextInjector>,
    settings: AppSettings,
    model_root: PathBuf,
    active_model: Option<String>,
    state_store: Option<Arc<Mutex<PttState>>>,
    models: Arc<Mutex<crate::state::ModelStore>>,
}

pub type SystemPttController = PttController<CpalAudioBackend>;

impl SystemPttController {
    pub fn new(model_root: PathBuf, models: Arc<Mutex<crate::state::ModelStore>>) -> Self {
        Self::with_backend(CpalAudioBackend::default(), model_root, models)
    }
}

impl<B: AudioBackend> PttController<B> {
    pub fn with_backend(
        backend: B,
        model_root: PathBuf,
        models: Arc<Mutex<crate::state::ModelStore>>,
    ) -> Self {
        let disable_hotkeys = std::env::var("OPENWHISPERAI_DISABLE_GLOBAL_HOTKEYS")
            .map(|value| value == "1" || value.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let wayland = matches!(std::env::var("XDG_SESSION_TYPE").as_deref(), Ok("wayland"));
        let allow_global_hotkeys = !disable_hotkeys && !wayland;
        if wayland {
            warn!("Wayland session detected: global hotkeys are disabled (use Hyprland binding)");
        }
        let hotkey = PttHotkeyPayload::default().to_hotkey().unwrap_or(Hotkey {
            key: HotkeyKey::F9,
            modifiers: HotkeyModifiers::none(),
        });
        let mut manager = HotkeyManager::new();
        register_hotkey_binding(&mut manager, hotkey);
        let transcriber = Arc::new(LocalTranscriber::new(model_root.clone(), ModelId::Base));

        Self {
            state: PttState::Idle,
            armed: false,
            hotkey,
            hotkey_manager: Arc::new(Mutex::new(manager)),
            hotkey_listener: None,
            hotkey_receiver: None,
            allow_global_hotkeys,
            runtime_started: false,
            level_receiver: None,
            capture: PttCaptureService::new(backend, "ptt"),
            transcriber,
            injector: Arc::new(ClipboardInjector),
            settings: AppSettings::default(),
            model_root,
            active_model: None,
            state_store: None,
            models,
        }
    }

    pub fn attach_state_store(&mut self, store: Arc<Mutex<PttState>>) {
        self.state_store = Some(store);
        if let Some(store) = &self.state_store {
            let mut guard = store
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *guard = self.state.clone();
        }
    }

    pub fn set_hotkey(&mut self, payload: PttHotkeyPayload) -> Result<PttHotkeyPayload, String> {
        let hotkey = payload.to_hotkey()?;
        if let Ok(mut manager) = self.hotkey_manager.lock() {
            manager.unregister(&self.hotkey);
            register_hotkey_binding(&mut manager, hotkey);
        }
        self.hotkey = hotkey;
        Ok(payload)
    }

    pub fn set_active_model(&mut self, model_name: Option<String>) {
        if let Some(active) = self.active_model.as_deref() {
            if let Ok(mut models) = self.models.lock() {
                models.clear_override(active);
            }
        }
        let model_id = model_id_from_name(model_name.as_deref());
        let display_name = model_id.display_name();
        self.transcriber = Arc::new(LocalTranscriber::new(self.model_root.clone(), model_id));
        self.active_model = model_name.or_else(|| Some(display_name));
        if let Some(active) = self.active_model.as_deref() {
            if let Ok(mut models) = self.models.lock() {
                models.clear_override(active);
            }
        }
        self.update_model_status_snapshot();
    }

    pub fn update_settings(&mut self, settings: AppSettings) {
        self.settings = settings;
    }

    pub fn start(
        &mut self,
        settings: AppSettings,
        active_model: Option<String>,
    ) -> Result<PttState, String> {
        self.ensure_runtime()?;
        self.arm(settings, active_model)
    }

    pub fn stop(&mut self) -> Result<PttState, String> {
        self.armed = false;
        if self.capture.audio().is_running() {
            let _ = self.capture.stop();
        }
        self.set_state(PttState::Idle);
        Ok(self.state.clone())
    }

    fn arm(
        &mut self,
        settings: AppSettings,
        active_model: Option<String>,
    ) -> Result<PttState, String> {
        self.settings = settings.clone();
        self.set_active_model(active_model);
        self.prepare_audio(&settings)?;
        self.armed = true;
        self.update_model_status_snapshot();
        self.set_state(PttState::Armed);
        Ok(self.state.clone())
    }

    fn prepare_audio(&mut self, settings: &AppSettings) -> Result<(), String> {
        let audio = self.capture.audio_mut();
        audio.refresh_devices().map_err(|err| err.to_string())?;
        if settings.input_device != "default" {
            let _ = audio.select_device(&settings.input_device);
        }
        if !audio.is_running() {
            self.capture.start().map_err(|err| err.to_string())?;
        }
        Ok(())
    }

    fn ensure_runtime(&mut self) -> Result<(), String> {
        if self.runtime_started {
            return Ok(());
        }

        if self.allow_global_hotkeys && self.hotkey_receiver.is_none() {
            let listener = GlobalHotkeyListener::new(Arc::clone(&self.hotkey_manager));
            let (handle_listener, receiver) = listener.start().map_err(|err| err.to_string())?;
            self.hotkey_listener = Some(handle_listener);
            self.hotkey_receiver = Some(receiver);
        }

        if self.level_receiver.is_none() {
            self.level_receiver = self.capture.level_feed();
        }

        self.runtime_started = true;
        Ok(())
    }

    fn poll_hotkey_events(&mut self) {
        let Some(receiver) = self.hotkey_receiver.take() else {
            return;
        };
        loop {
            match receiver.try_recv() {
                Ok(event) => {
                    let work = match self.handle_hotkey_action(&event) {
                        Ok(value) => value,
                        Err(err) => {
                            self.emit_error(&err);
                            None
                        }
                    };

                    if let Some(work) = work {
                        let transcription = work.transcriber.transcribe(&work.audio);
                        if let Ok(text) = &transcription {
                            if let Err(err) = self.handle_output(&work.output_mode, text) {
                                self.emit_output_warning(&err);
                            }
                        }
                        self.complete_transcription(transcription);
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.hotkey_receiver = None;
                    return;
                }
            }
        }

        self.hotkey_receiver = Some(receiver);
    }

    fn poll_level_readings(&mut self) {
        let Some(receiver) = self.level_receiver.take() else {
            return;
        };
        loop {
            match receiver.try_recv() {
                Ok(reading) => {
                    if self.armed {
                        emit_level(reading);
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.level_receiver = None;
                    return;
                }
            }
        }

        self.level_receiver = Some(receiver);
    }

    fn handle_hotkey_action(
        &mut self,
        event: &HotkeyActionEvent,
    ) -> Result<Option<TranscriptionWork>, String> {
        if !self.armed {
            return Ok(None);
        }
        if event.action != "ptt" {
            return Ok(None);
        }
        let mut effective_state = event.state;
        if matches!(event.state, HotkeyState::Pressed) && self.state == PttState::Capturing {
            warn!("ptt release not detected; treating press as release");
            effective_state = HotkeyState::Released;
        }
        info!("ptt hotkey {:?}", effective_state);
        let effective_event = HotkeyActionEvent {
            action: event.action.clone(),
            hotkey: event.hotkey.clone(),
            state: effective_state,
        };
        self.capture
            .handle_hotkey_action(&effective_event)
            .map_err(|err| err.to_string())?;

        match effective_state {
            HotkeyState::Pressed => {
                self.set_state(PttState::Capturing);
                Ok(None)
            }
            HotkeyState::Released => {
                self.set_state(PttState::Processing);
                self.mark_model_downloading();
                let audio = self.capture.take_audio().map_err(|err| err.to_string())?;
                let (sample_rate, channels) = self
                    .capture
                    .audio()
                    .selected_device()
                    .map(|device| (device.sample_rate, device.channels))
                    .unwrap_or((TARGET_SAMPLE_RATE, 1));
                let audio = resample_to_16k_mono(audio, sample_rate, channels);
                Ok(Some(TranscriptionWork {
                    audio,
                    transcriber: Arc::clone(&self.transcriber),
                    injector: Arc::clone(&self.injector),
                    output_mode: self.settings.output_mode.clone(),
                }))
            }
        }
    }

    fn manual_toggle_recording(&mut self) -> Result<PttState, String> {
        log::info!("manual toggle requested (state={:?})", self.state);
        if self.state == PttState::Processing {
            return Ok(self.state.clone());
        }
        if !self.armed {
            let settings = self.settings.clone();
            let active_model = self.active_model.clone();
            self.arm(settings, active_model)?;
        }

        let next_state = if self.state == PttState::Capturing {
            HotkeyState::Released
        } else {
            HotkeyState::Pressed
        };
        let event = HotkeyActionEvent {
            action: "ptt".to_string(),
            hotkey: self.hotkey.clone(),
            state: next_state,
        };
        let work = self.handle_hotkey_action(&event)?;
        if let Some(work) = work {
            let transcription = work.transcriber.transcribe(&work.audio);
            if let Ok(text) = &transcription {
                if let Err(err) = self.handle_output(&work.output_mode, text) {
                    self.emit_output_warning(&err);
                }
            }
            self.complete_transcription(transcription);
        }

        log::info!("manual toggle finished (state={:?})", self.state);
        Ok(self.state.clone())
    }

    fn complete_transcription(&mut self, result: Result<String, String>) {
        match result {
            Ok(text) => {
                if text.trim().is_empty() {
                    emit_app_event(PTT_ERROR_EVENT, &"no speech detected".to_string());
                    info!("transcription empty");
                    self.mark_model_ready();
                    self.set_state(if self.armed {
                        PttState::Armed
                    } else {
                        PttState::Idle
                    });
                    return;
                }
                if let Ok(mut models) = self.models.lock() {
                    models.set_last_transcript(text.clone());
                }
                emit_app_event(PTT_TRANSCRIPTION_EVENT, &text);
                info!("transcription complete ({} chars)", text.len());
                self.mark_model_ready();
                self.set_state(if self.armed {
                    PttState::Armed
                } else {
                    PttState::Idle
                });
            }
            Err(err) => {
                self.emit_error(&err);
                warn!("transcription failed: {err}");
                self.mark_model_failed();
                self.set_state(if self.armed {
                    PttState::Armed
                } else {
                    PttState::Idle
                });
            }
        }
    }

    fn emit_error(&mut self, message: &str) {
        self.set_state(PttState::Error {
            message: message.to_string(),
        });
        emit_app_event(PTT_ERROR_EVENT, &message.to_string());
    }

    fn set_state(&mut self, next: PttState) {
        if self.state == next {
            return;
        }
        self.state = next.clone();
        if let Some(store) = &self.state_store {
            let mut guard = store
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            *guard = next.clone();
        }
        emit_app_event(PTT_STATE_EVENT, &next);
    }

    fn update_model_status_snapshot(&self) {
        let overrides = self
            .models
            .lock()
            .map(|models| models.overrides_snapshot())
            .unwrap_or_default();
        let payload =
            build_model_status_payload(&self.model_root, self.active_model.as_deref(), &overrides);
        if let Ok(mut models) = self.models.lock() {
            let _ = models.set_models(payload.models.clone());
            let _ = models.set_active_model(payload.active_model.clone());
        }
        emit_app_event(MODEL_STATUS_EVENT, &payload);
    }

    fn mark_model_downloading(&mut self) {
        self.update_active_override(Some(ModelInstallStatus::Downloading));
        self.update_model_status_snapshot();
    }

    fn mark_model_ready(&mut self) {
        self.update_active_override(None);
        self.update_model_status_snapshot();
    }

    fn mark_model_failed(&mut self) {
        self.update_active_override(Some(ModelInstallStatus::Failed));
        self.update_model_status_snapshot();
    }

    fn update_active_override(&mut self, status: Option<ModelInstallStatus>) {
        let Some(active) = self.active_model.clone() else {
            return;
        };
        if let Ok(mut models) = self.models.lock() {
            match status {
                Some(status) => models.set_override(active, status),
                None => models.clear_override(&active),
            }
        }
    }

    fn handle_output(&self, mode: &OutputMode, text: &str) -> Result<(), String> {
        if text.is_empty() {
            return Ok(());
        }
        match mode {
            OutputMode::UiOnly => Ok(()),
            OutputMode::Clipboard => ClipboardOnlyInjector.inject(text),
            OutputMode::DirectWrite => match DirectWriteInjector.inject(text) {
                Ok(()) => {
                    log::info!("direct write succeeded");
                    Ok(())
                }
                Err(err) => {
                    let _ = ClipboardOnlyInjector.inject(text);
                    Err(format!("direct write failed; copied to clipboard: {err}"))
                }
            },
        }
    }

    fn emit_output_warning(&self, message: &str) {
        warn!("output warning: {message}");
        emit_app_event(PTT_ERROR_EVENT, &message.to_string());
    }
}

struct TranscriptionWork {
    audio: Vec<f32>,
    transcriber: Arc<dyn Transcriber>,
    injector: Arc<dyn TextInjector>,
    output_mode: OutputMode,
}

fn resample_to_16k_mono(audio: Vec<f32>, sample_rate: u32, channels: u16) -> Vec<f32> {
    let mono = if channels <= 1 {
        audio
    } else {
        downmix_to_mono(audio, channels)
    };

    if mono.is_empty() || sample_rate == TARGET_SAMPLE_RATE || sample_rate == 0 {
        return mono;
    }

    let target_len =
        ((mono.len() as f64) * TARGET_SAMPLE_RATE as f64 / sample_rate as f64).round() as usize;
    if target_len == 0 {
        return Vec::new();
    }

    let step = sample_rate as f64 / TARGET_SAMPLE_RATE as f64;
    let mut output = Vec::with_capacity(target_len);
    for i in 0..target_len {
        let src_pos = i as f64 * step;
        let idx = src_pos.floor() as usize;
        if idx >= mono.len() {
            break;
        }
        let frac = (src_pos - idx as f64) as f32;
        let next = if idx + 1 < mono.len() { idx + 1 } else { idx };
        let sample = mono[idx] + (mono[next] - mono[idx]) * frac;
        output.push(sample);
    }

    output
}

fn downmix_to_mono(audio: Vec<f32>, channels: u16) -> Vec<f32> {
    let channels = channels as usize;
    if channels == 0 {
        return Vec::new();
    }
    let frames = audio.len() / channels;
    let mut mono = Vec::with_capacity(frames);
    for frame in 0..frames {
        let offset = frame * channels;
        let mut sum = 0.0;
        for channel in 0..channels {
            sum += audio[offset + channel];
        }
        mono.push(sum / channels as f32);
    }
    mono
}

fn register_hotkey_binding(manager: &mut HotkeyManager, hotkey: Hotkey) {
    manager.register_with_trigger(hotkey, HotkeyTrigger::Pressed, "ptt");
    manager.register_with_trigger(hotkey, HotkeyTrigger::Released, "ptt");
}

fn parse_hotkey_key(name: &str) -> Option<HotkeyKey> {
    let key = name.trim().to_ascii_lowercase();
    if key.len() == 1 {
        let ch = key.chars().next()?;
        return match ch {
            'a' => Some(HotkeyKey::A),
            'b' => Some(HotkeyKey::B),
            'c' => Some(HotkeyKey::C),
            'd' => Some(HotkeyKey::D),
            'e' => Some(HotkeyKey::E),
            'f' => Some(HotkeyKey::F),
            'g' => Some(HotkeyKey::G),
            'h' => Some(HotkeyKey::H),
            'i' => Some(HotkeyKey::I),
            'j' => Some(HotkeyKey::J),
            'k' => Some(HotkeyKey::K),
            'l' => Some(HotkeyKey::L),
            'm' => Some(HotkeyKey::M),
            'n' => Some(HotkeyKey::N),
            'o' => Some(HotkeyKey::O),
            'p' => Some(HotkeyKey::P),
            'q' => Some(HotkeyKey::Q),
            'r' => Some(HotkeyKey::R),
            's' => Some(HotkeyKey::S),
            't' => Some(HotkeyKey::T),
            'u' => Some(HotkeyKey::U),
            'v' => Some(HotkeyKey::V),
            'w' => Some(HotkeyKey::W),
            'x' => Some(HotkeyKey::X),
            'y' => Some(HotkeyKey::Y),
            'z' => Some(HotkeyKey::Z),
            _ => None,
        };
    }

    match key.as_str() {
        "f1" => Some(HotkeyKey::F1),
        "f2" => Some(HotkeyKey::F2),
        "f3" => Some(HotkeyKey::F3),
        "f4" => Some(HotkeyKey::F4),
        "f5" => Some(HotkeyKey::F5),
        "f6" => Some(HotkeyKey::F6),
        "f7" => Some(HotkeyKey::F7),
        "f8" => Some(HotkeyKey::F8),
        "f9" => Some(HotkeyKey::F9),
        "f10" => Some(HotkeyKey::F10),
        "f11" => Some(HotkeyKey::F11),
        "f12" => Some(HotkeyKey::F12),
        "space" => Some(HotkeyKey::Space),
        "enter" => Some(HotkeyKey::Enter),
        "escape" => Some(HotkeyKey::Escape),
        "tab" => Some(HotkeyKey::Tab),
        "backspace" => Some(HotkeyKey::Backspace),
        "left" => Some(HotkeyKey::Left),
        "right" => Some(HotkeyKey::Right),
        "up" => Some(HotkeyKey::Up),
        "down" => Some(HotkeyKey::Down),
        _ => None,
    }
}

pub(crate) fn model_id_from_name(name: Option<&str>) -> ModelId {
    let Some(name) = name else {
        return ModelId::Base;
    };
    match name.trim().to_ascii_lowercase().as_str() {
        "tiny" => ModelId::Tiny,
        "base" => ModelId::Base,
        "small" => ModelId::Small,
        "medium" => ModelId::Medium,
        "large" => ModelId::Large,
        other => ModelId::Custom(other.to_string()),
    }
}

pub(crate) fn build_model_status_payload(
    root: &Path,
    active: Option<&str>,
    overrides: &HashMap<String, ModelInstallStatus>,
) -> ModelStatusPayload {
    let mut items = Vec::new();
    let standard = [
        ModelId::Tiny,
        ModelId::Base,
        ModelId::Small,
        ModelId::Medium,
        ModelId::Large,
    ];

    for model_id in standard {
        let id = model_id.display_name();
        let filename = format!("ggml-{}.bin", id);
        let path = root.join(&filename);
        let mut status = if path.exists() {
            ModelInstallStatus::Ready
        } else {
            ModelInstallStatus::Pending
        };
        let is_active = active.map_or(false, |name| name == id);
        if let Some(override_status) = overrides.get(&id) {
            status = override_status.clone();
        }
        let progress = if status == ModelInstallStatus::Ready {
            100.0
        } else {
            0.0
        };
        items.push(ModelStatusItem {
            id: id.clone(),
            name: id,
            status,
            total_bytes: 0,
            downloaded_bytes: 0,
            speed_bytes_per_sec: 0,
            eta_seconds: 0,
            progress,
            active: is_active,
        });
    }

    if let Some(active_name) = active {
        if !items.iter().any(|item| item.id == active_name) {
            let filename = format!("{active_name}.bin");
            let path = root.join(&filename);
            let mut status = if path.exists() {
                ModelInstallStatus::Ready
            } else {
                ModelInstallStatus::Pending
            };
            if let Some(override_status) = overrides.get(active_name) {
                status = override_status.clone();
            }
            let progress = if status == ModelInstallStatus::Ready {
                100.0
            } else {
                0.0
            };
            items.push(ModelStatusItem {
                id: active_name.to_string(),
                name: active_name.to_string(),
                status,
                total_bytes: 0,
                downloaded_bytes: 0,
                speed_bytes_per_sec: 0,
                eta_seconds: 0,
                progress,
                active: true,
            });
        }
    }

    let queue_count = items
        .iter()
        .filter(|model| {
            matches!(
                model.status,
                ModelInstallStatus::Downloading
                    | ModelInstallStatus::Queued
                    | ModelInstallStatus::Pending
            )
        })
        .count();

    ModelStatusPayload {
        models: items,
        active_model: active.map(|name| name.to_string()),
        queue_count,
    }
}

pub(crate) fn register_standard_models(manager: &mut ModelManager) {
    let standard = [
        ModelId::Tiny,
        ModelId::Base,
        ModelId::Small,
        ModelId::Medium,
        ModelId::Large,
    ];
    for model_id in standard {
        let filename = format!("ggml-{}.bin", model_id.display_name());
        let spec = ModelSpec::new(model_id, filename.clone())
            .with_download_url(model_download_url(&filename));
        manager.register_model(spec);
    }
}

pub(crate) fn register_custom_model(manager: &mut ModelManager, root: &Path, name: &str) {
    let filename = format!("{name}.bin");
    let spec = ModelSpec::new(ModelId::Custom(name.to_string()), filename.clone())
        .with_download_url(file_url(root.join(&filename)));
    manager.register_model(spec);
}

fn file_url(path: PathBuf) -> String {
    format!("file://{}", path.display())
}

fn model_download_url(filename: &str) -> String {
    format!("https://huggingface.co/ggerganov/whisper.cpp/resolve/main/{filename}")
}

fn emit_level(reading: LevelReading) {
    let level = PttLevel {
        rms: reading.rms,
        peak: reading.peak,
    };
    emit_app_event(PTT_LEVEL_EVENT, &level);
}

#[cfg(test)]
mod tests {
    use super::*;
    use core_input::{AudioDevice, AudioError, AudioStream};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    };
    use std::time::Duration;

    #[test]
    fn hotkey_payload_parses_letters() {
        let payload = PttHotkeyPayload {
            key: "k".to_string(),
            modifiers: PttHotkeyModifiers {
                ctrl: true,
                alt: false,
                shift: false,
                meta: false,
            },
        };
        let hotkey = payload.to_hotkey().expect("hotkey");
        assert_eq!(hotkey.key, HotkeyKey::K);
        assert!(hotkey.modifiers.ctrl);
    }

    #[test]
    fn hotkey_payload_rejects_unknown_key() {
        let payload = PttHotkeyPayload {
            key: "unknown".to_string(),
            modifiers: PttHotkeyModifiers {
                ctrl: false,
                alt: false,
                shift: false,
                meta: false,
            },
        };
        let result = payload.to_hotkey();
        assert!(result.is_err());
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn paste_candidates_prefer_wayland_helper() {
        let wayland_candidates = paste_command_candidates(true);
        let x11_candidates = paste_command_candidates(false);

        assert_eq!(
            wayland_candidates.first().map(|(cmd, _)| *cmd),
            Some("wtype")
        );
        assert_eq!(x11_candidates.first().map(|(cmd, _)| *cmd), Some("xdotool"));
    }

    #[derive(Clone)]
    struct MockStreamController {
        running: Arc<AtomicBool>,
        callback: Arc<Mutex<Option<Box<dyn FnMut(&[f32]) + Send>>>>,
    }

    impl MockStreamController {
        fn push_samples(&self, samples: &[f32]) {
            if !self.running.load(Ordering::SeqCst) {
                return;
            }

            if let Ok(mut callback) = self.callback.lock() {
                if let Some(handler) = callback.as_mut() {
                    handler(samples);
                }
            }
        }
    }

    struct MockStream {
        controller: MockStreamController,
    }

    impl AudioStream for MockStream {
        fn start(&self) -> Result<(), AudioError> {
            self.controller.running.store(true, Ordering::SeqCst);
            Ok(())
        }

        fn stop(&self) -> Result<(), AudioError> {
            self.controller.running.store(false, Ordering::SeqCst);
            Ok(())
        }
    }

    #[derive(Clone)]
    struct MockAudioBackend {
        devices: Vec<AudioDevice>,
        controller: Arc<Mutex<Option<MockStreamController>>>,
    }

    impl MockAudioBackend {
        fn new() -> Self {
            Self {
                devices: vec![AudioDevice {
                    id: "0:Mock".to_string(),
                    name: "Mock".to_string(),
                    sample_rate: 44_100,
                    channels: 2,
                }],
                controller: Arc::new(Mutex::new(None)),
            }
        }
    }

    impl AudioBackend for MockAudioBackend {
        type Stream = MockStream;

        fn list_input_devices(&self) -> Result<Vec<AudioDevice>, AudioError> {
            Ok(self.devices.clone())
        }

        fn default_input_device(&self) -> Result<Option<AudioDevice>, AudioError> {
            Ok(self.devices.first().cloned())
        }

        fn build_input_stream(
            &self,
            _device: &AudioDevice,
            on_samples: Box<dyn FnMut(&[f32]) + Send>,
        ) -> Result<Self::Stream, AudioError> {
            let controller = MockStreamController {
                running: Arc::new(AtomicBool::new(false)),
                callback: Arc::new(Mutex::new(Some(on_samples))),
            };

            *self.controller.lock().expect("lock") = Some(controller.clone());
            Ok(MockStream { controller })
        }
    }

    struct MockTranscriber;

    impl Transcriber for MockTranscriber {
        fn transcribe(&self, _audio: &[f32]) -> Result<String, String> {
            Ok("hello world".to_string())
        }
    }

    struct MockInjector {
        sender: mpsc::Sender<String>,
    }

    impl TextInjector for MockInjector {
        fn inject(&self, text: &str) -> Result<(), String> {
            let _ = self.sender.send(text.to_string());
            Ok(())
        }
    }

    #[test]
    fn ptt_transcribes_and_injects_on_release() {
        let backend = MockAudioBackend::new();
        let controller_handle = backend.controller.clone();
        let (inject_tx, inject_rx) = mpsc::channel();
        let models = Arc::new(Mutex::new(crate::state::ModelStore::new()));
        let mut controller = PttController::with_backend(backend, std::env::temp_dir(), models);

        controller
            .arm(AppSettings::default(), Some("base".to_string()))
            .expect("arm");
        controller.transcriber = Arc::new(MockTranscriber);
        controller.injector = Arc::new(MockInjector { sender: inject_tx });

        let event_pressed = HotkeyActionEvent {
            action: "ptt".to_string(),
            hotkey: Hotkey {
                key: HotkeyKey::F9,
                modifiers: HotkeyModifiers::none(),
            },
            state: HotkeyState::Pressed,
        };
        let event_released = HotkeyActionEvent {
            action: "ptt".to_string(),
            hotkey: Hotkey {
                key: HotkeyKey::F9,
                modifiers: HotkeyModifiers::none(),
            },
            state: HotkeyState::Released,
        };

        controller
            .handle_hotkey_action(&event_pressed)
            .expect("pressed");

        let stream_controller = controller_handle
            .lock()
            .expect("lock")
            .clone()
            .expect("controller ready");
        stream_controller.push_samples(&[0.1, 0.2, 0.3]);

        let work = controller
            .handle_hotkey_action(&event_released)
            .expect("released")
            .expect("work");
        let text = work
            .transcriber
            .transcribe(&work.audio)
            .expect("transcribe");
        controller.handle_output(&OutputMode::UiOnly, &text);

        let injected = inject_rx.recv_timeout(Duration::from_millis(50));
        assert!(injected.is_err());
    }

    #[test]
    fn resample_downmixes_stereo_to_mono() {
        let audio = vec![1.0, -1.0, 0.5, 0.5];
        let output = resample_to_16k_mono(audio, TARGET_SAMPLE_RATE, 2);
        assert_eq!(output, vec![0.0, 0.5]);
    }

    #[test]
    fn resample_linearly_interpolates() {
        let audio = vec![0.0, 1.0, 0.0, -1.0, 0.0];
        let output = resample_to_16k_mono(audio, 44_100, 1);
        assert_eq!(output.len(), 2);
        let expected = -0.75625_f32;
        assert!((output[0] - 0.0).abs() < 1e-6);
        assert!((output[1] - expected).abs() < 1e-4);
    }
}
