use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::mapping::RouteTarget;
use crate::preset::Preset;
use crate::provider::Provider;

#[derive(Debug, Error)]
pub enum RepositoryError {
    #[error("not found: {0}")]
    NotFound(String),
    #[error("storage error: {0}")]
    Storage(#[from] Box<dyn std::error::Error + Send + Sync>),
}

pub trait ProviderRepository: Send + Sync {
    fn list(&self) -> Result<Vec<Provider>, RepositoryError>;
    fn get(&self, name: &str) -> Result<Option<Provider>, RepositoryError>;
    fn save(&self, provider: &Provider) -> Result<(), RepositoryError>;
    fn delete(&self, name: &str) -> Result<(), RepositoryError>;
}

pub trait ConfigRepository: Send + Sync {
    fn get_default_provider(&self) -> Result<Option<String>, RepositoryError>;
    fn set_default_provider(&self, name: &str) -> Result<(), RepositoryError>;
}

pub trait PresetRepository: Send + Sync {
    fn list_presets(&self) -> Result<Vec<Preset>, RepositoryError>;
    fn get_preset(&self, name: &str) -> Result<Option<Preset>, RepositoryError>;
    fn save_preset(&self, preset: &Preset) -> Result<(), RepositoryError>;
    fn delete_preset(&self, name: &str) -> Result<(), RepositoryError>;
    fn is_presets_empty(&self) -> Result<bool, RepositoryError>;
    fn seed_presets(&self, presets: &[Preset]) -> Result<(), RepositoryError>;
}

pub trait RouteRepository: Send + Sync {
    fn get_routes(&self, claude_model: &str) -> Result<Option<Vec<RouteTarget>>, RepositoryError>;
    fn list_routes(&self) -> Result<HashMap<String, Vec<RouteTarget>>, RepositoryError>;
    fn rebuild(&self) -> Result<(), RepositoryError>;
}

pub trait UsageRepository: Send + Sync {
    fn record(&self, record: UsageRecord) -> Result<(), RepositoryError>;
    fn query(
        &self,
        start: &DateTime<Utc>,
        end: &DateTime<Utc>,
    ) -> Result<Vec<UsageRecord>, RepositoryError>;
    fn aggregate_by_provider(
        &self,
        start: &DateTime<Utc>,
        end: &DateTime<Utc>,
    ) -> Result<Vec<ProviderUsage>, RepositoryError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageRecord {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub provider_name: String,
    pub target_model: String,
    pub requested_model: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cost_usd: f64,
    pub latency_ms: u64,
    pub status: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderUsage {
    pub provider_name: String,
    pub total_requests: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cache_read_tokens: u64,
    pub total_cost_usd: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaInfo {
    pub provider_name: String,
    pub success: bool,
    pub tiers: Vec<QuotaTier>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaTier {
    pub name: String,
    pub utilization: f64,
    pub resets_at: Option<String>,
}

// --- Session ---

pub trait SessionRepository: Send + Sync {
    fn record(&self, record: SessionRecord) -> Result<(), RepositoryError>;
    fn list_sessions(&self, limit: u64) -> Result<Vec<SessionSummary>, RepositoryError>;
    fn get_session(&self, session_id: &str) -> Result<Vec<SessionRecord>, RepositoryError>;
    fn delete_session(&self, session_id: &str) -> Result<(), RepositoryError>;
    /// Find the most recent session_id whose first record matches the base hash prefix.
    /// Returns (session_id, last_timestamp) if found.
    fn find_latest_by_prefix(&self, base_hash: &str) -> Result<Option<(String, DateTime<Utc>)>, RepositoryError>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub id: String,
    pub session_id: String,
    pub timestamp: DateTime<Utc>,
    pub provider_name: String,
    pub target_model: String,
    pub requested_model: String,
    pub request_body: String,
    pub response_body: String,
    pub assistant_text: String,
    pub thinking_text: String,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub latency_ms: u64,
    pub status: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub session_id: String,
    pub first_timestamp: DateTime<Utc>,
    pub last_timestamp: DateTime<Utc>,
    pub total_turns: u64,
    pub provider_name: String,
    pub requested_model: String,
}
