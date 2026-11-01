use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DeviceId(String);

impl DeviceId {
    pub fn new(serial: impl Into<String>) -> Option<Self> {
        let serial = serial.into();
        (!serial.trim().is_empty()).then_some(Self(serial))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for DeviceId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Transport {
    Usb,
    Wifi,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DeviceStatus {
    Device,
    Offline,
    Unauthorized,
    NoPermissions,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Device {
    pub id: DeviceId,
    pub alias: Option<String>,
    pub transport: Transport,
    pub status: DeviceStatus,
    pub model: Option<String>,
    pub product: Option<String>,
}

impl Device {
    pub fn is_usable(&self) -> bool {
        self.status == DeviceStatus::Device
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_empty_device_ids() {
        assert!(DeviceId::new("  ").is_none());
    }
}
