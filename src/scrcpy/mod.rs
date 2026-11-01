pub mod capabilities;
pub mod launcher;

pub use capabilities::{ScrcpyVersion, VersionError, detect_version, parse_version};
pub use launcher::{ScrcpyError, ScrcpyLauncher, ScrcpyOptions};
