use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use thiserror::Error;

use crate::model::Settings;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("could not determine the configuration directory")]
    DirectoryUnavailable,
    #[error("could not read configuration: {0}")]
    Read(#[source] io::Error),
    #[error("configuration is invalid: {0}")]
    Parse(#[source] serde_json::Error),
    #[error("could not write configuration: {0}")]
    Write(#[source] io::Error),
    #[error("could not serialize configuration: {0}")]
    Serialize(#[source] serde_json::Error),
}

#[derive(Debug, Clone)]
pub struct ConfigRepository {
    path: PathBuf,
}

impl ConfigRepository {
    /// Descubre la ruta de configuración del usuario mediante las rutas del sistema.
    pub fn discover() -> Result<Self, ConfigError> {
        let dirs = ProjectDirs::from("com", "Espejismo", "Espejismo")
            .ok_or(ConfigError::DirectoryUnavailable)?;
        Ok(Self::at(dirs.config_dir().join("config.json")))
    }

    /// Crea un repositorio que utiliza la ruta indicada.
    pub fn at(path: impl Into<PathBuf>) -> Self {
        Self { path: path.into() }
    }

    /// Devuelve la ruta del archivo de configuración.
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Carga la configuración o devuelve los valores predeterminados si no existe.
    pub fn load(&self) -> Result<Settings, ConfigError> {
        match fs::read_to_string(&self.path) {
            Ok(contents) => serde_json::from_str(&contents).map_err(ConfigError::Parse),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(Settings::default()),
            Err(error) => Err(ConfigError::Read(error)),
        }
    }

    /// Carga la configuración y devuelve valores predeterminados junto con el error si falla.
    pub fn load_or_default(&self) -> (Settings, Option<ConfigError>) {
        match self.load() {
            Ok(settings) => (settings, None),
            Err(error) => (Settings::default(), Some(error)),
        }
    }

    /// Serializa y guarda la configuración mediante un reemplazo atómico.
    pub fn save(&self, settings: &Settings) -> Result<(), ConfigError> {
        let directory = self
            .path
            .parent()
            .ok_or_else(|| io::Error::other("configuration path has no parent"))
            .map_err(ConfigError::Write)?;
        fs::create_dir_all(directory).map_err(ConfigError::Write)?;

        let contents = serde_json::to_string_pretty(settings).map_err(ConfigError::Serialize)?;
        let temporary = directory.join(format!(".config.json.{}.tmp", std::process::id()));
        fs::write(&temporary, contents).map_err(ConfigError::Write)?;
        fs::rename(&temporary, &self.path).map_err(ConfigError::Write)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    /// Comprueba que un archivo inexistente produce la configuración predeterminada.
    fn missing_file_returns_defaults() {
        let path =
            std::env::temp_dir().join(format!("espejismo-missing-{}.json", std::process::id()));
        let repository = ConfigRepository::at(path);
        assert_eq!(
            repository.load().expect("missing config should default"),
            Settings::default()
        );
    }

    #[test]
    /// Comprueba que guardar y cargar conserva la configuración.
    fn save_and_load_round_trip() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let repository = ConfigRepository::at(directory.path().join("nested/config.json"));
        let settings = Settings::default();
        repository.save(&settings).expect("save settings");
        assert_eq!(repository.load().expect("load settings"), settings);
    }

    #[test]
    /// Comprueba que una configuración corrupta puede sustituirse por valores predeterminados.
    fn corrupt_file_can_fall_back_to_defaults() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("config.json");
        std::fs::write(&path, "not json").expect("corrupt config");
        let repository = ConfigRepository::at(path);
        let (settings, error) = repository.load_or_default();
        assert_eq!(settings, Settings::default());
        assert!(matches!(error, Some(ConfigError::Parse(_))));
    }
}
