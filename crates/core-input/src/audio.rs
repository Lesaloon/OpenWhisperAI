use crate::meter::{LevelMeter, LevelReading};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
}

#[derive(Debug, thiserror::Error)]
pub enum AudioError {
    #[error("audio backend error: {0}")]
    Backend(String),
    #[error("no input devices available")]
    NoInputDevice,
    #[error("input device not found")]
    DeviceNotFound,
    #[error("audio capture already running")]
    AlreadyRunning,
    #[error("audio capture is not running")]
    NotRunning,
    #[error("level meter lock was poisoned")]
    MeterLockPoisoned,
}

pub trait AudioStream {
    fn start(&self) -> Result<(), AudioError>;
    fn stop(&self) -> Result<(), AudioError>;
}

pub trait AudioBackend: Send + Sync + 'static {
    type Stream: AudioStream;

    fn list_input_devices(&self) -> Result<Vec<AudioDevice>, AudioError>;
    fn default_input_device(&self) -> Result<Option<AudioDevice>, AudioError>;
    fn build_input_stream(
        &self,
        device: &AudioDevice,
        on_samples: Box<dyn FnMut(&[f32]) + Send>,
    ) -> Result<Self::Stream, AudioError>;
}

fn normalize_u16_sample(value: u16) -> f32 {
    let midpoint = (u16::MAX as f32 + 1.0) / 2.0;
    (value as f32 - midpoint) / midpoint
}

pub struct AudioCaptureService<B: AudioBackend> {
    backend: B,
    devices: Vec<AudioDevice>,
    selected_device: Option<AudioDevice>,
    meter: Arc<Mutex<LevelMeter>>,
    stream: Option<B::Stream>,
}

impl<B: AudioBackend> AudioCaptureService<B> {
    pub fn new(backend: B) -> Self {
        Self {
            backend,
            devices: Vec::new(),
            selected_device: None,
            meter: Arc::new(Mutex::new(LevelMeter::new())),
            stream: None,
        }
    }

    pub fn refresh_devices(&mut self) -> Result<&[AudioDevice], AudioError> {
        self.devices = self.backend.list_input_devices()?;
        Ok(&self.devices)
    }

    pub fn devices(&self) -> &[AudioDevice] {
        &self.devices
    }

    pub fn select_device(&mut self, device_id: &str) -> Result<(), AudioError> {
        if let Some(device) = self
            .devices
            .iter()
            .find(|device| device.id == device_id)
            .cloned()
        {
            self.selected_device = Some(device);
            return Ok(());
        }

        Err(AudioError::DeviceNotFound)
    }

    pub fn selected_device(&self) -> Option<&AudioDevice> {
        self.selected_device.as_ref()
    }

    pub fn is_running(&self) -> bool {
        self.stream.is_some()
    }

    pub fn start(&mut self) -> Result<(), AudioError> {
        self.start_internal(None)
    }

    pub fn start_with_callback(
        &mut self,
        callback: impl FnMut(&[f32]) + Send + 'static,
    ) -> Result<(), AudioError> {
        self.start_internal(Some(Box::new(callback)))
    }

    fn start_internal(
        &mut self,
        mut callback: Option<Box<dyn FnMut(&[f32]) + Send>>,
    ) -> Result<(), AudioError> {
        if self.stream.is_some() {
            return Err(AudioError::AlreadyRunning);
        }

        let device = match self.selected_device.clone() {
            Some(device) => device,
            None => self
                .backend
                .default_input_device()?
                .ok_or(AudioError::NoInputDevice)?,
        };

        let meter = Arc::clone(&self.meter);
        let mut on_samples = move |samples: &[f32]| {
            if let Ok(mut meter) = meter.lock() {
                meter.update(samples);
            }
            if let Some(handler) = callback.as_mut() {
                handler(samples);
            }
        };

        let stream = self
            .backend
            .build_input_stream(&device, Box::new(move |samples| on_samples(samples)))?;
        if let Ok(mut meter) = self.meter.lock() {
            meter.reset();
        }
        stream.start()?;
        self.selected_device = Some(device.clone());
        self.stream = Some(stream);
        Ok(())
    }

    pub fn stop(&mut self) -> Result<(), AudioError> {
        let stream = self.stream.take().ok_or(AudioError::NotRunning)?;
        stream.stop()?;
        Ok(())
    }

    pub fn level(&self) -> Result<LevelReading, AudioError> {
        let meter = self
            .meter
            .lock()
            .map_err(|_| AudioError::MeterLockPoisoned)?;
        Ok(meter.reading())
    }
}

pub struct CpalAudioBackend {
    host: cpal::Host,
}

impl Default for CpalAudioBackend {
    fn default() -> Self {
        Self {
            host: cpal::default_host(),
        }
    }
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
                        let converted: Vec<f32> = data
                            .iter()
                            .map(|value| normalize_u16_sample(*value))
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
    use super::{
        normalize_u16_sample, AudioBackend, AudioCaptureService, AudioDevice, AudioError,
        AudioStream,
    };
    use crate::meter::LevelReading;
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    };

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

    #[test]
    fn capture_service_updates_level_meter() {
        let backend = MockAudioBackend::new(vec![AudioDevice {
            id: "0:Mock".to_string(),
            name: "Mock".to_string(),
        }]);
        let controller_handle = backend.controller.clone();
        let mut service = AudioCaptureService::new(backend);
        service.refresh_devices().expect("devices");
        service.select_device("0:Mock").expect("select device");
        service.start().expect("start capture");

        let controller = controller_handle
            .lock()
            .ok()
            .and_then(|value| value.clone())
            .expect("controller ready");
        controller.push_samples(&[0.5, -0.5]);

        let reading = service.level().expect("meter");
        assert!(reading.peak > 0.0);
    }

    #[test]
    fn capture_service_returns_silence_before_audio() {
        let backend = MockAudioBackend::new(vec![AudioDevice {
            id: "0:Mock".to_string(),
            name: "Mock".to_string(),
        }]);
        let service = AudioCaptureService::new(backend);
        let reading = service.level().expect("meter");
        assert_eq!(reading, LevelReading::silence());
    }

    #[test]
    fn capture_service_tracks_running_state() {
        let backend = MockAudioBackend::new(vec![AudioDevice {
            id: "0:Mock".to_string(),
            name: "Mock".to_string(),
        }]);
        let mut service = AudioCaptureService::new(backend);
        assert!(!service.is_running());
        service.start().expect("start capture");
        assert!(service.is_running());
        service.stop().expect("stop capture");
        assert!(!service.is_running());
    }

    #[test]
    fn capture_service_sets_default_device_on_start() {
        let backend = MockAudioBackend::new(vec![AudioDevice {
            id: "0:Mock".to_string(),
            name: "Mock".to_string(),
        }]);
        let mut service = AudioCaptureService::new(backend);
        service.start().expect("start capture");

        let selected = service.selected_device().expect("device selected");
        assert_eq!(selected.id, "0:Mock");
    }

    #[test]
    fn capture_service_resets_meter_on_start() {
        let backend = MockAudioBackend::new(vec![AudioDevice {
            id: "0:Mock".to_string(),
            name: "Mock".to_string(),
        }]);
        let controller_handle = backend.controller.clone();
        let mut service = AudioCaptureService::new(backend);
        service.start().expect("start capture");

        let controller = controller_handle
            .lock()
            .ok()
            .and_then(|value| value.clone())
            .expect("controller ready");
        controller.push_samples(&[0.5, -0.5]);
        assert!(service.level().expect("meter").peak > 0.0);

        service.stop().expect("stop capture");
        service.start().expect("start capture again");

        let reading = service.level().expect("meter after restart");
        assert_eq!(reading, LevelReading::silence());
    }

    #[test]
    fn u16_normalization_centers_at_zero() {
        let min = normalize_u16_sample(u16::MIN);
        let mid = normalize_u16_sample(0x8000);
        let max = normalize_u16_sample(u16::MAX);

        assert!((min + 1.0).abs() < 1e-6);
        assert!(mid.abs() < 1e-6);
        assert!(max <= 1.0);
        assert!(max > 0.99);
    }
}
