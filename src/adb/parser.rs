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
    let devices = output
        .lines()
        .filter(|line| !line.trim().is_empty() && !line.starts_with("List of devices attached"))
        .map(parse_device_line)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(deduplicate_wireless_mdns_devices(devices))
}

/// ADB can expose the same Wi-Fi device both as `host:port` and as its mDNS
/// service name after QR pairing. Keep the directly connected endpoint, which
/// is also the form scrcpy can use consistently.
fn deduplicate_wireless_mdns_devices(devices: Vec<AdbDevice>) -> Vec<AdbDevice> {
    devices
        .iter()
        .filter(|device| {
            !is_mdns_serial(device.id.as_str())
                || !devices.iter().any(|other| {
                    other.transport == Transport::Wifi
                        && !is_mdns_serial(other.id.as_str())
                        && (device.model.is_some() || device.product.is_some())
                        && other.model == device.model
                        && other.product == device.product
                })
        })
        .cloned()
        .collect()
}

fn is_mdns_serial(serial: &str) -> bool {
    serial.ends_with("._adb-tls-connect._tcp") || serial.ends_with("._adb-tls-connect._tcp.")
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
        transport: if serial.contains(':')
            || serial.ends_with("._adb-tls-connect._tcp")
            || serial.ends_with("._adb-tls-connect._tcp.")
        {
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
    fn recognizes_wireless_mdns_ipv6_and_usb_transports() {
        let devices = parse_devices(
            "adb-ABC-xyz._adb-tls-connect._tcp device model:Pixel\nadb-DEF-xyz._adb-tls-connect._tcp. offline\n[fe80::1]:5555 device\nUSB123 device usb:1-2 transport_id:4\n",
        ).unwrap();
        assert_eq!(
            devices.iter().map(|d| d.transport).collect::<Vec<_>>(),
            [
                Transport::Wifi,
                Transport::Wifi,
                Transport::Wifi,
                Transport::Usb
            ]
        );
    }

    #[test]
    fn removes_mdns_alias_when_direct_wireless_endpoint_is_present() {
        let devices = parse_devices(
            "adb-device._adb-tls-connect._tcp device product:foo model:Pixel\n10.0.0.31:39509 device product:foo model:Pixel\n",
        )
        .expect("valid device listing");

        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].id.as_str(), "10.0.0.31:39509");
    }

    #[test]
    fn keeps_distinct_wireless_devices_with_the_same_model() {
        let devices = parse_devices(
            "adb-one._adb-tls-connect._tcp device model:Pixel\n10.0.0.31:39509 device model:Pixel\n10.0.0.32:39509 device model:Pixel\n",
        )
        .expect("valid device listing");

        assert_eq!(devices.len(), 2);
        assert!(
            devices
                .iter()
                .any(|device| device.id.as_str() == "10.0.0.31:39509")
        );
        assert!(
            devices
                .iter()
                .any(|device| device.id.as_str() == "10.0.0.32:39509")
        );
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
