use crate::audio::{AudioBackend, AudioCaptureService, AudioError};
use crate::hotkeys::{HotkeyActionEvent, HotkeyState};
use crate::meter::{LevelMeter, LevelReading};
use std::sync::{
    atomic::{AtomicBool, Ordering},
    mpsc, Arc, Mutex,
};

#[derive(Debug, thiserror::Error)]
pub enum PttCaptureError {
    #[error(transparent)]
    Audio(#[from] AudioError),
    #[error("capture buffer lock was poisoned")]
    BufferLockPoisoned,
    #[error("level meter lock was poisoned")]
    MeterLockPoisoned,
}

pub struct PttCaptureService<B: AudioBackend> {
    action: String,
    audio: AudioCaptureService<B>,
    buffer: Arc<Mutex<Vec<f32>>>,
    capture_active: Arc<AtomicBool>,
    meter: Arc<Mutex<LevelMeter>>,
    level_sender: mpsc::Sender<LevelReading>,
    level_receiver: Option<mpsc::Receiver<LevelReading>>,
}

impl<B: AudioBackend> PttCaptureService<B> {
    pub fn new(backend: B, action: impl Into<String>) -> Self {
        let (level_sender, level_receiver) = mpsc::channel();
        Self {
            action: action.into(),
            audio: AudioCaptureService::new(backend),
            buffer: Arc::new(Mutex::new(Vec::new())),
            capture_active: Arc::new(AtomicBool::new(false)),
            meter: Arc::new(Mutex::new(LevelMeter::new())),
            level_sender,
            level_receiver: Some(level_receiver),
        }
    }

    pub fn audio(&self) -> &AudioCaptureService<B> {
        &self.audio
    }

    pub fn audio_mut(&mut self) -> &mut AudioCaptureService<B> {
        &mut self.audio
    }

    pub fn start(&mut self) -> Result<(), PttCaptureError> {
        self.capture_active.store(false, Ordering::SeqCst);
        {
            let mut buffer = self
                .buffer
                .lock()
                .map_err(|_| PttCaptureError::BufferLockPoisoned)?;
            buffer.clear();
        }
        {
            let mut meter = self
                .meter
                .lock()
                .map_err(|_| PttCaptureError::MeterLockPoisoned)?;
            meter.reset();
        }

        let buffer = Arc::clone(&self.buffer);
        let meter = Arc::clone(&self.meter);
        let capture_active = Arc::clone(&self.capture_active);
        let level_sender = self.level_sender.clone();

        self.audio
            .start_with_callback(move |samples| {
                if let Ok(mut meter) = meter.lock() {
                    meter.update(samples);
                    let _ = level_sender.send(meter.reading());
                }

                if capture_active.load(Ordering::SeqCst) {
                    if let Ok(mut buffer) = buffer.lock() {
                        buffer.extend_from_slice(samples);
                    }
                }
            })
            .map_err(PttCaptureError::from)
    }

    pub fn stop(&mut self) -> Result<(), PttCaptureError> {
        self.capture_active.store(false, Ordering::SeqCst);
        self.audio.stop().map_err(PttCaptureError::from)
    }

    pub fn take_audio(&self) -> Result<Vec<f32>, PttCaptureError> {
        let mut buffer = self
            .buffer
            .lock()
            .map_err(|_| PttCaptureError::BufferLockPoisoned)?;
        Ok(std::mem::take(&mut *buffer))
    }

    pub fn level_feed(&mut self) -> Option<mpsc::Receiver<LevelReading>> {
        self.level_receiver.take()
    }

    pub fn level(&self) -> Result<LevelReading, PttCaptureError> {
        let meter = self
            .meter
            .lock()
            .map_err(|_| PttCaptureError::MeterLockPoisoned)?;
        Ok(meter.reading())
    }

    pub fn handle_hotkey_action(
        &mut self,
        event: &HotkeyActionEvent,
    ) -> Result<(), PttCaptureError> {
        if event.action != self.action {
            return Ok(());
        }

        match event.state {
            HotkeyState::Pressed => {
                self.capture_active.store(true, Ordering::SeqCst);
                let mut buffer = self
                    .buffer
                    .lock()
                    .map_err(|_| PttCaptureError::BufferLockPoisoned)?;
                buffer.clear();
            }
            HotkeyState::Released => {
                self.capture_active.store(false, Ordering::SeqCst);
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{PttCaptureError, PttCaptureService};
    use crate::audio::{AudioBackend, AudioDevice, AudioError, AudioStream};
    use crate::hotkeys::{Hotkey, HotkeyActionEvent, HotkeyKey, HotkeyModifiers, HotkeyState};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    };
    use std::time::Duration;

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
        fn new(devices: Vec<AudioDevice>) -> Self {
            Self {
                devices,
                controller: Arc::new(Mutex::new(None)),
            }
        }

        fn controller(&self) -> Option<MockStreamController> {
            self.controller.lock().ok()?.clone()
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

            if let Ok(mut stored) = self.controller.lock() {
                *stored = Some(controller.clone());
            }

            Ok(MockStream { controller })
        }
    }

    fn hotkey_event(state: HotkeyState) -> HotkeyActionEvent {
        HotkeyActionEvent {
            action: "ptt".to_string(),
            hotkey: Hotkey {
                key: HotkeyKey::F9,
                modifiers: HotkeyModifiers::none(),
            },
            state,
        }
    }

    #[test]
    fn ptt_capture_buffers_samples_when_active() {
        let backend = MockAudioBackend::new(vec![AudioDevice {
            id: "0:Mock".to_string(),
            name: "Mock".to_string(),
            sample_rate: 48_000,
            channels: 2,
        }]);
        let controller_handle = backend.controller.clone();
        let mut service = PttCaptureService::new(backend, "ptt");
        service.start().expect("start capture");

        let controller = controller_handle
            .lock()
            .ok()
            .and_then(|value| value.clone())
            .expect("controller ready");

        controller.push_samples(&[0.1, 0.2]);
        assert!(service.take_audio().expect("take audio").is_empty());

        service
            .handle_hotkey_action(&hotkey_event(HotkeyState::Pressed))
            .expect("activate capture");
        controller.push_samples(&[0.25, -0.25, 0.5]);

        let captured = service.take_audio().expect("take audio");
        assert_eq!(captured, vec![0.25, -0.25, 0.5]);

        service
            .handle_hotkey_action(&hotkey_event(HotkeyState::Released))
            .expect("deactivate capture");
        controller.push_samples(&[0.3]);
        assert!(service.take_audio().expect("take audio").is_empty());
    }

    #[test]
    fn ptt_capture_emits_level_updates() {
        let backend = MockAudioBackend::new(vec![AudioDevice {
            id: "0:Mock".to_string(),
            name: "Mock".to_string(),
            sample_rate: 48_000,
            channels: 2,
        }]);
        let controller_handle = backend.controller.clone();
        let mut service = PttCaptureService::new(backend, "ptt");
        let receiver = service.level_feed().expect("level feed");
        service.start().expect("start capture");

        let controller = controller_handle
            .lock()
            .ok()
            .and_then(|value| value.clone())
            .expect("controller ready");
        controller.push_samples(&[0.5, -0.5]);

        let reading = receiver
            .recv_timeout(Duration::from_millis(50))
            .expect("level reading");
        assert!(reading.peak > 0.0);
    }

    #[test]
    fn ptt_capture_ignores_unrelated_actions() {
        let backend = MockAudioBackend::new(vec![AudioDevice {
            id: "0:Mock".to_string(),
            name: "Mock".to_string(),
            sample_rate: 48_000,
            channels: 2,
        }]);
        let mut service = PttCaptureService::new(backend, "ptt");
        service.start().expect("start capture");

        let event = HotkeyActionEvent {
            action: "other".to_string(),
            hotkey: Hotkey {
                key: HotkeyKey::F10,
                modifiers: HotkeyModifiers::none(),
            },
            state: HotkeyState::Pressed,
        };

        let result = service.handle_hotkey_action(&event);
        assert!(matches!(result, Ok(())));
    }

    #[test]
    fn ptt_capture_returns_error_when_buffer_poisoned() {
        let backend = MockAudioBackend::new(vec![AudioDevice {
            id: "0:Mock".to_string(),
            name: "Mock".to_string(),
            sample_rate: 48_000,
            channels: 2,
        }]);
        let mut service = PttCaptureService::new(backend, "ptt");

        let buffer = service.buffer.clone();
        let _ = std::panic::catch_unwind(move || {
            let _guard = buffer.lock().expect("lock");
            panic!("poison");
        });

        let result = service.take_audio();
        assert!(matches!(result, Err(PttCaptureError::BufferLockPoisoned)));
    }

    #[test]
    fn ptt_capture_propagates_level_feed_drop() {
        let backend = MockAudioBackend::new(vec![AudioDevice {
            id: "0:Mock".to_string(),
            name: "Mock".to_string(),
            sample_rate: 48_000,
            channels: 2,
        }]);
        let controller_handle = backend.controller.clone();
        let mut service = PttCaptureService::new(backend, "ptt");
        let receiver = service.level_feed().expect("level feed");
        drop(receiver);

        service.start().expect("start capture");

        let controller = controller_handle
            .lock()
            .ok()
            .and_then(|value| value.clone())
            .expect("controller ready");
        controller.push_samples(&[0.5]);
    }
}
