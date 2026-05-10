use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::mapping::ModelMapping;
use crate::pricing::Pricing;
use crate::provider::{AuthType, Protocol};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelInfo {
    pub id: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct QuotaAdapter {
    pub quota_api_url: String,
    #[serde(default)]
    pub auth_style: String,
    #[serde(default)]
    pub extra_headers: HashMap<String, String>,
    pub response_mapping: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Preset {
    pub name: String,
    pub base_url: String,
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
    pub suggested_mappings: Vec<ModelMapping>,
    #[serde(default)]
    pub quota_adapter: Option<QuotaAdapter>,
}
