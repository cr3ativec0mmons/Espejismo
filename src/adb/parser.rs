use std::collections::HashMap;

use thiserror::Error;

use crate::model::{Device, DeviceId, DeviceStatus, Transport};

#[derive(Debug, Error, PartialEq, Eq)]
pub enum AdbParseError {
    #[error("invalid device row: {0}")]
    InvalidRow(String),
}

pub type AdbDevice = Device;

/// Analiza la salida de `adb devices -l` y construye los dispositivos observados.
pub fn parse_devices(output: &str) -> Result<Vec<AdbDevice>, AdbParseError> {
    output
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with("List of devices attached"))
        .map(parse_device_line)
        .collect()
}

/// Analiza una línea de dispositivo, incluido su estado y sus metadatos.
fn parse_device_line(line: &str) -> Result<AdbDevice, AdbParseError> {
    let fields = line.split_whitespace().collect::<Vec<_>>();
    let serial = fields
        .first()
        .copied()
        .ok_or_else(|| AdbParseError::InvalidRow(line.to_owned()))?;
    let status_end = if fields.get(1) == Some(&"no") && fields.get(2) == Some(&"permissions") {
        3
    } else {
        2
    };
    let raw_status = fields
        .get(1..status_end)
        .map(|parts| parts.join(" "))
        .ok_or_else(|| AdbParseError::InvalidRow(line.to_owned()))?;
    let attributes = fields
        .get(status_end..)
        .unwrap_or_default()
        .iter()
        .filter_map(|field| field.split_once(':'))
        .collect::<HashMap<_, _>>();
    let status = match raw_status.as_str() {
        "device" => DeviceStatus::Device,
        "offline" => DeviceStatus::Offline,
        "unauthorized" => DeviceStatus::Unauthorized,
        "no permissions" => DeviceStatus::NoPermissions,
        _ => DeviceStatus::Unknown,
    };
    let id = DeviceId::new(serial).ok_or_else(|| AdbParseError::InvalidRow(line.to_owned()))?;

    Ok(Device {
        id,
        alias: None,
        transport: if serial.contains(':') {
            Transport::Wifi
        } else {
            Transport::Usb
        },
        status,
        model: attributes.get("model").map(ToString::to_string),
        product: attributes.get("product").map(ToString::to_string),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// Verifica el análisis de dispositivos y sus metadatos conocidos.
    fn parses_devices_and_metadata() {
        let devices = parse_devices(
            "List of devices attached\nusb-serial\tdevice product:foo model:Pixel_8\n192.0.2.1:5555\tdevice model:Tablet\n",
        )
        .expect("valid device listing");
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].transport, Transport::Usb);
        assert_eq!(devices[0].model.as_deref(), Some("Pixel_8"));
        assert_eq!(devices[1].transport, Transport::Wifi);
    }

    #[test]
    /// Verifica que se conservan los estados de diagnóstico de ADB.
    fn preserves_diagnostic_states() {
        let devices = parse_devices(
            "List of devices attached\na\toffline\nb\tunauthorized\nc\tno permissions\n",
        )
        .expect("valid diagnostic listing");
        assert_eq!(devices[0].status, DeviceStatus::Offline);
        assert_eq!(devices[1].status, DeviceStatus::Unauthorized);
        assert_eq!(devices[2].status, DeviceStatus::NoPermissions);
    }
}
