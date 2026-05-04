use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthType {
    Bearer,
    ApiKey,
}

impl Default for AuthType {
    fn default() -> Self {
        Self::Bearer
    }
}

impl AuthType {
    pub fn to_str(&self) -> &'static str {
        match self {
            AuthType::Bearer => "bearer",
            AuthType::ApiKey => "api_key",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Anthropic,
    #[serde(rename = "openai")]
    OpenAI,
}

impl Default for Protocol {
    fn default() -> Self {
        Self::Anthropic
    }
}

impl Protocol {
    pub fn to_str(&self) -> &'static str {
        match self {
            Protocol::Anthropic => "anthropic",
            Protocol::OpenAI => "openai",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Provider {
    pub base_url: String,
    pub auth_token: String,
    #[serde(default)]
    pub auth_type: AuthType,
    #[serde(default)]
    pub protocol: Protocol,
}

#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
    #[error("TOML parse error: {0}")]
    Parse(#[from] toml::de::Error),
    #[error("TOML serialize error: {0}")]
    Serialize(#[from] toml::ser::Error),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct ProviderTable {
    #[serde(default)]
    pub providers: HashMap<String, Provider>,
}

impl ProviderTable {
    pub fn from_str(content: &str) -> Result<Self, ProviderError> {
        Ok(toml::from_str(content)?)
    }
}

pub fn load_providers(path: &Path) -> Result<ProviderTable, ProviderError> {
    let content = std::fs::read_to_string(path)?;
    let table: ProviderTable = toml::from_str(&content)?;
    Ok(table)
}

pub fn save_providers(path: &Path, table: &ProviderTable) -> Result<(), ProviderError> {
    let content = toml::to_string_pretty(table)?;
    let tmp_path = path.with_extension("toml.tmp");
    std::fs::write(&tmp_path, &content)?;
    std::fs::rename(&tmp_path, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_toml() -> &'static str {
        r#"
[providers.kimi]
base_url = "https://api.kimi.ai"
auth_token = "sk-test-123"
auth_type = "bearer"
protocol = "anthropic"

[providers.zhipu]
base_url = "https://open.bigmodel.cn/api/paas"
auth_token = "zp-test-456"
auth_type = "api_key"
protocol = "openai"
"#
    }

    #[test]
    fn t6_parse_provider_toml() {
        let table = ProviderTable::from_str(sample_toml()).unwrap();
        let kimi = table.providers.get("kimi").unwrap();
        assert_eq!(kimi.base_url, "https://api.kimi.ai");
        assert_eq!(kimi.auth_token, "sk-test-123");
        assert_eq!(kimi.auth_type, AuthType::Bearer);
        assert_eq!(kimi.protocol, Protocol::Anthropic);

        let zhipu = table.providers.get("zhipu").unwrap();
        assert_eq!(zhipu.auth_type, AuthType::ApiKey);
        assert_eq!(zhipu.protocol, Protocol::OpenAI);
    }

    #[test]
    fn t7_round_trip() {
        let mut table = ProviderTable::default();
        table.providers.insert(
            "test".to_string(),
            Provider {
                base_url: "https://api.test.com".to_string(),
                auth_token: "tok".to_string(),
                auth_type: AuthType::Bearer,
                protocol: Protocol::Anthropic,
            },
        );
        let serialized = toml::to_string_pretty(&table).unwrap();
        let deserialized: ProviderTable = toml::from_str(&serialized).unwrap();
        assert_eq!(table, deserialized);
    }

    #[test]
    fn t8_atomic_write() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("providers.toml");

        let mut table = ProviderTable::default();
        table.providers.insert(
            "p1".to_string(),
            Provider {
                base_url: "https://a.com".to_string(),
                auth_token: "t1".to_string(),
                auth_type: AuthType::Bearer,
                protocol: Protocol::Anthropic,
            },
        );
        save_providers(&path, &table).unwrap();

        let mut table2 = ProviderTable::default();
        table2.providers.insert(
            "p2".to_string(),
            Provider {
                base_url: "https://b.com".to_string(),
                auth_token: "t2".to_string(),
                auth_type: AuthType::ApiKey,
                protocol: Protocol::OpenAI,
            },
        );
        save_providers(&path, &table2).unwrap();

        let loaded = load_providers(&path).unwrap();
        assert_eq!(loaded.providers.len(), 1);
        assert!(loaded.providers.contains_key("p2"));
    }
}
