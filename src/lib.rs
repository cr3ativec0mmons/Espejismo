pub mod adb;
pub mod app;
pub mod config;
pub mod model;
pub mod scrcpy;

#[cfg(feature = "gui")]
pub mod ui;

pub const APPLICATION_ID: &str = "com.espejismo.Espejismo";
