use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BackendState {
    Idle,
    Recording,
    Processing,
    Error { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum BackendEvent {
    StartRecording,
    StopRecording,
    StartProcessing,
    FinishProcessing,
    Fail { message: String },
    Reset,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ModelInstallStatus {
    Ready,
    Installed,
    Downloading,
    Queued,
    Pending,
    Failed,
    Error,
    #[default]
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelStatusItem {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub status: ModelInstallStatus,
    #[serde(default)]
    pub total_bytes: u64,
    #[serde(default)]
    pub downloaded_bytes: u64,
    #[serde(default)]
    pub speed_bytes_per_sec: u64,
    #[serde(default)]
    pub eta_seconds: u64,
    #[serde(default)]
    pub progress: f32,
    #[serde(default)]
    pub active: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ModelStatusPayload {
    #[serde(default)]
    pub models: Vec<ModelStatusItem>,
    #[serde(default)]
    pub active_model: Option<String>,
    #[serde(default)]
    pub queue_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum OverlayPosition {
    Docked,
    Floating,
    Compact,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub struct AppSettings {
    pub input_device: String,
    pub noise_reduction: bool,
    pub auto_language: bool,
    pub latency_ms: u16,
    pub auto_export: bool,
    pub overlay_position: OverlayPosition,
    pub show_timestamps: bool,
    pub auto_punctuation: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            input_device: "default".to_string(),
            noise_reduction: true,
            auto_language: false,
            latency_ms: 600,
            auto_export: true,
            overlay_position: OverlayPosition::Docked,
            show_timestamps: true,
            auto_punctuation: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub struct SettingsUpdate {
    #[serde(default)]
    pub input_device: Option<String>,
    #[serde(default)]
    pub noise_reduction: Option<bool>,
    #[serde(default)]
    pub auto_language: Option<bool>,
    #[serde(default)]
    pub latency_ms: Option<u16>,
    #[serde(default)]
    pub auto_export: Option<bool>,
    #[serde(default)]
    pub overlay_position: Option<OverlayPosition>,
    #[serde(default)]
    pub show_timestamps: Option<bool>,
    #[serde(default)]
    pub auto_punctuation: Option<bool>,
}

impl AppSettings {
    pub fn apply_update(&self, update: SettingsUpdate) -> Self {
        Self {
            input_device: update
                .input_device
                .unwrap_or_else(|| self.input_device.clone()),
            noise_reduction: update.noise_reduction.unwrap_or(self.noise_reduction),
            auto_language: update.auto_language.unwrap_or(self.auto_language),
            latency_ms: update.latency_ms.unwrap_or(self.latency_ms),
            auto_export: update.auto_export.unwrap_or(self.auto_export),
            overlay_position: update
                .overlay_position
                .unwrap_or_else(|| self.overlay_position.clone()),
            show_timestamps: update.show_timestamps.unwrap_or(self.show_timestamps),
            auto_punctuation: update.auto_punctuation.unwrap_or(self.auto_punctuation),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppVersion {
    pub major: u8,
    pub minor: u8,
    pub patch: u8,
}

impl AppVersion {
    pub const fn new(major: u8, minor: u8, patch: u8) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    pub fn as_string(&self) -> String {
        format!("{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[cfg(test)]
mod tests {
    use super::{AppSettings, AppVersion, OverlayPosition, SettingsUpdate};

    #[test]
    fn version_string_formats() {
        let version = AppVersion::new(1, 2, 3);
        assert_eq!(version.as_string(), "1.2.3");
    }

    #[test]
    fn version_roundtrips_json() {
        let version = AppVersion::new(0, 9, 0);
        let json = serde_json::to_string(&version).expect("serialize version");
        let decoded: AppVersion = serde_json::from_str(&json).expect("deserialize version");
        assert_eq!(decoded, version);
    }

    #[test]
    fn settings_update_merges_fields() {
        let settings = AppSettings::default();
        let update = SettingsUpdate {
            input_device: Some("USB Mic".to_string()),
            latency_ms: Some(900),
            overlay_position: Some(OverlayPosition::Floating),
            ..SettingsUpdate::default()
        };

        let merged = settings.apply_update(update);
        assert_eq!(merged.input_device, "USB Mic");
        assert_eq!(merged.latency_ms, 900);
        assert_eq!(merged.overlay_position, OverlayPosition::Floating);
        assert_eq!(merged.auto_export, settings.auto_export);
    }
}
