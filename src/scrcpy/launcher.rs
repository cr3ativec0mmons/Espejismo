use std::collections::HashSet;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::Mutex;

use thiserror::Error;

use crate::model::{Decoder, DeviceId, ScrcpySettings};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrcpyOptions {
    pub codec: Option<String>,
    pub bitrate: Option<String>,
    pub decoder: Decoder,
    pub control_enabled: bool,
}

impl TryFrom<&ScrcpySettings> for ScrcpyOptions {
    type Error = ScrcpyError;

    fn try_from(settings: &ScrcpySettings) -> Result<Self, Self::Error> {
        let codec = settings.codec.as_deref().map(validate_codec).transpose()?;
        let bitrate = settings
            .bitrate
            .as_deref()
            .map(validate_bitrate)
            .transpose()?;
        Ok(Self {
            codec,
            bitrate,
            decoder: settings.decoder,
            control_enabled: settings.control_enabled,
        })
    }
}

#[derive(Debug, Error)]
pub enum ScrcpyError {
    #[error("unsupported scrcpy codec: {0}")]
    InvalidCodec(String),
    #[error("invalid scrcpy bitrate: {0}")]
    InvalidBitrate(String),
    #[error("a scrcpy session is already running for device {0}")]
    AlreadyRunning(DeviceId),
    #[error("could not launch scrcpy: {0}")]
    Launch(#[source] std::io::Error),
}

pub struct ScrcpyLauncher {
    executable: PathBuf,
    sessions: Mutex<HashSet<DeviceId>>,
}

impl ScrcpyLauncher {
    pub fn from_path(path: impl Into<PathBuf>) -> Self {
        Self {
            executable: path.into(),
            sessions: Mutex::new(HashSet::new()),
        }
    }

    pub fn from_path_environment() -> Self {
        Self::from_path("scrcpy")
    }

    pub fn arguments(device_id: &DeviceId, options: &ScrcpyOptions) -> Vec<OsString> {
        let mut arguments = vec!["--serial".into(), device_id.as_str().into()];
        if let Some(codec) = &options.codec {
            arguments.extend(["--video-codec".into(), codec.clone().into()]);
        }
        if let Some(bitrate) = &options.bitrate {
            arguments.extend(["--video-bit-rate".into(), bitrate.clone().into()]);
        }
        if !options.control_enabled {
            arguments.push("--no-control".into());
        }
        arguments
    }

    pub fn launch(
        &self,
        device_id: &DeviceId,
        options: &ScrcpyOptions,
    ) -> Result<Child, ScrcpyError> {
        let mut sessions = self.sessions.lock().expect("scrcpy sessions lock");
        if !sessions.insert(device_id.clone()) {
            return Err(ScrcpyError::AlreadyRunning(device_id.clone()));
        }
        let result = Command::new(&self.executable)
            .args(Self::arguments(device_id, options))
            .spawn();
        match result {
            Ok(child) => Ok(child),
            Err(error) => {
                sessions.remove(device_id);
                Err(ScrcpyError::Launch(error))
            }
        }
    }

    pub fn release(&self, device_id: &DeviceId) {
        self.sessions
            .lock()
            .expect("scrcpy sessions lock")
            .remove(device_id);
    }
}

fn validate_codec(codec: &str) -> Result<String, ScrcpyError> {
    match codec {
        "h264" | "h265" | "av1" => Ok(codec.to_owned()),
        _ => Err(ScrcpyError::InvalidCodec(codec.to_owned())),
    }
}

fn validate_bitrate(bitrate: &str) -> Result<String, ScrcpyError> {
    let valid = !bitrate.is_empty()
        && bitrate.len() <= 16
        && bitrate.chars().all(|character| {
            character.is_ascii_digit() || matches!(character, 'k' | 'K' | 'm' | 'M')
        });
    valid
        .then(|| bitrate.to_owned())
        .ok_or_else(|| ScrcpyError::InvalidBitrate(bitrate.to_owned()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_serial_and_optional_arguments_without_control_override_by_default() {
        let device = DeviceId::new("usb-serial").expect("device id");
        let options = ScrcpyOptions {
            codec: Some("h265".to_owned()),
            bitrate: Some("8M".to_owned()),
            decoder: Decoder::Auto,
            control_enabled: true,
        };
        let arguments = ScrcpyLauncher::arguments(&device, &options);
        assert_eq!(
            arguments,
            [
                "--serial",
                "usb-serial",
                "--video-codec",
                "h265",
                "--video-bit-rate",
                "8M"
            ]
        );
        assert!(!arguments.iter().any(|argument| argument == "--no-control"));
    }

    #[test]
    fn validates_options_before_launch() {
        let settings = ScrcpySettings {
            codec: Some("vp9".to_owned()),
            ..ScrcpySettings::default()
        };
        assert!(matches!(
            ScrcpyOptions::try_from(&settings),
            Err(ScrcpyError::InvalidCodec(codec)) if codec == "vp9"
        ));
    }
}
