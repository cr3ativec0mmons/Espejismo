use thiserror::Error;

use crate::adb::{AdbClient, AdbClientError, AdbRunner, Endpoint};
use crate::config::{ConfigError, ConfigRepository};
use crate::model::{AppState, Device, DeviceId, SavedDevice, Settings, Transport};
use crate::scrcpy::{ScrcpyError, ScrcpyLauncher, ScrcpyOptions};

#[derive(Debug, Error)]
pub enum AppError {
    #[error("configuration error: {0}")]
    Config(#[from] ConfigError),
    #[error("ADB error: {0}")]
    Adb(#[from] AdbClientError),
    #[error("scrcpy error: {0}")]
    Scrcpy(#[from] ScrcpyError),
    #[error("device {0} was not found after connecting")]
    DeviceNotFound(DeviceId),
}

pub struct AppController<R> {
    repository: ConfigRepository,
    adb: AdbClient<R>,
    scrcpy: ScrcpyLauncher,
    settings: Settings,
    devices: Vec<Device>,
    state: AppState,
}

impl<R> AppController<R>
where
    R: AdbRunner,
{
    /// Carga la configuración y crea el controlador en estado de carga.
    pub fn new(
        repository: ConfigRepository,
        adb: AdbClient<R>,
        scrcpy: ScrcpyLauncher,
    ) -> Result<Self, AppError> {
        let settings = repository.load()?;
        Ok(Self {
            repository,
            adb,
            scrcpy,
            settings,
            devices: Vec::new(),
            state: AppState::LoadingDevices,
        })
    }

    /// Devuelve el estado actual de la aplicación.
    pub fn state(&self) -> AppState {
        self.state
    }

    /// Devuelve la lista de dispositivos observados sin copiarla.
    pub fn devices(&self) -> &[Device] {
        &self.devices
    }

    /// Devuelve la configuración cargada sin copiarla.
    pub fn settings(&self) -> &Settings {
        &self.settings
    }

    /// Inicia ADB, actualiza la lista de dispositivos y ajusta el estado resultante.
    pub fn refresh(&mut self) -> Result<&[Device], AppError> {
        self.state = AppState::LoadingDevices;
        let result = self.adb.start_server().and_then(|()| self.adb.devices());
        match result {
            Ok(devices) => {
                self.state = if devices.is_empty() {
                    AppState::Disconnected
                } else {
                    AppState::DeviceAvailable
                };
                self.devices = devices;
                Ok(&self.devices)
            }
            Err(error) => {
                self.state = AppState::Error;
                Err(error.into())
            }
        }
    }

    /// Empareja, conecta y persiste un dispositivo inalámbrico válido.
    pub fn pair_and_connect(
        &mut self,
        pairing_endpoint: &Endpoint,
        pairing_code: &str,
        adb_endpoint: &Endpoint,
    ) -> Result<Device, AppError> {
        self.state = AppState::Pairing;
        self.adb.pair(pairing_endpoint, pairing_code)?;
        self.state = AppState::Connecting;
        self.adb.connect(adb_endpoint)?;
        let device_id = DeviceId::new(adb_endpoint.to_string()).expect("validated endpoint");
        let device = self
            .adb
            .devices()?
            .into_iter()
            .find(|device| device.id == device_id && device.is_usable())
            .ok_or_else(|| AppError::DeviceNotFound(device_id.clone()))?;
        self.save_device(&device)?;
        self.devices = self.adb.devices()?;
        self.state = AppState::Connected;
        Ok(device)
    }

    /// Lanza scrcpy con el override del dispositivo o la configuración global.
    pub fn launch_scrcpy(&mut self, device_id: &DeviceId) -> Result<std::process::Child, AppError> {
        let device = self
            .devices
            .iter()
            .find(|device| &device.id == device_id && device.is_usable())
            .ok_or_else(|| AppError::DeviceNotFound(device_id.clone()))?;
        let settings = self
            .settings
            .device_overrides
            .get(&device.id)
            .unwrap_or(&self.settings.scrcpy);
        let options = ScrcpyOptions::try_from(settings)?;
        self.state = AppState::LaunchingScrcpy;
        let child = self.scrcpy.launch(&device.id, &options)?;
        self.state = AppState::Streaming;
        Ok(child)
    }

    /// Guarda o actualiza un dispositivo conectado en la configuración.
    fn save_device(&mut self, device: &Device) -> Result<(), AppError> {
        let saved = SavedDevice {
            id: device.id.clone(),
            alias: device.alias.clone(),
            transport: Transport::Wifi,
        };
        if let Some(existing) = self
            .settings
            .devices
            .iter_mut()
            .find(|saved| saved.id == device.id)
        {
            *existing = saved;
        } else {
            self.settings.devices.push(saved);
        }
        self.repository.save(&self.settings)?;
        Ok(())
    }
}
