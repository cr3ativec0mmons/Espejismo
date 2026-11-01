use std::ffi::OsString;
use std::io;
use std::path::PathBuf;
use std::process::{Command, Output};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum RunnerError {
    #[error("ADB executable is unavailable: {0}")]
    Start(#[source] io::Error),
    #[error("ADB command returned no output: {0}")]
    Output(#[source] io::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdbOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub status_code: Option<i32>,
}

impl From<Output> for AdbOutput {
    /// Convierte la salida de un proceso en la representación usada por ADB.
    fn from(output: Output) -> Self {
        Self {
            stdout: output.stdout,
            stderr: output.stderr,
            status_code: output.status.code(),
        }
    }
}

pub trait AdbRunner {
    /// Ejecuta ADB con argumentos separados y devuelve su salida capturada.
    fn run(&self, arguments: &[OsString]) -> Result<AdbOutput, RunnerError>;
}

#[derive(Debug, Clone)]
pub struct CommandAdbRunner {
    executable: PathBuf,
}

impl CommandAdbRunner {
    /// Crea un ejecutor que invoca el binario ubicado en la ruta indicada.
    pub fn from_path(path: impl Into<PathBuf>) -> Self {
        Self {
            executable: path.into(),
        }
    }

    /// Crea un ejecutor que busca `adb` en el entorno del proceso.
    pub fn from_path_environment() -> Self {
        Self::from_path("adb")
    }
}

impl AdbRunner for CommandAdbRunner {
    /// Ejecuta el binario configurado sin pasar por un shell.
    fn run(&self, arguments: &[OsString]) -> Result<AdbOutput, RunnerError> {
        Command::new(&self.executable)
            .args(arguments)
            .output()
            .map(AdbOutput::from)
            .map_err(RunnerError::Start)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// Verifica que un ejecutable inexistente produce un error de inicio.
    fn reports_missing_executable_without_shelling_out() {
        let runner = CommandAdbRunner::from_path("/definitely/missing/adb");
        let result = runner.run(&[OsString::from("devices")]);
        assert!(matches!(result, Err(RunnerError::Start(_))));
    }
}
