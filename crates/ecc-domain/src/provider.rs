use std::collections::HashMap;

use serde::{Deserialize, Serialize};

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
}

/// Provider — ecc 的聚合根。
///
/// 所有业务围绕 Provider 展开：配置、映射、转发、用量统计。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Provider {
    pub name: String,
    pub base_url: String,
    #[serde(default)]
    pub alt_base_urls: HashMap<String, String>,
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
    pub preset_name: Option<String>,
}

impl Provider {
    /// 根据当前协议返回实际应该使用的 base_url
    pub fn effective_base_url(&self) -> &str {
        self.alt_base_urls
            .get(self.protocol.to_str())
            .unwrap_or(&self.base_url)
    }

    /// 查找某个 claude_model 的映射
    pub fn find_mapping(&self, claude_model: &str) -> Option<&ModelMapping> {
        self.model_mappings.iter().find(|m| m.claude_model == claude_model)
    }

    /// 查找某个 provider_model 的定价
    pub fn find_pricing(&self, model: &str) -> Option<&Pricing> {
        self.pricing.get(model)
    }

    /// 计算一次请求的费用
    pub fn calculate_cost(&self, provider_model: &str, input: u64, cache_read: u64, output: u64) -> f64 {
        match self.find_pricing(provider_model) {
            Some(p) => p.calculate(input, cache_read, output),
            None => 0.0,
        }
    }
}
