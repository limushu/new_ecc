//! ecc-domain — 纯数据模型 + Repository trait（零外部依赖）。
//!
//! 定义 Provider 聚合根、Preset 模型、值对象和持久化契约。
//! 不依赖任何其他 crate，是六边形架构的核心。

pub mod provider;
pub mod preset;
pub mod mapping;
pub mod pricing;
pub mod repository;

pub use provider::{AuthType, Protocol, Provider};
pub use preset::{ModelInfo, Preset, QuotaAdapter};
pub use mapping::{ModelMapping, RouteTarget};
pub use pricing::Pricing;
pub use repository::{
    ConfigRepository, PresetRepository, ProviderRepository, ProviderUsage, QuotaInfo,
    QuotaTier, RepositoryError, RouteRepository, UsageRecord, UsageRepository,
};
