use crate::provider::Provider;

/// Provider 仓库 trait — 基础设施层实现。
///
/// 定义了 Provider 聚合根的持久化接口。
/// 实现可以是 SQLite、TOML 文件、内存等。
pub trait ProviderRepository: Send + Sync {
    /// 获取所有 Provider
    fn list(&self) -> Result<Vec<Provider>, RepositoryError>;

    /// 按 name 查找 Provider
    fn get(&self, name: &str) -> Result<Option<Provider>, RepositoryError>;

    /// 保存 Provider（创建或更新）
    fn save(&self, provider: &Provider) -> Result<(), RepositoryError>;

    /// 删除 Provider
    fn delete(&self, name: &str) -> Result<(), RepositoryError>;
}

/// 系统配置仓库 trait
pub trait ConfigRepository: Send + Sync {
    /// 获取默认 Provider 名称
    fn get_default_provider(&self) -> Result<Option<String>, RepositoryError>;

    /// 设置默认 Provider
    fn set_default_provider(&self, name: &str) -> Result<(), RepositoryError>;
}

/// 用量记录
#[derive(Debug, Clone)]
pub struct UsageRecord {
    pub id: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
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

/// 用量仓库 trait
pub trait UsageRepository: Send + Sync {
    /// 记录一次请求用量
    fn record(&self, record: UsageRecord) -> Result<(), RepositoryError>;

    /// 查询指定日期范围的用量记录
    fn query(&self, start: &chrono::DateTime<chrono::Utc>, end: &chrono::DateTime<chrono::Utc>) -> Result<Vec<UsageRecord>, RepositoryError>;

    /// 按 provider 聚合用量
    fn aggregate_by_provider(&self, start: &chrono::DateTime<chrono::Utc>, end: &chrono::DateTime<chrono::Utc>) -> Result<Vec<ProviderUsage>, RepositoryError>;
}

/// 按供应商聚合的用量统计
#[derive(Debug, Clone)]
pub struct ProviderUsage {
    pub provider_name: String,
    pub total_requests: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cache_read_tokens: u64,
    pub total_cost_usd: f64,
}

/// 配额信息
#[derive(Debug, Clone)]
pub struct QuotaInfo {
    pub provider_name: String,
    pub tiers: Vec<QuotaTier>,
}

/// 配额层级
#[derive(Debug, Clone)]
pub struct QuotaTier {
    pub name: String,
    pub utilization: f64,
    pub resets_at: Option<String>,
}

/// 仓库错误
#[derive(Debug, thiserror::Error)]
pub enum RepositoryError {
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Storage error: {0}")]
    Storage(#[from] Box<dyn std::error::Error + Send + Sync>),
}
