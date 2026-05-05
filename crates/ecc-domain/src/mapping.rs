use serde::{Deserialize, Serialize};

/// 模型映射 — 属于 Provider 聚合根。
///
/// 定义了「Claude 模型 → 该供应商的模型」的映射关系。
/// 只有映射了的 Claude 模型才能通过该 Provider 转发。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelMapping {
    pub claude_model: String,
    pub provider_model: String,
}

/// 路由解析结果。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RouteTarget {
    pub provider_name: String,
    pub provider_model: String,
}
