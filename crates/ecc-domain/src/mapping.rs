use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelMapping {
    pub claude_model: String,
    pub provider_model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RouteTarget {
    pub provider_name: String,
    pub provider_model: String,
}
