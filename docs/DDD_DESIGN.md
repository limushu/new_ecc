# ecc v2 后端推倒重建 — 详细设计

## Context

新分支 `refactor/ddd-v2` 上推倒重建后端，按 issue #9 需求地图 + issue #10 设计决策执行。

### 设计决策速查

| 决策 | 选择 |
|------|------|
| Quota 适配 | JSON 字段映射，配置驱动 |
| 路由 | 派生路由表，从 Provider mappings 自动生成 |
| Preset | 一等公民，存数据库，完整 CRUD |
| 代码复用 | 协议转换 + 加密搬过来，其余重写 |
| Engine 管道 | Middleware trait + Pipeline 模式 |
| DB Seed | JSON 文件 → 首次启动写入 presets 表 |
| API 风格 | 保持当前 REST 结构 |

---

## Crate 依赖关系

```
ecc-api ──→ ecc-app ──→ ecc-domain ←── ecc-infra
   │                      ↑                │
   └────→ ecc-engine ────┘                │
              ↑                           │
              └───────────────────────────┘ (engine 直接引用 domain)
```

- ecc-domain: 零外部依赖（仅 serde + chrono）
- ecc-app: 依赖 ecc-domain（通过 trait 使用 infra，不直接依赖 infra）
- ecc-infra: 依赖 ecc-domain（实现 trait）
- ecc-engine: 依赖 ecc-domain（读模型数据）
- ecc-api: 依赖 ecc-app + ecc-engine
- **app 和 engine 平级，互不依赖**

---

## Crate 1: ecc-domain — 纯数据模型 + Repository trait

### Cargo.toml

```toml
[dependencies]
serde = { workspace = true }
chrono = { workspace = true }
thiserror = { workspace = true }
```

### 模块结构

```
ecc-domain/src/
  lib.rs           — 模块声明 + re-export 所有公开类型
  provider.rs      — Provider 聚合根 + AuthType + Protocol
  preset.rs        — Preset 模型 + QuotaAdapter
  mapping.rs       — ModelMapping + RouteTarget
  pricing.rs       — Pricing 值对象
  repository.rs    — 所有 Repository trait + 共享类型 + RepositoryError
```

### 1.1 lib.rs

```rust
pub mod provider;
pub mod preset;
pub mod mapping;
pub mod pricing;
pub mod repository;

pub use provider::{AuthType, Protocol, Provider};
pub use preset::{Preset, QuotaAdapter, ModelInfo};
pub use mapping::{ModelMapping, RouteTarget};
pub use pricing::Pricing;
pub use repository::{
    ConfigRepository, PresetRepository, ProviderRepository,
    ProviderUsage, QuotaInfo, QuotaTier, RepositoryError,
    RouteRepository, UsageRecord, UsageRepository,
};
```

### 1.2 Provider 聚合根 (provider.rs)

```rust
/// 认证类型
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthType { Bearer, ApiKey }

impl Default for AuthType { fn default() -> Self { Self::Bearer } }

impl AuthType {
    pub fn to_str(&self) -> &'static str;
    pub fn from_str(s: &str) -> Self;  // "bearer"→Bearer, "api_key"→ApiKey, 其余→Bearer
}

/// 数据协议
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol { Anthropic, #[serde(rename = "openai")] OpenAI }

impl Default for Protocol { fn default() -> Self { Self::Anthropic } }

impl Protocol {
    pub fn to_str(&self) -> &'static str;
    pub fn from_str(s: &str) -> Self;  // "openai"→OpenAI, 其余→Anthropic
}

/// Provider — ecc 的聚合根
pub struct Provider {
    pub name: String,
    pub base_url: String,
    pub auth_token: String,          // 明文（存储时由 infra 加密）
    pub auth_type: AuthType,
    pub protocol: Protocol,
    pub is_coding_plan: bool,
    pub model_mappings: Vec<ModelMapping>,
    pub pricing: HashMap<String, Pricing>,
    pub quota_adapter: Option<QuotaAdapter>,  // 新增：配额查询适配配置
}

impl Provider {
    pub fn find_mapping(&self, claude_model: &str) -> Option<&ModelMapping>;
    pub fn find_pricing(&self, provider_model: &str) -> Option<&Pricing>;
    pub fn calculate_cost(&self, provider_model: &str, input: u64, cache_read: u64, output: u64) -> f64;
}
```

**注意**：`quota_adapter` 是新增字段。当前已存在的 Provider 结构体没有此字段，需要添加。当前 ecc-domain 的 provider.rs 已经存在，仅需添加此字段 + `from_str` 方法。

### 1.3 Preset 模型 (preset.rs) — **新建**

```rust
/// 预设支持的模型信息（前端显示用）
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModelInfo {
    pub id: String,        // provider model id，如 "deepseek-chat"
    pub display_name: String,  // 前端显示名，如 "DeepSeek-V4 Flash"
}

/// 配额查询适配配置 — JSON 格式，描述如何查询和解析供应商的配额 API
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
pub struct QuotaAdapter {
    /// 配额 API 完整 URL
    pub quota_api_url: String,
    /// 认证方式：bearer / raw
    #[serde(default)]
    pub auth_style: String,
    /// 额外 HTTP 请求头
    #[serde(default)]
    pub extra_headers: HashMap<String, String>,
    /// JSON 字段映射规则，描述如何从响应中提取 tiers
    /// 格式：{ "tiers": [{ "name": "...", "limit_path": "...", "remaining_path": "...", "resets_at_path": "..." }], "error_check": { "field": "...", "value": false } }
    pub response_mapping: serde_json::Value,
}

/// 预设 — 一等公民数据，存数据库
pub struct Preset {
    pub name: String,                            // 预设名称，主键
    pub base_url: String,                        // 默认 API 基础地址
    pub alt_base_urls: HashMap<String, String>,  // protocol→url 映射
    pub protocol: Protocol,                      // 默认数据协议
    pub auth_type: AuthType,                     // 默认认证方式
    pub models: Vec<ModelInfo>,                  // 支持的模型列表
    pub pricing: HashMap<String, Pricing>,       // 各模型定价
    pub suggested_mappings: Vec<ModelMapping>,   // 建议的 claude_model→provider_model
    pub quota_adapter: Option<QuotaAdapter>,     // 配额查询适配
}
```

### 1.4 ModelMapping + RouteTarget (mapping.rs)

现有文件，保持不变：

```rust
pub struct ModelMapping {
    pub claude_model: String,    // Claude 侧模型名，如 "claude-sonnet-4-6"
    pub provider_model: String,  // 供应商侧模型名，如 "deepseek-chat"
}

pub struct RouteTarget {
    pub provider_name: String,
    pub provider_model: String,
}
```

### 1.5 Pricing 值对象 (pricing.rs)

现有文件，保持不变：

```rust
pub struct Pricing {
    pub input_per_m: f64,
    pub output_per_m: f64,
    pub cache_read_per_m: Option<f64>,
}

impl Pricing {
    pub fn calculate(&self, input_tokens: u64, cache_read_tokens: u64, output_tokens: u64) -> f64;
}
```

### 1.6 Repository trait (repository.rs)

现有文件，需要改动：

```rust
// ----- 错误类型（不变）-----
pub enum RepositoryError {
    NotFound(String),
    Storage(Box<dyn std::error::Error + Send + Sync>),
}

// ----- ProviderRepository（不变）-----
pub trait ProviderRepository: Send + Sync {
    fn list(&self) -> Result<Vec<Provider>, RepositoryError>;
    fn get(&self, name: &str) -> Result<Option<Provider>, RepositoryError>;
    fn save(&self, provider: &Provider) -> Result<(), RepositoryError>;
    fn delete(&self, name: &str) -> Result<(), RepositoryError>;
}

// ----- ConfigRepository（不变）-----
pub trait ConfigRepository: Send + Sync {
    fn get_default_provider(&self) -> Result<Option<String>, RepositoryError>;
    fn set_default_provider(&self, name: &str) -> Result<(), RepositoryError>;
}

// ----- 新增：PresetRepository -----
pub trait PresetRepository: Send + Sync {
    fn list_presets(&self) -> Result<Vec<Preset>, RepositoryError>;
    fn get_preset(&self, name: &str) -> Result<Option<Preset>, RepositoryError>;
    fn save_preset(&self, preset: &Preset) -> Result<(), RepositoryError>;
    fn delete_preset(&self, name: &str) -> Result<(), RepositoryError>;
    /// 首次启动检测 presets 表是否为空
    fn is_presets_empty(&self) -> Result<bool, RepositoryError>;
    /// 批量写入预设（DB Seed 用）
    fn seed_presets(&self, presets: &[Preset]) -> Result<(), RepositoryError>;
}

// ----- 新增：RouteRepository -----
pub trait RouteRepository: Send + Sync {
    /// 查找 claude_model 对应的路由条目（主 + 回退）
    fn get_routes(&self, claude_model: &str) -> Result<Option<Vec<RouteTarget>>, RepositoryError>;
    /// 列出所有路由
    fn list_routes(&self) -> Result<HashMap<String, Vec<RouteTarget>>, RepositoryError>;
    /// 从所有 Provider 的 mappings 重建路由表
    fn rebuild(&self) -> Result<(), RepositoryError>;
}

// ----- UsageRepository（不变）-----
pub trait UsageRepository: Send + Sync {
    fn record(&self, record: UsageRecord) -> Result<(), RepositoryError>;
    fn query(&self, start: &DateTime<Utc>, end: &DateTime<Utc>) -> Result<Vec<UsageRecord>, RepositoryError>;
    fn aggregate_by_provider(&self, start: &DateTime<Utc>, end: &DateTime<Utc>) -> Result<Vec<ProviderUsage>, RepositoryError>;
}

// ----- 共享类型（不变）-----
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

pub struct ProviderUsage {
    pub provider_name: String,
    pub total_requests: u64,
    pub total_input_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cache_read_tokens: u64,
    pub total_cost_usd: f64,
}

pub struct QuotaInfo {
    pub provider_name: String,
    pub success: bool,
    pub tiers: Vec<QuotaTier>,
    pub error: Option<String>,
}

pub struct QuotaTier {
    pub name: String,
    pub utilization: f64,
    pub resets_at: Option<String>,
}
```

---

## Crate 2: ecc-infra — 基础设施层

### Cargo.toml

```toml
[dependencies]
ecc-domain = { path = "../ecc-domain" }
rusqlite = { version = "0.31", features = ["bundled"] }
aes-gcm = "0.10"
base64 = "0.21"
sha2 = "0.10"
serde_json = { workspace = true }
thiserror = { workspace = true }
```

### 模块结构

```
ecc-infra/src/
  lib.rs    — pub mod crypto; pub mod repo; pub mod seed;
  crypto.rs — Token 加解密（从现有 infra 搬运，不变）
  repo.rs   — Repository 实现：持有 Connection + 加密 key，直接写 SQL
  seed.rs   — DB Seed（新建，读取内置 JSON 写入 presets 表）
```

**不设独立的 DAO 层**。DAO 函数只被 repo 调用，没有第二个消费者。SQL 直接写 repo 方法里，消除 `ProviderRow` ↔ `Provider` 的中间转换。

### 2.1 数据库 Schema

repo.rs 中 `init_schema()` 负责建表。

**现有表（已有）**：providers、model_mappings、pricing、config、usage_records

**新增表**：

```sql
-- presets 表
CREATE TABLE IF NOT EXISTS presets (
    name TEXT PRIMARY KEY,
    data TEXT NOT NULL  -- JSON blob，序列化整个 Preset 对象
);

-- routes 表（派生表，从 mappings 重建）
CREATE TABLE IF NOT EXISTS routes (
    claude_model TEXT NOT NULL,
    provider_name TEXT NOT NULL,
    provider_model TEXT NOT NULL,
    priority INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (claude_model, provider_name)
);
```

**providers 表新增列**：

```sql
ALTER TABLE providers ADD COLUMN quota_adapter TEXT;
```

### 2.2 加密模块 (crypto.rs)

从现有 `ecc-infra/src/crypto.rs` **完整搬运**，不改动。

接口：
```rust
pub fn derive_key(seed: &str) -> Vec<u8>;
pub fn encrypt(key: &[u8], plaintext: &str) -> Result<String, CryptoError>;
pub fn decrypt(key: &[u8], encoded: &str) -> Result<String, CryptoError>;
```

### 2.3 Repository 实现 (repo.rs)

不设中间 DAO 层，repo 直接写 SQL + 处理加密。

```rust
pub struct SqliteRepo {
    conn: Mutex<Connection>,
    crypto_key: Vec<u8>,
}

impl SqliteRepo {
    pub fn open(path: &Path, crypto_key: &[u8]) -> Result<Self, RepositoryError>;
    pub fn open_in_memory() -> Result<Self, RepositoryError>;

    // 私有方法
    fn init_schema(&self) -> Result<(), RepositoryError>;
    fn encrypt(&self, plain: &str) -> Result<String, RepositoryError>;
    fn decrypt(&self, enc: &str) -> Result<String, RepositoryError>;
}

// 实现 5 个 domain trait
impl ProviderRepository for SqliteRepo {
    // save: 加密 token → INSERT INTO providers (含 quota_adapter JSON)
    // get:  SELECT providers + JOIN mappings + JOIN pricing → 解密 token → 组装 Provider
    // list: 同上，遍历全部
    // delete: DELETE FROM providers WHERE name=?1
}
impl ConfigRepository for SqliteRepo { ... }
impl UsageRepository for SqliteRepo { ... }
impl PresetRepository for SqliteRepo {
    // save: serde_json::to_string(&preset) → INSERT INTO presets
    // get:  SELECT data → serde_json::from_str()
    // seed: 批量 INSERT
}
impl RouteRepository for SqliteRepo {
    // rebuild: 遍历所有 Provider → 收集 model_mappings → DELETE + 重新 INSERT routes 表
    // get_routes: SELECT FROM routes WHERE claude_model=?1 ORDER BY priority
    // list_routes: SELECT 全部
}
```

### 2.4 DB Seed (seed.rs) — **新建**

```rust
/// 内置预设数据（编译时嵌入）
const BUILTIN_PRESETS_JSON: &str = include_str!("presets.json");

/// 首次启动时初始化预设
pub fn seed_if_empty(repo: &dyn PresetRepository) -> Result<usize, RepositoryError> {
    if !repo.is_presets_empty()? { return Ok(0); }
    let presets: Vec<Preset> = serde_json::from_str(BUILTIN_PRESETS_JSON)
        .map_err(|e| RepositoryError::Storage(e.into()))?;
    let count = presets.len();
    repo.seed_presets(&presets)?;
    Ok(count)
}
```

**presets.json 结构**：

```json
[
  {
    "name": "DeepSeek",
    "base_url": "https://api.deepseek.com",
    "alt_base_urls": {},
    "protocol": "openai",
    "auth_type": "bearer",
    "models": [
      { "id": "deepseek-chat", "display_name": "DeepSeek-V3" },
      { "id": "deepseek-chat-latest", "display_name": "DeepSeek-V4 Flash" }
    ],
    "pricing": {
      "deepseek-chat": { "input_per_m": 0.27, "output_per_m": 1.10, "cache_read_per_m": 0.027 },
      "deepseek-chat-latest": { "input_per_m": 0.14, "output_per_m": 0.28, "cache_read_per_m": 0.014 }
    },
    "suggested_mappings": [
      { "claude_model": "claude-sonnet-4-6", "provider_model": "deepseek-chat-latest" },
      { "claude_model": "claude-haiku-4-5", "provider_model": "deepseek-chat" }
    ],
    "quota_adapter": null
  },
  {
    "name": "Kimi",
    "base_url": "https://api.kimi.com/coding",
    "alt_base_urls": { "anthropic": "https://api.kimi.com/coding/anthropic" },
    "protocol": "anthropic",
    "auth_type": "bearer",
    "models": [
      { "id": "kimi-for-coding", "display_name": "Kimi K2.6" }
    ],
    "pricing": {
      "kimi-for-coding": { "input_per_m": 0.00, "output_per_m": 0.00 }
    },
    "suggested_mappings": [
      { "claude_model": "claude-sonnet-4-6", "provider_model": "kimi-for-coding" }
    ],
    "quota_adapter": {
      "quota_api_url": "https://api.kimi.com/coding/v1/usages",
      "auth_style": "bearer",
      "extra_headers": { "Accept": "application/json" },
      "response_mapping": {
        "tiers": [
          {
            "name": "five_hour",
            "limit_path": "limits[0].detail.limit",
            "remaining_path": "limits[0].detail.remaining",
            "resets_at_path": "limits[0].detail.resetTime"
          },
          {
            "name": "weekly_limit",
            "limit_path": "usage.limit",
            "remaining_path": "usage.remaining",
            "resets_at_path": "usage.resetTime"
          }
        ]
      }
    }
  },
  {
    "name": "GLM",
    "base_url": "https://open.bigmodel.cn/api/anthropic",
    "alt_base_urls": {},
    "protocol": "anthropic",
    "auth_type": "api_key",
    "models": [
      { "id": "glm-5-fp8", "display_name": "GLM-5 FP8" }
    ],
    "pricing": {
      "glm-5-fp8": { "input_per_m": 0.00, "output_per_m": 0.00 }
    },
    "suggested_mappings": [
      { "claude_model": "claude-sonnet-4-6", "provider_model": "glm-5-fp8" }
    ],
    "quota_adapter": null
  },
  {
    "name": "MiniMax",
    "base_url": "https://api.minimax.io/v1",
    "alt_base_urls": {},
    "protocol": "openai",
    "auth_type": "bearer",
    "models": [
      { "id": "m2.5-coding", "display_name": "MiniMax M2.5 Coding" }
    ],
    "pricing": {
      "m2.5-coding": { "input_per_m": 0.00, "output_per_m": 0.00 }
    },
    "suggested_mappings": [
      { "claude_model": "claude-sonnet-4-6", "provider_model": "m2.5-coding" }
    ],
    "quota_adapter": {
      "quota_api_url": "https://api.minimax.io/v1/api/openplatform/coding_plan/remains",
      "auth_style": "bearer",
      "extra_headers": { "Content-Type": "application/json" },
      "response_mapping": {
        "error_check": { "field": "base_resp.status_code", "value": 0, "not_equal": true },
        "tiers": [
          {
            "name": "five_hour",
            "total_path": "model_remains[0].current_interval_total_count",
            "used_path": "model_remains[0].current_interval_usage_count",
            "resets_at_path": "model_remains[0].end_time"
          },
          {
            "name": "weekly_limit",
            "total_path": "model_remains[0].current_weekly_total_count",
            "used_path": "model_remains[0].current_weekly_usage_count",
            "resets_at_path": "model_remains[0].weekly_end_time"
          }
        ]
      }
    }
  }
]
```

---

## Crate 3: ecc-engine — 运行时管道

### Cargo.toml

```toml
[dependencies]
ecc-domain = { path = "../ecc-domain" }
tokio = { workspace = true }
reqwest = { workspace = true }
hyper = { workspace = true }
http = { workspace = true }
bytes = { workspace = true }
serde_json = { workspace = true }
uuid = { workspace = true }
thiserror = { workspace = true }
```

### 模块结构

```
ecc-engine/src/
  lib.rs           — pub mod + re-export Pipeline 相关类型
  middleware.rs     — Middleware trait + Next + Pipeline（从 ecc-core 搬运并重写，去掉 ecc-config 依赖）
  context.rs        — RequestContext（重写，去掉 ecc-config 依赖）
  converter.rs      — ProtocolConverter trait + get_converter 工厂（从 ecc-core/protocol/mod.rs 搬运）
  anthropic.rs      — AnthropicConverter 直通（搬运）
  openai.rs         — OpenAiConverter 双向转换（搬运，最大文件 ~800行）
  router.rs         — RouterMiddleware（重写，改为查询 RouteRepository）
  forwarder.rs      — Forwarder（重写，改为通用 HTTP 转发）
  usage_tracker.rs  — UsageTracker 中间件（重写）
  circuit_breaker.rs — CircuitBreaker（搬运，不变）
  rectifier.rs       — ThinkingRectifier（搬运，不变）
```

### 3.1 Pipeline 核心 (middleware.rs)

从 ecc-core 搬运，去掉对 ecc-config 的依赖。接口不变：

```rust
pub trait Middleware: Send + Sync {
    fn handle<'a>(&'a self, ctx: &'a mut RequestContext, next: Next<'a>)
        -> BoxFuture<'a, Result<(), MiddlewareError>>;
}

pub struct Next<'a> { ... }
impl Next<'a> { pub async fn run(self, ctx: &mut RequestContext) -> Result<(), MiddlewareError>; }

pub struct Pipeline {
    middlewares: Vec<Arc<dyn Middleware>>,
    max_retries: u8,
}

impl Pipeline {
    pub fn new() -> Self;
    pub fn with_max_retries(mut self, n: u8) -> Self;
    pub fn add(mut self, middleware: Arc<dyn Middleware>) -> Self;
    pub async fn execute(&self, ctx: &mut RequestContext) -> Result<(), MiddlewareError>;
}
```

### 3.2 RequestContext (context.rs)

重写，去掉 `ecc_config::provider::Protocol` 和 `ecc_config::route::RouteTarget` 依赖，改为使用 `ecc_domain` 类型：

```rust
pub struct RequestContext {
    pub id: Uuid,
    pub timestamp: Instant,

    // 原始请求
    pub method: Method,
    pub path: String,
    pub headers: HeaderMap,
    pub body: Bytes,

    // 路由解析结果
    pub requested_model: Option<String>,
    pub resolved_target: Option<RouteTarget>,      // 主路由目标
    pub fallback_targets: Vec<RouteTarget>,         // 回退目标列表
    pub protocol: Protocol,                         // 目标协议
    pub provider_config: Option<ProviderRef>,       // 目标供应商的核心配置

    // 协议转换后的上游请求
    pub upstream_url: Option<String>,
    pub upstream_headers: Option<Vec<(String, String)>>,
    pub upstream_body: Option<Bytes>,

    // 响应
    pub response_status: Option<u16>,
    pub usage: Option<TokenUsage>,

    // 重试
    pub retry_count: u8,
    pub max_retries: u8,
}

/// Provider 核心配置（Engine 只需要这些信息）
pub struct ProviderRef {
    pub name: String,
    pub base_url: String,
    pub auth_token: String,
    pub auth_type: AuthType,
    pub protocol: Protocol,
    pub pricing: HashMap<String, Pricing>,
}

pub struct TokenUsage {
    pub input_tokens: u64,
    pub cache_read_tokens: u64,
    pub output_tokens: u64,
}
```

### 3.3 协议转换 (converter.rs + anthropic.rs + openai.rs)

从 `ecc-core/protocol/` **完整搬运**，改动点：
- `convert_request` 不再需要自己拼 URL/headers，改为从 `ctx.provider_config` 读取
- 去掉 `ecc_config` 依赖，使用 `ecc_domain` 的 Protocol 类型

```rust
pub trait ProtocolConverter: Send + Sync {
    fn convert_request(&self, ctx: &RequestContext) -> Result<ConvertedRequest, MiddlewareError>;
    fn convert_response(&self, body: Bytes) -> Result<Bytes, MiddlewareError>;
    fn convert_stream_chunk(&self, chunk: Bytes) -> Result<Vec<String>, MiddlewareError>;
}

pub struct ConvertedRequest {
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Bytes,
}

pub fn get_converter(protocol: &ecc_domain::provider::Protocol) -> Box<dyn ProtocolConverter>;
```

AnthropicConverter：request/response/stream 全部透传。
OpenAiConverter：搬运现有实现（session 中已读过，~800 行，Anthropic↔OpenAI 完整双向转换 + 流式）。

### 3.4 Router 中间件 (router.rs) — **重写**

```rust
pub struct Router {
    route_repo: Arc<dyn RouteRepository>,
    provider_repo: Arc<dyn ProviderRepository>,
}

impl Router {
    pub fn new(route_repo: Arc<dyn RouteRepository>, provider_repo: Arc<dyn ProviderRepository>) -> Self;
}

impl Middleware for Router {
    fn handle(&self, ctx: &mut RequestContext, next: Next) -> BoxFuture<Result<(), MiddlewareError>> {
        // 1. 从 ctx.body 提取 model 字段
        // 2. 调用 route_repo.get_routes(model) 查找路由
        // 3. 如果没找到，尝试日期后缀回退（claude-haiku-4-5-20251001 → claude-haiku-4-5）
        // 4. 设置 ctx.resolved_target = 第一个结果
        // 5. 设置 ctx.fallback_targets = 其余结果
        // 6. 调用 provider_repo.get(target.provider_name) 获取 Provider
        // 7. 设置 ctx.protocol = provider.protocol
        // 8. 设置 ctx.provider_config = ProviderRef { name, base_url, auth_token, auth_type, protocol, pricing }
        // 9. next.run(ctx)
    }
}
```

### 3.5 Forwarder 中间件 (forwarder.rs) — **重写**

```rust
pub struct Forwarder {
    client: reqwest::Client,
}

impl Forwarder {
    pub fn new(client: reqwest::Client) -> Self;
}

impl Middleware for Forwarder {
    fn handle(&self, ctx: &mut RequestContext, next: Next) -> BoxFuture<Result<(), MiddlewareError>> {
        // 1. 读取 ctx.upstream_body + ctx.upstream_url + ctx.upstream_headers
        // 2. 如果有 body 是 None（非流式），调用 convert_request 生成 ConvertedRequest
        // 3. 发 HTTP POST 到上游
        //    - 如果 ctx.body 中有 stream:true，走流式转发
        //    - 否则走普通请求/响应
        // 4. 流式：逐 chunk 调用 convert_stream_chunk → SSE 输出
        // 5. 非流式：调用 convert_response 转换响应
        // 6. 提取用量信息到 ctx.usage
        // 7. 设置 ctx.response_status
        // 8. next.run(ctx)
    }
}
```

**Forwarder 的通用性**：不检查 provider 名称，不写 `if kimi` / `if deepseek`。只根据 `ctx.provider_config.protocol` 选择 converter，根据 `ctx.provider_config.auth_type` 构造 auth header。

### 3.6 UsageTracker 中间件 (usage_tracker.rs) — **重写**

```rust
pub struct UsageTracker {
    usage_repo: Arc<dyn UsageRepository>,
}

impl UsageTracker {
    pub fn new(usage_repo: Arc<dyn UsageRepository>) -> Self;
}

impl Middleware for UsageTracker {
    fn handle(&self, ctx: &mut RequestContext, next: Next) -> BoxFuture<Result<(), MiddlewareError>> {
        // 1. next.run(ctx) 先执行（等 Forwarder 完成后）
        // 2. 从 ctx.usage + ctx.provider_config 组装 UsageRecord
        // 3. 计算费用：provider_config.pricing.calculate(tokens)
        // 4. usage_repo.record(record)
    }
}
```

### 3.7 CircuitBreaker (circuit_breaker.rs)

从 ecc-core **完整搬运**，不依赖 ecc-config，零改动：

```rust
pub struct CircuitBreakerConfig { failure_threshold: u32, cooldown: Duration }
pub struct CircuitBreaker { ... }
impl CircuitBreaker {
    pub fn new(config: CircuitBreakerConfig) -> Self;
    pub fn is_open(&self, key: &str) -> bool;
    pub fn record_success(&self, key: &str);
    pub fn record_failure(&self, key: &str);
}
```

### 3.8 Rectifier (rectifier.rs)

从 ecc-core 搬运，处理 thinking block 修复。

### 3.9 Pipeline 执行流程图

```
请求进入 ProxyServer
      │
      ▼
┌─────────────────────────────────────────────────────┐
│ Pipeline                                              │
│                                                       │
│  ┌──────────┐   ┌───────────┐   ┌────────────┐       │
│  │  Router  │──▶│ Converter │──▶│ Rectifier   │       │
│  │          │   │(request)  │   │            │       │
│  └──────────┘   └───────────┘   └────────────┘       │
│       │                              │                │
│       │ resolved_target              │                │
│       │ provider_config              ▼                │
│       │ protocol            ┌────────────────┐       │
│       │                     │ CircuitBreaker  │       │
│       │                     └────────────────┘       │
│       │                              │                │
│       │                              ▼                │
│       │                     ┌────────────────┐       │
│       │                     │   Forwarder    │       │
│       │                     │ (HTTP 转发)     │       │
│       │                     └────────────────┘       │
│       │                              │                │
│       │                    ctx.usage   │                │
│       │                    ctx.response_status         │
│       │                              ▼                │
│       │                     ┌────────────────┐       │
│       │                     │ UsageTracker   │       │
│       │                     │ (记录用量)      │       │
│       │                     └────────────────┘       │
│       │                              │                │
│       ▼                              ▼                │
│  ┌─────────────────────────────────────────┐         │
│  │ Failover: 失败且有 fallback → 换目标重试  │         │
│  └─────────────────────────────────────────┘         │
└─────────────────────────────────────────────────────┘
      │
      ▼
返回给 Claude Code
```

---

## Crate 4: ecc-app — 用例编排层

### Cargo.toml

```toml
[dependencies]
ecc-domain = { path = "../ecc-domain" }
serde = { workspace = true }
serde_json = { workspace = true }
chrono = { workspace = true }
reqwest = { workspace = true }
```

### 模块结构

```
ecc-app/src/
  lib.rs               — pub mod + re-export 所有 Service
  provider_service.rs  — Provider CRUD + 默认供应商
  preset_service.rs    — Preset CRUD
  usage_service.rs     — 用量查询统计
  quota_service.rs     — 通用配额查询
  playground_service.rs — 连通性测试
```

### 4.1 ProviderService

```rust
// ── Command DTOs ──
pub struct CreateProviderCommand {
    pub name: String,
    pub base_url: String,
    pub auth_token: String,
    pub auth_type: AuthType,
    pub protocol: Protocol,
    pub is_coding_plan: bool,
    pub model_mappings: Vec<ModelMapping>,
    pub pricing: HashMap<String, Pricing>,
    pub quota_adapter: Option<QuotaAdapter>,
}

pub struct UpdateProviderCommand {
    pub base_url: Option<String>,
    pub auth_token: Option<String>,
    pub auth_type: Option<AuthType>,
    pub protocol: Option<Protocol>,
    pub is_coding_plan: Option<bool>,
    pub model_mappings: Option<Vec<ModelMapping>>,
    pub pricing: Option<HashMap<String, Pricing>>,
    pub quota_adapter: Option<Option<QuotaAdapter>>,  // Option<Option<...>>: None=不更新, Some(None)=清空
}

// ── Service ──
pub struct ProviderService<P, C, R>
where P: ProviderRepository, C: ConfigRepository, R: RouteRepository
{
    provider_repo: P,
    config_repo: C,
    route_repo: R,
}

impl<P, C, R> ProviderService<P, C, R>
where P: ProviderRepository, C: ConfigRepository, R: RouteRepository
{
    pub fn new(provider_repo: P, config_repo: C, route_repo: R) -> Self;

    pub fn list_providers(&self) -> Result<Vec<Provider>, RepositoryError>;
    pub fn get_provider(&self, name: &str) -> Result<Option<Provider>, RepositoryError>;

    pub fn create_provider(&self, cmd: CreateProviderCommand) -> Result<Provider, RepositoryError>;
    // 行为：校验 name 非空 → 校验不重复 → 构建 Provider → provider_repo.save() → route_repo.rebuild()

    pub fn update_provider(&self, name: &str, cmd: UpdateProviderCommand) -> Result<Provider, RepositoryError>;
    // 行为：查询现有 → 逐字段更新 → provider_repo.save() → route_repo.rebuild()

    pub fn delete_provider(&self, name: &str) -> Result<(), RepositoryError>;
    // 行为：校验存在 → provider_repo.delete() → route_repo.rebuild()

    pub fn set_default_provider(&self, name: &str) -> Result<(), RepositoryError>;
    pub fn get_default_provider(&self) -> Result<Option<Provider>, RepositoryError>;
}
```

**关键**：Provider 变更（create/update/delete）后自动调用 `route_repo.rebuild()` 重建路由表。

### 4.2 PresetService

```rust
pub struct CreatePresetCommand { pub preset: Preset }
pub struct UpdatePresetCommand { pub preset: Preset }

pub struct PresetService<R: PresetRepository> {
    repo: R,
}

impl<R: PresetRepository> PresetService<R> {
    pub fn new(repo: R) -> Self;

    pub fn list_presets(&self) -> Result<Vec<Preset>, RepositoryError>;
    pub fn get_preset(&self, name: &str) -> Result<Option<Preset>, RepositoryError>;
    pub fn create_preset(&self, cmd: CreatePresetCommand) -> Result<Preset, RepositoryError>;
    pub fn update_preset(&self, name: &str, cmd: UpdatePresetCommand) -> Result<Preset, RepositoryError>;
    pub fn delete_preset(&self, name: &str) -> Result<(), RepositoryError>;
}
```

Preset 的 CRUD 不会触发路由表重建（路由从 Provider mappings 派生，不涉及 preset）。

### 4.3 UsageService

```rust
pub struct UsageService<U: UsageRepository> {
    usage_repo: U,
    provider_repo: Arc<dyn ProviderRepository>,  // 用于获取定价信息
}

impl<U: UsageRepository> UsageService<U> {
    pub fn new(usage_repo: U, provider_repo: Arc<dyn ProviderRepository>) -> Self;

    /// 按日期范围查询用量记录
    pub fn query(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Result<Vec<UsageRecord>, RepositoryError>;

    /// 按供应商聚合统计
    pub fn aggregate(&self, start: DateTime<Utc>, end: DateTime<Utc>) -> Result<Vec<ProviderUsage>, RepositoryError>;

    /// 按供应商+模型查看明细（按小时/按天聚合）
    pub fn query_detail(&self, provider: &str, model: &str, days: u32) -> Result<UsageDetail, RepositoryError>;
}

pub struct UsageDetail {
    pub summary: UsageSummary,
    pub hourly: Vec<HourlyBucket>,
    pub daily: Vec<DailyBucket>,
    pub recent: Vec<UsageRecord>,
}

pub struct UsageSummary { pub total_requests, total_input_tokens, total_output_tokens, total_cost_usd }
pub struct HourlyBucket { pub hour: String, pub requests, input, output, cost }
pub struct DailyBucket { pub date: String, pub requests, input, output, cost }
```

### 4.4 QuotaService — 通用配额查询

```rust
pub struct QuotaService;

impl QuotaService {
    pub fn new() -> Self;

    /// 通用配额查询 — 根据 provider 的 quota_adapter 配置发起查询
    pub async fn query(
        &self,
        client: &reqwest::Client,
        provider: &Provider,
    ) -> QuotaInfo;

    /// 批量查询所有 Coding Plan 供应商的配额
    pub async fn query_all<I>(&self, client: &reqwest::Client, providers: I) -> HashMap<String, QuotaInfo>
    where I: Iterator<Item = &Provider>;
}
```

**query 内部流程**：
1. 如果没有 `quota_adapter` → 返回 `QuotaInfo { success: false, error: "not configured" }`
2. 按 `quota_api_url` + `auth_style` + `extra_headers` 发 HTTP GET
3. 拿到 JSON 响应
4. 按 `response_mapping` 解析
5. 组装 `QuotaInfo` 返回

**JSON path 解析器**（内嵌在 quota_service.rs 中）：
- 支持 dot notation：`data.limits[0].detail.limit`
- 支持数组索引：`model_remains[0].current_interval_total_count`
- 支持错误检查：`{ "field": "base_resp.status_code", "value": 0, "not_equal": true }`

### 4.5 PlaygroundService — 连通性测试

```rust
pub struct PlaygroundService;

impl PlaygroundService {
    pub async fn test(
        &self,
        client: &reqwest::Client,
        provider: &Provider,
        model: &str,
        message: &str,
    ) -> PlaygroundResult;
}

pub struct PlaygroundResult {
    pub status: u16,
    pub body: bytes::Bytes,
    pub latency_ms: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
}
```

通用实现：根据 `provider.protocol` 构建请求体（Anthropic 格式或 OpenAI 格式），不写供应商特异代码。

---

## Crate 5: ecc-api — HTTP 接入层

### 模块结构

```
ecc-api/src/
  lib.rs           — pub mod admin_server; pub mod proxy_server;
  admin_server.rs  — Dashboard + REST API
  proxy_server.rs  — 代理入口
```

### 5.1 ProxyServer (proxy_server.rs)

```rust
pub struct ProxyServer {
    pipeline: Arc<Pipeline>,
}

impl ProxyServer {
    pub fn new(pipeline: Arc<Pipeline>) -> Self;

    pub async fn handle(&self, req: Request<Incoming>) -> Response<BoxBody>;
    // 读取 body → 构造 RequestContext → pipeline.execute() → 返回响应
}
```

与当前实现基本一致，改动点是去掉对 `ecc-core` 和 `ecc-config` 的直接依赖。

### 5.2 AdminServer (admin_server.rs) — REST API

```rust
pub struct AdminServer {
    provider_service: Arc<ProviderService<...>>,
    preset_service: Arc<PresetService<...>>,
    usage_service: Arc<UsageService<...>>,
    quota_service: Arc<QuotaService>,
    playground_service: Arc<PlaygroundService>,
    quota_cache: Arc<RwLock<HashMap<String, QuotaInfo>>>,
    reqwest_client: reqwest::Client,
}

impl AdminServer {
    pub fn new(...) -> Self;
    pub async fn handle(&self, req: Request<Incoming>) -> Response<BoxBody>;
}
```

**路由表**：

| Method | Path | Handler | 说明 |
|--------|------|---------|------|
| GET | `/` | `dashboard()` | Dashboard HTML |
| GET | `/style.css` | `style_css()` | |
| GET | `/app.js` | `app_js()` | |
| GET | `/api/providers` | `list_providers()` | 供应商列表 |
| POST | `/api/providers` | `create_provider()` | 创建供应商 |
| PUT | `/api/providers/{name}` | `update_provider()` | 更新供应商 |
| DELETE | `/api/providers/{name}` | `delete_provider()` | 删除供应商 |
| GET | `/api/presets` | `list_presets()` | 预设列表 |
| POST | `/api/presets` | `create_preset()` | 创建预设 |
| PUT | `/api/presets/{name}` | `update_preset()` | 更新预设 |
| DELETE | `/api/presets/{name}` | `delete_preset()` | 删除预设 |
| GET | `/api/routes` | `list_routes()` | 路由表（只读，派生数据） |
| GET | `/api/usage` | `usage_stats()` | 用量统计 |
| GET | `/api/usage/detail` | `usage_detail()` | 用量明细 |
| GET | `/api/quota` | `query_all_quotas()` | 所有配额 |
| GET | `/api/quota/{name}` | `query_quota()` | 单个配额 |
| POST | `/api/playground` | `playground()` | 连通性测试 |
| OPTIONS | `*` | `cors_preflight()` | CORS 预检 |

**AdminServer 只做**：解析 HTTP 请求 → 调用 app 层 Service → 格式化为 JSON 响应。零业务逻辑。

**后台配额刷新**：AdminServer 初始化时启动 tokio::spawn 循环，每 20s 扫描所有 `is_coding_plan` 的 Provider，调用 QuotaService 查询并缓存。

---

## Crate 6: main.rs 组装

```rust
#[tokio::main]
async fn main() {
    // 1. 初始化日志
    // 2. 读取配置（proxy_port, admin_port, db_path, crypto_key）
    // 3. 打开 SQLite → SqliteRepo::open()
    // 4. DB Seed → seed_if_empty(&repo)
    // 5. 构建路由表 → repo.rebuild()
    // 6. 创建 Service 实例（注入 SqliteRepo）
    let provider_service = Arc::new(ProviderService::new(repo.clone(), repo.clone(), repo.clone()));
    let preset_service = Arc::new(PresetService::new(repo.clone()));
    let usage_service = Arc::new(UsageService::new(repo.clone(), Arc::new(repo.clone())));
    let quota_service = Arc::new(QuotaService::new());
    let playground_service = Arc::new(PlaygroundService::new());

    // 7. 构建 Pipeline（注入 repo）
    let pipeline = Pipeline::new()
        .add(RouterMiddleware::new(Arc::new(repo.clone()), Arc::new(repo.clone())))
        .add(Rectifier::new())
        .add(CircuitBreaker::new(...))
        .add(Forwarder::new(reqwest::Client::new()))
        .add(UsageTracker::new(Arc::new(repo.clone())));

    // 8. 启动 ProxyServer + AdminServer
    // 9. tokio::select! 监听两个 server
}
```

**SqliteRepo 需要同时实现多个 trait**：
- `ProviderRepository + ConfigRepository + UsageRepository + PresetRepository + RouteRepository`
- 用 `Arc<SqliteRepo>` 共享同一个实例
- 每个 Service 只接收它需要的 trait（通过泛型约束）

---

## 数据流总览

### 创建供应商（管理操作）

```
用户 → Dashboard UI → POST /api/providers
                           │
                           ▼
                    AdminServer::create_provider()
                           │
                    parse JSON → CreateProviderCommand
                           │
                           ▼
                    ProviderService::create_provider()
                           │
                    ┌──────┴──────┐
                    ▼              ▼
          provider_repo.save()   route_repo.rebuild()
          (写 providers 表)      (全量重建 routes 表)
                           │
                           ▼
                    返回 Provider → JSON 响应
```

### 代理转发（运行时操作）

```
Claude Code → POST /v1/messages → ProxyServer
                                       │
                                       ▼
                                Pipeline::execute()
                                       │
                                ┌──────┴──────┐
                                ▼             ▼
                            Router          Router
                        提取 model    查 route_repo
                             │             │
                             ▼             ▼
                     resolved_target  provider_config
                             │             │
                             ▼             ▼
                         Converter    Forwarder
                       协议转换 body  HTTP 转发
                             │             │
                             ▼             ▼
                    upstream_body    response + 用量
                             │             │
                             ▼             ▼
                        UsageTracker  CircuitBreaker
                        记录用量+费用  熔断状态更新
                                       │
                                       ▼
                                返回给 Claude Code
```

### 初始启动流程

```
main() 启动
    │
    ▼
SqliteRepo::open()  — 打开/创建数据库
    │
    ├── dao::init_schema()  — 建表
    │
    ▼
seed::seed_if_empty()  — 检查 presets 表
    │
    ├── 为空 → 读 presets.json → 写入
    └── 不为空 → 跳过
    │
    ▼
route_repo.rebuild()  — 从 Provider mappings 重建路由表
    │
    ▼
启动 Web Server
```

---

## 数据库完整 Schema

```sql
-- providers 表（已有，新加 quota_adapter 列）
CREATE TABLE providers (
    name           TEXT PRIMARY KEY,
    base_url       TEXT NOT NULL,
    auth_token     TEXT NOT NULL,          -- AES-256-GCM 加密
    auth_type      TEXT NOT NULL DEFAULT 'bearer',
    protocol       TEXT NOT NULL DEFAULT 'anthropic',
    is_coding_plan INTEGER NOT NULL DEFAULT 0,
    quota_adapter  TEXT                     -- JSON blob, NULL if not configured
);

-- presets 表（新建）
CREATE TABLE presets (
    name TEXT PRIMARY KEY,
    data TEXT NOT NULL                     -- JSON blob，完整的 Preset 对象
);

-- model_mappings 表（已有，不变）
CREATE TABLE model_mappings (
    provider_name  TEXT NOT NULL REFERENCES providers(name) ON DELETE CASCADE,
    claude_model   TEXT NOT NULL,
    provider_model TEXT NOT NULL,
    PRIMARY KEY (provider_name, claude_model)
);

-- pricing 表（已有，不变）
CREATE TABLE pricing (
    provider_name    TEXT NOT NULL REFERENCES providers(name) ON DELETE CASCADE,
    model            TEXT NOT NULL,
    input_per_m      REAL NOT NULL,
    output_per_m     REAL NOT NULL,
    cache_read_per_m REAL,
    PRIMARY KEY (provider_name, model)
);

-- config 表（已有，不变）
CREATE TABLE config (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- usage_records 表（已有，不变）
CREATE TABLE usage_records (
    id                INTEGER PRIMARY KEY AUTOINCREMENT,
    timestamp         TEXT NOT NULL,
    provider_name     TEXT NOT NULL,
    target_model      TEXT NOT NULL,
    requested_model   TEXT NOT NULL,
    input_tokens      INTEGER NOT NULL DEFAULT 0,
    output_tokens     INTEGER NOT NULL DEFAULT 0,
    cache_read_tokens INTEGER NOT NULL DEFAULT 0,
    cost_usd          REAL NOT NULL DEFAULT 0.0,
    latency_ms        INTEGER NOT NULL DEFAULT 0,
    status            INTEGER NOT NULL DEFAULT 0
);

-- routes 表（新建，派生表）
CREATE TABLE routes (
    claude_model   TEXT NOT NULL,
    provider_name  TEXT NOT NULL REFERENCES providers(name) ON DELETE CASCADE,
    provider_model TEXT NOT NULL,
    priority       INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (claude_model, provider_name)
);

-- 索引
CREATE INDEX IF NOT EXISTS idx_usage_ts ON usage_records(timestamp);
CREATE INDEX IF NOT EXISTS idx_routes_model ON routes(claude_model, priority);
```

---

## 实施顺序（26 个 Commit）

| # | Commit | 说明 |
|---|--------|------|
| 1 | 创建分支，清理旧 crate | `git checkout -b refactor/ddd-v2`，删除 ecc-domain/infra/app/core/gateway/config |
| 2 | ecc-domain: 基础值对象 | Provider, AuthType, Protocol, ModelMapping, Pricing, RouteTarget |
| 3 | ecc-domain: Preset 模型 | Preset, QuotaAdapter, ModelInfo |
| 4 | ecc-domain: Repository trait | 所有 5 个 trait + 共享类型 + RepositoryError |
| 5 | ecc-domain: 领域规则 | Provider::find_mapping/find_pricing/calculate_cost, Pricing::calculate |
| 6 | ecc-infra: 加密模块 | 搬运 crypto.rs |
| 7 | ecc-infra: Repository 实现 | Schema + 加密 + SQL + 5 个 trait impl（不设 DAO 层） |
| 9 | ecc-infra: DB Seed | presets.json + seed.rs |
| 10 | ecc-engine: Pipeline 基础 | Middleware trait, Next, Pipeline, RequestContext |
| 11 | ecc-engine: 协议转换 | 搬运 protocol/ 模块 |
| 12 | ecc-engine: Router | router.rs（查询 RouteRepository） |
| 13 | ecc-engine: Forwarder | forwarder.rs（通用 HTTP 转发） |
| 14 | ecc-engine: UsageTracker | usage_tracker.rs（用量记录） |
| 15 | ecc-engine: CircuitBreaker + Rectifier | 搬运 |
| 16 | ecc-app: ProviderService | CRUD + 默认供应商 |
| 17 | ecc-app: PresetService | Preset CRUD |
| 18 | ecc-app: UsageService | 用量查询统计 |
| 19 | ecc-app: QuotaService | 通用配额查询 + JSON path 解析器 |
| 20 | ecc-app: PlaygroundService | 连通性测试 |
| 21 | ecc-api: AdminServer | REST API 路由分发 |
| 22 | ecc-api: ProxyServer | 代理入口 + Pipeline 组装 |
| 23 | main.rs: 组装 | 依赖注入 + 启动 |
| 24 | 前端适配 | 调整 API 路径和请求/响应格式 |
| 25 | 端到端验证 | 启动 → 手动验证各功能 |
| 26 | PR 提交 | gh pr create |

---

## 待解决问题

1. **quota_adapter 的 response_mapping DSL 精确格式**：实现时详细设计。需要支持 dot notation 路径、数组索引、错误检查、resets_at 时间戳转换（毫秒 → ISO8601）

2. **Forwarder 的流式转发**：当前实现在 openai.rs 的 convert_stream_chunk 中。搬运过来后需要适配新的 Context 结构

3. **SQLite 多 trait 共享**：SqliteRepo 需要同时 impl 5 个 trait。可以用 `Arc<SqliteRepo>` + `clone()` 分发给不同 Service

4. **数据库迁移**：首次启动新建表。如果 providers 表已存在但没有 quota_adapter 列 → ALTER TABLE 添加（或重建时包含）
