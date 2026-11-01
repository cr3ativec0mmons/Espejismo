use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AppState {
    NoAdb,
    LoadingDevices,
    DeviceAvailable,
    Pairing,
    Connecting,
    Connected,
    LaunchingScrcpy,
    Streaming,
    Disconnected,
    Error,
}
