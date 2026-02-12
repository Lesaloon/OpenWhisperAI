use serde::{Deserialize, Serialize};

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
    use super::AppVersion;

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
}
