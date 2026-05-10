use ecc_domain::preset::Preset;
use ecc_domain::repository::{PresetRepository, RepositoryError};

const BUILTIN_PRESETS_JSON: &str = include_str!("presets.json");

pub fn seed_if_empty(repo: &dyn PresetRepository) -> Result<usize, RepositoryError> {
    if !repo.is_presets_empty()? {
        return Ok(0);
    }
    let presets: Vec<Preset> = serde_json::from_str(BUILTIN_PRESETS_JSON)
        .map_err(|e| RepositoryError::Storage(e.into()))?;
    let count = presets.len();
    repo.seed_presets(&presets)?;
    Ok(count)
}
