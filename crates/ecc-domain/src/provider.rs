use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::mapping::ModelMapping;
use crate::pricing::Pricing;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

    pub fn from_str(s: &str) -> Self {
        match s {
            "api_key" => AuthType::ApiKey,
            _ => AuthType::Bearer,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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

    pub fn from_str(s: &str) -> Self {
        match s {
            "openai" => Protocol::OpenAI,
            _ => Protocol::Anthropic,
        }
    }
}

/// Provider — ecc 的聚合根。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Provider {
    pub name: String,
    pub base_url: String,
    pub auth_token: String,
    #[serde(default)]
    pub auth_type: AuthType,
    #[serde(default)]
    pub protocol: Protocol,
    #[serde(default)]
    pub is_coding_plan: bool,
    #[serde(default)]
    pub model_mappings: Vec<ModelMapping>,
    #[serde(default)]
    pub pricing: HashMap<String, Pricing>,
    #[serde(default)]
    pub quota_adapter: Option<crate::preset::QuotaAdapter>,
}

impl Provider {
    pub fn find_mapping(&self, claude_model: &str) -> Option<&ModelMapping> {
        self.model_mappings
            .iter()
            .find(|m| m.claude_model == claude_model)
    }

    pub fn find_pricing(&self, provider_model: &str) -> Option<&Pricing> {
        self.pricing.get(provider_model)
    }

    pub fn calculate_cost(
        &self,
        provider_model: &str,
        input: u64,
        cache_read: u64,
        output: u64,
    ) -> f64 {
        match self.find_pricing(provider_model) {
            Some(p) => p.calculate(input, cache_read, output),
            None => 0.0,
        }
    }
}
