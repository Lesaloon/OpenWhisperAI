use crate::logging::emit_app_event;
use core_input::{
    AudioBackend, AudioDevice, AudioError, AudioStream, GlobalHotkeyListener, Hotkey,
    HotkeyActionEvent, HotkeyKey, HotkeyListenerHandle, HotkeyManager, HotkeyModifiers,
    HotkeyState, HotkeyTrigger, LevelReading, PttCaptureService,
};
use serde::{Deserialize, Serialize};
use shared_types::{AppSettings, PttLevel, PttState};
use std::{
    io::ErrorKind,
    path::{Path, PathBuf},
    process::Command,
    sync::{mpsc, Arc, Mutex},
};
use transcribe_engine::{FsDownloader, ModelId, ModelManager, ModelSpec, TranscriptionPipeline};

pub const PTT_STATE_EVENT: &str = "ptt_state";
pub const PTT_LEVEL_EVENT: &str = "ptt_level";
pub const PTT_TRANSCRIPTION_EVENT: &str = "ptt_transcription";
pub const PTT_ERROR_EVENT: &str = "ptt_error";

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
            key: "f9".to_string(),
            modifiers: PttHotkeyModifiers {
                ctrl: false,
                alt: false,
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

pub struct PipelineTranscriber {
    pipeline: TranscriptionPipeline,
    model_id: ModelId,
}

impl PipelineTranscriber {
    pub fn new(model_root: PathBuf, model_id: ModelId) -> Self {
        let mut manager = ModelManager::new(model_root.clone());
        register_standard_models(&mut manager, &model_root);
        if let ModelId::Custom(name) = &model_id {
            register_custom_model(&mut manager, &model_root, name);
        }
        Self {
            pipeline: TranscriptionPipeline::new(manager, FsDownloader),
            model_id,
        }
    }
}

impl Transcriber for PipelineTranscriber {
    fn transcribe(&self, audio: &[f32]) -> Result<String, String> {
        self.pipeline
            .transcribe(self.model_id.clone(), audio)
            .map(|result| result.text)
            .map_err(|err| err.to_string())
    }
}

pub struct PttController<B: AudioBackend> {
    state: PttState,
    armed: bool,
    hotkey: Hotkey,
    hotkey_manager: Arc<Mutex<HotkeyManager>>,
    hotkey_listener: Option<HotkeyListenerHandle>,
    hotkey_receiver: Option<mpsc::Receiver<HotkeyActionEvent>>,
    runtime_started: bool,
    level_task_started: bool,
    capture: PttCaptureService<B>,
    transcriber: Arc<dyn Transcriber>,
    injector: Arc<dyn TextInjector>,
    settings: AppSettings,
    model_root: PathBuf,
}

pub type SystemPttController = PttController<CpalAudioBackend>;

impl SystemPttController {
    pub fn new(model_root: PathBuf) -> Self {
        Self::with_backend(CpalAudioBackend::default(), model_root)
    }
}

impl<B: AudioBackend> PttController<B> {
    pub fn with_backend(backend: B, model_root: PathBuf) -> Self {
        let hotkey = PttHotkeyPayload::default().to_hotkey().unwrap_or(Hotkey {
            key: HotkeyKey::F9,
            modifiers: HotkeyModifiers::none(),
        });
        let mut manager = HotkeyManager::new();
        register_hotkey_binding(&mut manager, hotkey);
        let transcriber = Arc::new(PipelineTranscriber::new(model_root.clone(), ModelId::Base));

        Self {
            state: PttState::Idle,
            armed: false,
            hotkey,
            hotkey_manager: Arc::new(Mutex::new(manager)),
            hotkey_listener: None,
            hotkey_receiver: None,
            runtime_started: false,
            level_task_started: false,
            capture: PttCaptureService::new(backend, "ptt"),
            transcriber,
            injector: Arc::new(ClipboardInjector),
            settings: AppSettings::default(),
            model_root,
        }
    }

    pub fn current_state(&self) -> PttState {
        self.state.clone()
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
        let model_id = model_id_from_name(model_name.as_deref());
        self.transcriber = Arc::new(PipelineTranscriber::new(self.model_root.clone(), model_id));
    }

    pub fn update_settings(&mut self, settings: AppSettings) {
        self.settings = settings;
    }

    pub fn start_with_handle(
        handle: Arc<Mutex<Self>>,
        settings: AppSettings,
        active_model: Option<String>,
    ) -> Result<PttState, String> {
        Self::ensure_runtime(Arc::clone(&handle))?;
        let mut controller = lock_controller(&handle);
        controller.arm(settings, active_model)
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

    fn ensure_runtime(handle: Arc<Mutex<Self>>) -> Result<(), String> {
        let mut controller = lock_controller(&handle);
        if controller.runtime_started {
            return Ok(());
        }

        if controller.hotkey_receiver.is_none() {
            let listener = GlobalHotkeyListener::new(Arc::clone(&controller.hotkey_manager));
            let (handle_listener, receiver) = listener.start().map_err(|err| err.to_string())?;
            controller.hotkey_listener = Some(handle_listener);
            controller.hotkey_receiver = Some(receiver);
        }

        if !controller.level_task_started {
            if let Some(receiver) = controller.capture.level_feed() {
                let handle_clone = Arc::clone(&handle);
                std::thread::spawn(move || {
                    for reading in receiver {
                        let should_emit = {
                            let controller = lock_controller(&handle_clone);
                            controller.armed
                        };
                        if should_emit {
                            emit_level(reading);
                        }
                    }
                });
                controller.level_task_started = true;
            }
        }

        let receiver = controller
            .hotkey_receiver
            .take()
            .ok_or_else(|| "hotkey receiver missing".to_string())?;
        controller.runtime_started = true;
        drop(controller);

        let handle_clone = Arc::clone(&handle);
        std::thread::spawn(move || {
            for event in receiver {
                let work = {
                    let mut controller = lock_controller(&handle_clone);
                    match controller.handle_hotkey_action(&event) {
                        Ok(value) => value,
                        Err(err) => {
                            controller.emit_error(&err);
                            None
                        }
                    }
                };

                if let Some(work) = work {
                    let transcription = work.transcriber.transcribe(&work.audio);
                    if let Ok(text) = &transcription {
                        if work.auto_export && !text.is_empty() {
                            let _ = work.injector.inject(text);
                        }
                    }

                    let mut controller = lock_controller(&handle_clone);
                    controller.complete_transcription(transcription);
                }
            }
        });

        Ok(())
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
        self.capture
            .handle_hotkey_action(event)
            .map_err(|err| err.to_string())?;

        match event.state {
            HotkeyState::Pressed => {
                self.set_state(PttState::Capturing);
                Ok(None)
            }
            HotkeyState::Released => {
                self.set_state(PttState::Processing);
                let audio = self.capture.take_audio().map_err(|err| err.to_string())?;
                Ok(Some(TranscriptionWork {
                    audio,
                    transcriber: Arc::clone(&self.transcriber),
                    injector: Arc::clone(&self.injector),
                    auto_export: self.settings.auto_export,
                }))
            }
        }
    }

    fn complete_transcription(&mut self, result: Result<String, String>) {
        match result {
            Ok(text) => {
                emit_app_event(PTT_TRANSCRIPTION_EVENT, &text);
                self.set_state(if self.armed {
                    PttState::Armed
                } else {
                    PttState::Idle
                });
            }
            Err(err) => {
                self.emit_error(&err);
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
        emit_app_event(PTT_STATE_EVENT, &next);
    }
}

struct TranscriptionWork {
    audio: Vec<f32>,
    transcriber: Arc<dyn Transcriber>,
    injector: Arc<dyn TextInjector>,
    auto_export: bool,
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

fn model_id_from_name(name: Option<&str>) -> ModelId {
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

fn register_standard_models(manager: &mut ModelManager, root: &Path) {
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
            .with_download_url(file_url(root.join(&filename)));
        manager.register_model(spec);
    }
}

fn register_custom_model(manager: &mut ModelManager, root: &Path, name: &str) {
    let filename = format!("{name}.bin");
    let spec = ModelSpec::new(ModelId::Custom(name.to_string()), filename.clone())
        .with_download_url(file_url(root.join(&filename)));
    manager.register_model(spec);
}

fn file_url(path: PathBuf) -> String {
    format!("file://{}", path.display())
}

fn emit_level(reading: LevelReading) {
    let level = PttLevel {
        rms: reading.rms,
        peak: reading.peak,
    };
    emit_app_event(PTT_LEVEL_EVENT, &level);
}

fn lock_controller<T>(handle: &Arc<Mutex<T>>) -> std::sync::MutexGuard<'_, T> {
    handle
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[derive(Default)]
pub struct CpalAudioBackend {
    host: cpal::Host,
}

pub struct CpalAudioStream {
    stream: cpal::Stream,
}

impl AudioStream for CpalAudioStream {
    fn start(&self) -> Result<(), AudioError> {
        self.stream
            .play()
            .map_err(|err| AudioError::Backend(err.to_string()))
    }

    fn stop(&self) -> Result<(), AudioError> {
        self.stream
            .pause()
            .map_err(|err| AudioError::Backend(err.to_string()))
    }
}

impl CpalAudioBackend {
    fn device_from_id(&self, device: &AudioDevice) -> Result<cpal::Device, AudioError> {
        if device.id.starts_with("default:") {
            return self
                .host
                .default_input_device()
                .ok_or(AudioError::NoInputDevice);
        }

        let mut parts = device.id.splitn(2, ':');
        let index = parts
            .next()
            .and_then(|value| value.parse::<usize>().ok())
            .ok_or(AudioError::DeviceNotFound)?;

        let mut devices = self
            .host
            .input_devices()
            .map_err(|err| AudioError::Backend(err.to_string()))?;
        devices.nth(index).ok_or(AudioError::DeviceNotFound)
    }
}

impl AudioBackend for CpalAudioBackend {
    type Stream = CpalAudioStream;

    fn list_input_devices(&self) -> Result<Vec<AudioDevice>, AudioError> {
        let mut devices = Vec::new();
        for (index, device) in self
            .host
            .input_devices()
            .map_err(|err| AudioError::Backend(err.to_string()))?
            .enumerate()
        {
            let name = device
                .name()
                .map_err(|err| AudioError::Backend(err.to_string()))?;
            devices.push(AudioDevice {
                id: format!("{}:{}", index, name),
                name,
            });
        }
        Ok(devices)
    }

    fn default_input_device(&self) -> Result<Option<AudioDevice>, AudioError> {
        let device = match self.host.default_input_device() {
            Some(device) => device,
            None => return Ok(None),
        };

        let name = device
            .name()
            .map_err(|err| AudioError::Backend(err.to_string()))?;

        Ok(Some(AudioDevice {
            id: format!("default:{}", name),
            name,
        }))
    }

    fn build_input_stream(
        &self,
        device: &AudioDevice,
        mut on_samples: Box<dyn FnMut(&[f32]) + Send>,
    ) -> Result<Self::Stream, AudioError> {
        let device = self.device_from_id(device)?;
        let default_config = device
            .default_input_config()
            .map_err(|err| AudioError::Backend(err.to_string()))?;
        let stream_config: cpal::StreamConfig = default_config.clone().into();

        let error_callback = |err| {
            eprintln!("audio input stream error: {err}");
        };

        let stream = match default_config.sample_format() {
            cpal::SampleFormat::F32 => device
                .build_input_stream(
                    &stream_config,
                    move |data: &[f32], _| on_samples(data),
                    error_callback,
                    None,
                )
                .map_err(|err| AudioError::Backend(err.to_string()))?,
            cpal::SampleFormat::I16 => device
                .build_input_stream(
                    &stream_config,
                    move |data: &[i16], _| {
                        let converted: Vec<f32> = data
                            .iter()
                            .map(|value| *value as f32 / i16::MAX as f32)
                            .collect();
                        on_samples(&converted);
                    },
                    error_callback,
                    None,
                )
                .map_err(|err| AudioError::Backend(err.to_string()))?,
            cpal::SampleFormat::U16 => device
                .build_input_stream(
                    &stream_config,
                    move |data: &[u16], _| {
                        let midpoint = (u16::MAX as f32 + 1.0) / 2.0;
                        let converted: Vec<f32> = data
                            .iter()
                            .map(|value| (*value as f32 - midpoint) / midpoint)
                            .collect();
                        on_samples(&converted);
                    },
                    error_callback,
                    None,
                )
                .map_err(|err| AudioError::Backend(err.to_string()))?,
            _ => return Err(AudioError::Backend("unsupported sample format".to_string())),
        };

        Ok(CpalAudioStream { stream })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
                }],
                controller: Arc::new(Mutex::new(None)),
            }
        }

        fn controller(&self) -> MockStreamController {
            self.controller
                .lock()
                .expect("lock")
                .clone()
                .expect("controller")
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
        let mut controller = PttController::with_backend(backend, std::env::temp_dir());
        controller.transcriber = Arc::new(MockTranscriber);
        controller.injector = Arc::new(MockInjector { sender: inject_tx });

        controller
            .arm(AppSettings::default(), Some("base".to_string()))
            .expect("arm");

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
        if work.auto_export {
            work.injector.inject(&text).expect("inject");
        }

        let injected = inject_rx
            .recv_timeout(Duration::from_millis(50))
            .expect("inject event");
        assert_eq!(injected, "hello world");
    }
}
