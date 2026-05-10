use std::sync::Arc;

use ecc_domain::preset::Preset;
use ecc_domain::repository::{PresetRepository, RepositoryError};

pub struct PresetService<R: PresetRepository> {
    repo: Arc<R>,
}

impl<R: PresetRepository> PresetService<R> {
    pub fn new(repo: Arc<R>) -> Self {
        Self { repo }
    }

    pub fn list_presets(&self) -> Result<Vec<Preset>, RepositoryError> {
        self.repo.list_presets()
    }

    pub fn get_preset(&self, name: &str) -> Result<Option<Preset>, RepositoryError> {
        self.repo.get_preset(name)
    }

    pub fn create_preset(&self, preset: Preset) -> Result<Preset, RepositoryError> {
        if preset.name.is_empty() {
            return Err(RepositoryError::NotFound("preset name is empty".into()));
        }
        if self.repo.get_preset(&preset.name)?.is_some() {
            return Err(RepositoryError::NotFound(format!(
                "preset '{}' already exists",
                preset.name
            )));
        }
        self.repo.save_preset(&preset)?;
        Ok(preset)
    }

    pub fn update_preset(&self, name: &str, preset: Preset) -> Result<Preset, RepositoryError> {
        if self.repo.get_preset(name)?.is_none() {
            return Err(RepositoryError::NotFound(format!("preset '{name}' not found")));
        }
        self.repo.save_preset(&preset)?;
        Ok(preset)
    }

    pub fn delete_preset(&self, name: &str) -> Result<(), RepositoryError> {
        self.repo.delete_preset(name)
    }
}
