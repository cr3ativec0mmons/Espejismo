use std::io;
use std::path::Path;
use std::process::Command;

use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct ScrcpyVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

#[derive(Debug, Error)]
pub enum VersionError {
    #[error("scrcpy executable is unavailable: {0}")]
    Start(#[source] io::Error),
    #[error("scrcpy version command failed with status {status:?}: {stderr}")]
    Command { status: Option<i32>, stderr: String },
    #[error("could not find a scrcpy version in output")]
    InvalidOutput,
}

pub fn detect_version(path: impl AsRef<Path>) -> Result<ScrcpyVersion, VersionError> {
    let output = Command::new(path.as_ref())
        .arg("--version")
        .output()
        .map_err(VersionError::Start)?;
    if output.status.success() {
        parse_version(&String::from_utf8_lossy(&output.stdout))
            .or_else(|_| parse_version(&String::from_utf8_lossy(&output.stderr)))
    } else {
        Err(VersionError::Command {
            status: output.status.code(),
            stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
        })
    }
}

pub fn parse_version(output: &str) -> Result<ScrcpyVersion, VersionError> {
    output
        .split_whitespace()
        .find_map(|token| {
            let token = token.trim_start_matches('v');
            let mut parts = token.split('.');
            let major = parts.next()?.parse().ok()?;
            let minor = parts.next()?.parse().ok()?;
            let patch = parts.next().unwrap_or("0").parse().ok()?;
            Some(ScrcpyVersion {
                major,
                minor,
                patch,
            })
        })
        .ok_or(VersionError::InvalidOutput)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_release_output() {
        assert_eq!(
            parse_version("scrcpy 3.3.1 <https://github.com/Genymobile/scrcpy>").expect("version"),
            ScrcpyVersion {
                major: 3,
                minor: 3,
                patch: 1
            }
        );
    }

    #[test]
    fn rejects_output_without_version() {
        assert!(matches!(
            parse_version("scrcpy unavailable"),
            Err(VersionError::InvalidOutput)
        ));
    }
}
