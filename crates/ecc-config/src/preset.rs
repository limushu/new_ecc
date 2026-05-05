use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::pricing::Pricing;
use crate::provider::{AuthType, Protocol};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
}

/// Suggested Claude model → provider model mapping.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SuggestedMapping {
    pub claude_model: String,
    pub provider_models: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Preset {
    pub name: String,
    pub base_url: String,
    /// Alternative base URLs keyed by protocol name ("openai", "anthropic").
    /// When a template is selected and the user switches protocol, the matching
    /// URL here overrides `base_url`. E.g. DeepSeek uses different URLs for
    /// OpenAI vs Anthropic protocol.
    #[serde(default)]
    pub alt_base_urls: HashMap<String, String>,
    #[serde(default)]
    pub protocol: Protocol,
    #[serde(default)]
    pub auth_type: AuthType,
    #[serde(default)]
    pub models: Vec<ModelInfo>,
    #[serde(default)]
    pub pricing: HashMap<String, Pricing>,
    #[serde(default)]
    pub suggested_mappings: Vec<SuggestedMapping>,
}

#[derive(Debug, thiserror::Error)]
pub enum PresetError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("TOML parse error: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("Preset not found: {0}")]
    NotFound(String),
}

/// Load built-in presets compiled into the binary
pub fn list_builtin_presets() -> Vec<Preset> {
    let mut presets = Vec::new();

    // ── DeepSeek ──
    let mut ds_pricing = HashMap::new();
    ds_pricing.insert("deepseek-v4-flash".to_string(), Pricing { input_per_m: 0.28, output_per_m: 0.42, cache_read_per_m: Some(0.028) });
    ds_pricing.insert("deepseek-v4-pro".to_string(), Pricing { input_per_m: 0.55, output_per_m: 2.19, cache_read_per_m: Some(0.138) });
    ds_pricing.insert("deepseek-chat".to_string(), Pricing { input_per_m: 0.28, output_per_m: 0.42, cache_read_per_m: Some(0.028) });
    ds_pricing.insert("deepseek-reasoner".to_string(), Pricing { input_per_m: 0.55, output_per_m: 2.19, cache_read_per_m: Some(0.138) });
    let mut ds_alt_urls = HashMap::new();
    ds_alt_urls.insert("openai".to_string(), "https://api.deepseek.com".to_string());
    ds_alt_urls.insert("anthropic".to_string(), "https://api.deepseek.com/anthropic".to_string());
    presets.push(Preset {
        name: "DeepSeek".to_string(),
        base_url: "https://api.deepseek.com".to_string(),
        alt_base_urls: ds_alt_urls,
        protocol: Protocol::OpenAI,
        auth_type: AuthType::Bearer,
        models: vec![
            ModelInfo { id: "deepseek-v4-flash".to_string(), name: "DeepSeek V4 Flash".to_string() },
            ModelInfo { id: "deepseek-v4-pro".to_string(), name: "DeepSeek V4 Pro".to_string() },
            ModelInfo { id: "deepseek-chat".to_string(), name: "DeepSeek Chat (legacy)".to_string() },
            ModelInfo { id: "deepseek-reasoner".to_string(), name: "DeepSeek Reasoner (legacy)".to_string() },
        ],
        pricing: ds_pricing,
        suggested_mappings: vec![
            SuggestedMapping { claude_model: "claude-sonnet-4-6".into(), provider_models: vec!["deepseek-v4-flash".into()] },
            SuggestedMapping { claude_model: "claude-opus-4-7".into(), provider_models: vec!["deepseek-v4-pro".into(), "deepseek-v4-flash".into()] },
            SuggestedMapping { claude_model: "claude-haiku-4-5".into(), provider_models: vec!["deepseek-v4-flash".into()] },
        ],
    });

    // ── Kimi (月之暗面) ──
    let mut kimi_pricing = HashMap::new();
    kimi_pricing.insert("kimi-for-coding".to_string(), Pricing { input_per_m: 0.60, output_per_m: 2.50, cache_read_per_m: None });
    presets.push(Preset {
        name: "Kimi".to_string(),
        base_url: "https://api.kimi.com/coding/".to_string(),
        alt_base_urls: HashMap::new(),
        protocol: Protocol::Anthropic,
        auth_type: AuthType::Bearer,
        models: vec![
            ModelInfo { id: "kimi-for-coding".to_string(), name: "Kimi Code (K2.6)".to_string() },
        ],
        pricing: kimi_pricing,
        suggested_mappings: vec![
            SuggestedMapping { claude_model: "claude-sonnet-4-6".into(), provider_models: vec!["kimi-for-coding".into()] },
            SuggestedMapping { claude_model: "claude-opus-4-7".into(), provider_models: vec!["kimi-for-coding".into()] },
            SuggestedMapping { claude_model: "claude-haiku-4-5".into(), provider_models: vec!["kimi-for-coding".into()] },
        ],
    });

    // ── 智谱 (GLM) ──
    let mut glm_pricing = HashMap::new();
    glm_pricing.insert("glm-5-turbo".to_string(), Pricing { input_per_m: 0.70, output_per_m: 3.10, cache_read_per_m: None });
    glm_pricing.insert("glm-4.6".to_string(), Pricing { input_per_m: 0.70, output_per_m: 3.10, cache_read_per_m: None });
    glm_pricing.insert("glm-4-flash".to_string(), Pricing { input_per_m: 0.0, output_per_m: 0.0, cache_read_per_m: None });
    let mut glm_alt_urls = HashMap::new();
    glm_alt_urls.insert("openai".to_string(), "https://open.bigmodel.cn/api/paas/v4".to_string());
    glm_alt_urls.insert("anthropic".to_string(), "https://open.bigmodel.cn/api/anthropic".to_string());
    presets.push(Preset {
        name: "GLM".to_string(),
        base_url: "https://open.bigmodel.cn/api/anthropic".to_string(),
        alt_base_urls: glm_alt_urls,
        protocol: Protocol::Anthropic,
        auth_type: AuthType::Bearer,
        models: vec![
            ModelInfo { id: "glm-5-turbo".to_string(), name: "GLM-5 Turbo".to_string() },
            ModelInfo { id: "glm-4.6".to_string(), name: "GLM-4.6".to_string() },
            ModelInfo { id: "glm-4-flash".to_string(), name: "GLM-4 Flash".to_string() },
        ],
        pricing: glm_pricing,
        suggested_mappings: vec![
            SuggestedMapping { claude_model: "claude-sonnet-4-6".into(), provider_models: vec!["glm-4.6".into(), "glm-5-turbo".into()] },
            SuggestedMapping { claude_model: "claude-opus-4-7".into(), provider_models: vec!["glm-5-turbo".into(), "glm-4.6".into()] },
            SuggestedMapping { claude_model: "claude-haiku-4-5".into(), provider_models: vec!["glm-4-flash".into(), "glm-4.6".into()] },
        ],
    });

    presets
}

/// Find a built-in preset by name (case-insensitive)
pub fn get_builtin_preset(name: &str) -> Option<Preset> {
    let lower = name.to_lowercase();
    list_builtin_presets().into_iter().find(|p| p.name.to_lowercase() == lower)
}

/// Load user presets from a directory of TOML files
pub fn load_user_presets(dir: &Path) -> Result<Vec<Preset>, PresetError> {
    let mut presets = Vec::new();
    if !dir.exists() {
        return Ok(presets);
    }
    for entry in std::fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.extension().map(|e| e == "toml").unwrap_or(false) {
            let content = std::fs::read_to_string(&path)?;
            let preset: Preset = toml::from_str(&content)?;
            presets.push(preset);
        }
    }
    Ok(presets)
}

/// Save a user preset to a TOML file
pub fn save_user_preset(dir: &Path, preset: &Preset) -> Result<(), PresetError> {
    std::fs::create_dir_all(dir)?;
    let filename = preset.name.to_lowercase().replace(' ', "-");
    let path = dir.join(format!("{}.toml", filename));
    let content = toml::to_string_pretty(preset).map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    std::fs::write(&path, content)?;
    Ok(())
}

/// List all presets (built-in + user overrides merged)
/// User presets with the same name override built-in ones
pub fn list_all_presets(user_dir: &Path) -> Result<Vec<Preset>, PresetError> {
    let mut presets = list_builtin_presets();
    let user_presets = load_user_presets(user_dir)?;
    for user_preset in user_presets {
        if let Some(existing) = presets.iter_mut().find(|p| p.name.to_lowercase() == user_preset.name.to_lowercase()) {
            *existing = user_preset;
        } else {
            presets.push(user_preset);
        }
    }
    Ok(presets)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t9_builtin_presets_contain_deepseek() {
        let presets = list_builtin_presets();
        let names: Vec<&str> = presets.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"DeepSeek"), "built-in presets should contain DeepSeek");
    }

    #[test]
    fn t9_builtin_presets_contain_kimi_and_glm() {
        let presets = list_builtin_presets();
        let names: Vec<&str> = presets.iter().map(|p| p.name.as_str()).collect();
        assert!(names.contains(&"Kimi"), "built-in presets should contain Kimi");
        assert!(names.contains(&"GLM"), "built-in presets should contain GLM");
    }

    #[test]
    fn t10_preset_has_models_and_pricing() {
        let preset = get_builtin_preset("deepseek").unwrap();
        assert!(!preset.models.is_empty(), "preset should have models");
        assert!(!preset.pricing.is_empty(), "preset should have pricing");
        // Verify latest DeepSeek pricing
        let chat_price = preset.pricing.get("deepseek-chat").unwrap();
        assert_eq!(chat_price.input_per_m, 0.28);
        assert_eq!(chat_price.output_per_m, 0.42);
        // Verify suggested mappings
        assert!(!preset.suggested_mappings.is_empty(), "preset should have suggested mappings");
    }

    #[test]
    fn t11_user_override_precedence() {
        let dir = tempfile::tempdir().unwrap();
        let user_dir = dir.path().join("presets");

        // Save a user override for DeepSeek with custom URL
        let custom = Preset {
            name: "DeepSeek".to_string(),
            base_url: "https://custom.deepseek.proxy".to_string(),
            alt_base_urls: HashMap::new(),
            protocol: Protocol::OpenAI,
            auth_type: AuthType::Bearer,
            models: vec![ModelInfo { id: "deepseek-chat".to_string(), name: "Custom DS".to_string() }],
            pricing: HashMap::new(),
            suggested_mappings: vec![],
        };
        save_user_preset(&user_dir, &custom).unwrap();

        let all = list_all_presets(&user_dir).unwrap();
        let ds = all.iter().find(|p| p.name == "DeepSeek").unwrap();
        assert_eq!(ds.base_url, "https://custom.deepseek.proxy");
        assert_eq!(ds.models[0].name, "Custom DS");
    }

    #[test]
    fn t11_user_adds_new_preset() {
        let dir = tempfile::tempdir().unwrap();
        let user_dir = dir.path().join("presets");

        let custom = Preset {
            name: "MyProvider".to_string(),
            base_url: "https://my.provider.com".to_string(),
            alt_base_urls: HashMap::new(),
            protocol: Protocol::Anthropic,
            auth_type: AuthType::Bearer,
            models: vec![],
            pricing: HashMap::new(),
            suggested_mappings: vec![],
        };
        save_user_preset(&user_dir, &custom).unwrap();

        let all = list_all_presets(&user_dir).unwrap();
        assert!(all.iter().any(|p| p.name == "MyProvider"));
    }

    #[test]
    fn t11_no_user_dir_returns_builtins() {
        let all = list_all_presets(Path::new("/nonexistent/path")).unwrap();
        assert!(all.len() >= 3); // at least deepseek, kimi, glm
    }
}
