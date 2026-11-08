use std::ffi::OsString;

use thiserror::Error;

use crate::model::Device;

use super::{AdbParseError, AdbRunner, RunnerError, parse_devices};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Endpoint {
    host: String,
    port: u16,
}

impl Endpoint {
    /// Crea un endpoint con un host no vacío y un puerto válido.
    pub fn new(host: impl Into<String>, port: u16) -> Result<Self, AdbClientError> {
        let host = host.into();
        if host.trim().is_empty() || host.chars().any(char::is_whitespace) || port == 0 {
            return Err(AdbClientError::InvalidEndpoint);
        }
        Ok(Self { host, port })
    }

    /// Convierte el endpoint al formato `host:puerto` usado por ADB.
    fn as_argument(&self) -> OsString {
        if self.host.contains(':') && !self.host.starts_with('[') {
            format!("[{}]:{}", self.host, self.port).into()
        } else {
            format!("{}:{}", self.host, self.port).into()
        }
    }
}

impl std::fmt::Display for Endpoint {
    /// Escribe el endpoint en el formato que espera ADB.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.as_argument().to_string_lossy())
    }
}

#[derive(Debug, Error)]
pub enum AdbClientError {
    #[error("invalid ADB endpoint")]
    InvalidEndpoint,
    #[error("ADB runner failed during {operation}: {source}")]
    Runner {
        operation: &'static str,
        #[source]
        source: RunnerError,
    },
    #[error("ADB {operation} failed with status {status:?}: {stderr}")]
    Command {
        operation: &'static str,
        status: Option<i32>,
        stderr: String,
    },
    #[error("could not parse ADB device list: {0}")]
    Parse(#[from] AdbParseError),
}

pub struct AdbClient<R> {
    runner: R,
}

impl<R> AdbClient<R>
where
    R: AdbRunner,
{
    /// Crea un cliente ADB respaldado por el ejecutor indicado.
    pub fn new(runner: R) -> Self {
        Self { runner }
    }

    /// Inicia el servidor ADB y devuelve un error si el comando falla.
    pub fn start_server(&self) -> Result<(), AdbClientError> {
        self.run_success("start-server", &["start-server"])
    }

    /// Detiene el servidor ADB al cerrar la aplicación.
    pub fn kill_server(&self) -> Result<(), AdbClientError> {
        self.run_success("kill-server", &["kill-server"])
    }

    /// Obtiene y analiza la lista detallada de dispositivos conectados.
    pub fn devices(&self) -> Result<Vec<Device>, AdbClientError> {
        let output = self
            .runner
            .run(&[OsString::from("devices"), OsString::from("-l")])
            .map_err(|source| AdbClientError::Runner {
                operation: "devices",
                source,
            })?;
        ensure_success("devices", &output)?;
        parse_devices(&String::from_utf8_lossy(&output.stdout)).map_err(Into::into)
    }

    /// Empareja un dispositivo mediante el endpoint y el código recibidos.
    pub fn pair(&self, endpoint: &Endpoint, pairing_code: &str) -> Result<(), AdbClientError> {
        if pairing_code.trim().is_empty() || pairing_code.chars().any(char::is_whitespace) {
            return Err(AdbClientError::InvalidEndpoint);
        }
        let endpoint = endpoint.as_argument();
        self.run_success(
            "pair",
            &["pair", endpoint.to_string_lossy().as_ref(), pairing_code],
        )
    }

    /// Conecta con un dispositivo ADB mediante su endpoint.
    pub fn connect(&self, endpoint: &Endpoint) -> Result<(), AdbClientError> {
        let endpoint = endpoint.as_argument();
        self.run_success("connect", &["connect", endpoint.to_string_lossy().as_ref()])
    }

    /// Desconecta el dispositivo asociado al endpoint indicado.
    pub fn disconnect(&self, endpoint: &Endpoint) -> Result<(), AdbClientError> {
        let endpoint = endpoint.as_argument();
        self.run_success(
            "disconnect",
            &["disconnect", endpoint.to_string_lossy().as_ref()],
        )
    }

    /// Ejecuta una operación ADB y exige que termine correctamente.
    fn run_success(
        &self,
        operation: &'static str,
        arguments: &[&str],
    ) -> Result<(), AdbClientError> {
        let arguments = arguments.iter().map(OsString::from).collect::<Vec<_>>();
        let output = self
            .runner
            .run(&arguments)
            .map_err(|source| AdbClientError::Runner { operation, source })?;
        ensure_success(operation, &output)
    }
}

/// Convierte un código de salida ADB distinto de cero en un error de dominio.
fn ensure_success(
    operation: &'static str,
    output: &super::AdbOutput,
) -> Result<(), AdbClientError> {
    if output.status_code == Some(0) {
        return Ok(());
    }
    Err(AdbClientError::Command {
        operation,
        status: output.status_code,
        stderr: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;
    use crate::adb::AdbOutput;

    struct FakeRunner {
        calls: Mutex<Vec<Vec<OsString>>>,
        output: AdbOutput,
    }

    impl AdbRunner for FakeRunner {
        /// Registra los argumentos y devuelve la salida simulada configurada.
        fn run(&self, arguments: &[OsString]) -> Result<AdbOutput, RunnerError> {
            self.calls
                .lock()
                .expect("calls lock")
                .push(arguments.to_vec());
            Ok(self.output.clone())
        }
    }

    /// Construye un ejecutor simulado cuya salida representa una operación válida.
    fn successful_runner() -> FakeRunner {
        FakeRunner {
            calls: Mutex::new(Vec::new()),
            output: AdbOutput {
                stdout: b"List of devices attached\nserial\tdevice\n".to_vec(),
                stderr: Vec::new(),
                status_code: Some(0),
            },
        }
    }

    #[test]
    /// Verifica que el pairing transmite cada argumento por separado.
    fn uses_separate_arguments_for_pairing() {
        let runner = successful_runner();
        let client = AdbClient::new(runner);
        let endpoint = Endpoint::new("192.0.2.10", 37099).expect("endpoint");
        client.pair(&endpoint, "123456").expect("pair command");
        let calls = client.runner.calls.lock().expect("calls lock");
        assert_eq!(calls[0], ["pair", "192.0.2.10:37099", "123456"]);
    }

    #[test]
    /// Verifica que los errores de ADB no exponen el código de pairing.
    fn does_not_include_pairing_code_in_command_errors() {
        let runner = FakeRunner {
            calls: Mutex::new(Vec::new()),
            output: AdbOutput {
                stdout: Vec::new(),
                stderr: b"pairing failed".to_vec(),
                status_code: Some(1),
            },
        };
        let client = AdbClient::new(runner);
        let endpoint = Endpoint::new("192.0.2.10", 37099).expect("endpoint");
        let error = client
            .pair(&endpoint, "123456")
            .expect_err("pair should fail");
        assert!(!error.to_string().contains("123456"));
    }

    #[test]
    /// Verifica que los endpoints vacíos o inválidos se rechazan.
    fn rejects_invalid_endpoints() {
        assert!(matches!(
            Endpoint::new("", 5555),
            Err(AdbClientError::InvalidEndpoint)
        ));
        assert!(matches!(
            Endpoint::new("host name", 5555),
            Err(AdbClientError::InvalidEndpoint)
        ));
        assert!(matches!(
            Endpoint::new("host", 0),
            Err(AdbClientError::InvalidEndpoint)
        ));
    }

    #[test]
    fn formats_ipv6_endpoints_without_ambiguity() {
        let endpoint = Endpoint::new("fe80::1", 5555).expect("endpoint");
        assert_eq!(endpoint.to_string(), "[fe80::1]:5555");
    }
}
