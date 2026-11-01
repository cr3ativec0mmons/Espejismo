use serde::{Deserialize, Serialize};

use super::{DeviceId, Transport};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Theme {
    System,
    Light,
    Dark,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Decoder {
    #[default]
    Auto,
    Vaapi,
    Software,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScrcpySettings {
    #[serde(default)]
    pub codec: Option<String>,
    #[serde(default)]
    pub bitrate: Option<String>,
    #[serde(default)]
    pub decoder: Decoder,
    #[serde(default = "default_control_enabled")]
    pub control_enabled: bool,
}

fn default_control_enabled() -> bool {
    true
}

impl Default for ScrcpySettings {
    fn default() -> Self {
        Self {
            codec: None,
            bitrate: None,
            decoder: Decoder::Auto,
            control_enabled: true,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Settings {
    pub version: u32,
    pub theme: Theme,
    #[serde(default)]
    pub devices: Vec<SavedDevice>,
    #[serde(default)]
    pub device_overrides: std::collections::HashMap<DeviceId, ScrcpySettings>,
    #[serde(default)]
    pub scrcpy: ScrcpySettings,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedDevice {
    pub id: DeviceId,
    pub alias: Option<String>,
    pub transport: Transport,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            version: 1,
            theme: Theme::System,
            devices: Vec::new(),
            device_overrides: std::collections::HashMap::new(),
            scrcpy: ScrcpySettings::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_keep_control_enabled() {
        assert!(Settings::default().scrcpy.control_enabled);
    }

    #[test]
    fn unknown_fields_are_ignored() {
        let settings: Settings = serde_json::from_str(
            r#"{"version":1,"theme":"dark","scrcpy":{"decoder":"auto","control_enabled":true,"future":42},"future":true}"#,
        )
        .expect("settings with unknown fields should deserialize");
        assert_eq!(settings.theme, Theme::Dark);
    }

    #[test]
    fn older_settings_receive_empty_device_collections() {
        let settings: Settings = serde_json::from_str(
            r#"{"version":1,"theme":"system","scrcpy":{"decoder":"auto","control_enabled":true}}"#,
        )
        .expect("legacy settings");
        assert!(settings.devices.is_empty());
        assert!(settings.device_overrides.is_empty());
    }
}
