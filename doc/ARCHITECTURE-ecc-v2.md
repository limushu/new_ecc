# ecc v2 架构设计文档

> ADMEMS 方法论驱动 | 创建于 2026-05-02 | 状态：**进行中（RA 阶段）**

---

## 1. 需求分析（PA — 预备架构）

### 1.1 ADMEMS 矩阵

| 维度 | 功能需求 | 质量需求 | 约束 |
|------|---------|---------|------|
| **业务** | F1. HTTP 反向代理（模型级路由）<br>F2. Anthropic/OpenAI 协议互转<br>F3. SSE 流式透传<br>F4. 供应商预设模板（编译时嵌入）<br>F5. Thinking block 修补<br>F6. 精细化用量统计（详情+费用+趋势+配额告警）<br>F7. 故障转移 + 熔断器<br>F8. Web Dashboard（内嵌 HTML/JS）<br>F9. Playground（测试供应商连通性、试对话） | Q1. 跨平台（Linux 为主，macOS/Windows 兼容）<br>Q2. 单二进制，零运行时依赖<br>Q3. 低资源占用<br>Q4. 请求转发低延迟<br>Q5. 高可靠（故障转移不丢请求）<br>Q6. 数据本地安全 | C1. Rust 技术栈<br>C2. 仅本地监听（127.0.0.1）<br>C3. v1.0 仅 Claude Code，架构预留多工具 |
| **用户** | F9. 一键启动（单二进制）<br>F10. 预设模板选择+填 key 即用<br>F11. 用量可视化（图表）<br>F12. 配额告警 | Q7. 新手 5 分钟上手<br>Q8. 交互反馈清晰（Web Dashboard） | C4. 目标用户：Claude Code 用户 |
| **开发** | F13. 模块化架构<br>F14. 完善文档<br>F15. 配置导入导出 | Q9. 可维护性<br>Q10. 可测试性 | C5. 单人/小团队开发 |

### 1.2 约束推导

| 约束 | 直接限制 | 推导出的功能需求 | 推导出的质量需求 |
|------|---------|-----------------|-----------------|
| **C1. Rust 技术栈** | 生态选型限 Rust crate | — | async runtime 选型影响延迟和体积 |
| **C2. 仅本地监听** | 无需 TLS、无需认证中间件 | — | 安全性天然满足（攻击面小） |
| **C3. v1.0 仅 CC** | 代理协议只处理 Anthropic | F2. OpenAI 协议转为可选模块 | 可扩展性需要模块化协议层 |
| **C4. CC 用户** | 交互以 Claude Code 工作流为主 | F5. Thinking block 修补必需 | — |
| **C5. 小团队** | 不能过度设计 | F13. 模块化但不过度抽象 | Q10. 测试必须自动化 |

### 1.3 关键驱动因素

**关键功能驱动（4 个）：**
1. **模型级路由** — ecc 的灵魂，多客户端并发用不同供应商
2. **协议转换引擎** — Anthropic ↔ OpenAI，可扩展
3. **精细化用量统计** — 详情+费用+趋势+配额，数据量最大的模块
4. **故障转移 + 熔断器** — 可靠性保障

**关键质量驱动（按优先级排序）：**
1. **Q2. 单二进制零依赖** — 影响所有技术选型
2. **Q4. 低延迟** — 代理层必须是非阻塞的
3. **Q5. 高可靠** — 故障转移架构
4. **Q7. 新手 5 分钟上手** — 预设模板系统

**关键质量冲突：**
- Q2（单二进制）vs Q3（低资源占用） — 依赖越少越好
- Q4（低延迟）vs Q5（高可靠） — 故障转移需要超时等待，会增加延迟

### 1.4 技术决策记录

| 决策项 | 选择 | 理由 |
|--------|------|------|
| 语言 | Rust | 用户为 Rust 开发者，追求性能和跨平台单二进制 |
| 前端 | 原生 HTML/JS + CDN（Chart.js） | 轻量、无 Node 构建链、`include_str!` 嵌入 |
| 配置存储 | TOML（配置）+ JSONL（用量） | TOML 易手写，JSONL append-only 适合用量记录 |
| 预设模板 | 编译时嵌入二进制的 TOML | 用户不可编辑核心模板，但可覆盖 |
| Web UI 形态 | 内嵌 HTTP 服务 + 静态文件 | 不引入 Tauri，保持轻量 |
| CLI | 弱化，主要交互走 Web Dashboard | CLI 仅保留 start/stop/status |
| 多工具 | v1.0 仅 Claude Code | 架构预留扩展点 |

---

## 2. 概念架构（CA）

> **状态：已完成**

### 2.1 核心用例：Claude Code 流式请求代理转发

```
                    Boundary Objects          Control Objects           Entity Objects
                    ───────────────          ─────────────────          ──────────────

Claude Code ──→  [HTTP Listener]  ──→  [Router]  ──────→  [Route Table]
                    (端口 4000)           │                        (routes.toml)
                                          │
                                    [Protocol Adapter]  ──→  [Provider Config]
                                          │                   (providers.toml)
                                          │
                                    [Forwarder]  ─────────→  [Provider API]
                                          │                   (上游供应商)
                                          │
                                    [Stream Transformer]  ─→  [Usage Tracker]
                                          │                   (JSONL + 内存索引)
                                          │
                                    [Thinking Rectifier]
                                          │
                                          ↓
Claude Code  ←────────────────  [SSE Response Writer]
```

### 2.2 高层分区：三层 + 两横切

```
┌─────────────────────────────────────────────────────┐
│                    Gateway Layer                      │
│                                                       │
│   ┌──────────┐    ┌──────────────┐                   │
│   │ Listener  │───→│  Dispatcher  │                   │
│   │ (HTTP)    │    │  (路由分发)   │                   │
│   └──────────┘    └──────┬───────┘                   │
│                          │                            │
└──────────────────────────┼────────────────────────────┘
                           │ RequestContext
┌──────────────────────────┼────────────────────────────┐
│                    Proxy Layer                         │
│                          │                            │
│   ┌──────────┐    ┌──────▼───────┐    ┌──────────┐   │
│   │  Router   │←──│  Middleware   │──→│ Forwarder │   │
│   │ (路由解析) │    │   Chain      │    │ (转发器)  │   │
│   └──────────┘    │              │    └────┬─────┘   │
│                   │ ┌──────────┐ │         │         │
│                   │ │ Protocol │ │         │         │
│                   │ │ Adapter  │ │         │         │
│                   │ └──────────┘ │         │         │
│                   │ ┌──────────┐ │         │         │
│                   │ │ Thinking │ │         │         │
│                   │ │ Rectifier│ │         │         │
│                   │ └──────────┘ │         │         │
│                   │ ┌──────────┐ │         │         │
│                   │ │Circuit   │ │         │         │
│                   │ │Breaker   │ │         │         │
│                   │ └──────────┘ │         │         │
│                   └──────────────┘         │         │
└────────────────────────────────────────────┼─────────┘
                                             │
┌────────────────────────────────────────────┼─────────┐
│                    Data Layer               │         │
│                                            │         │
│   ┌──────────┐  ┌──────────┐  ┌───────────▼──┐      │
│   │ Config   │  │  Usage   │  │  Provider    │      │
│   │ Store    │  │  Store   │  │  Client      │      │
│   │(TOML)    │  │(JSONL+   │  │  (HTTP)      │      │
│   │          │  │ 内存索引) │  │              │      │
│   └──────────┘  └──────────┘  └──────────────┘      │
│                                                       │
│   ┌──────────┐  ┌──────────┐                         │
│   │ Preset   │  │ Dashboard│                         │
│   │ Store    │  │ (Web UI) │                         │
│   │(编译嵌入) │  │          │                         │
│   └──────────┘  └──────────┘                         │
└───────────────────────────────────────────────────────┘

         ── 横切关注点 ──────────────────────────
         ┌────────────┐  ┌──────────────┐
         │   Logger   │  │   Metrics    │
         │  (结构化)   │  │ (延迟/计数)   │
         └────────────┘  └──────────────┘
```

### 2.3 中间件风格

**选择：trait Middleware 管道（注册式）**

```rust
trait Middleware: Send + Sync {
    async fn process(&self, ctx: &mut RequestContext, next: Next<'_>) -> Result<()>;
}
```

注册式管道，中间件按序执行，每个可读取/修改 RequestContext，通过 `next` 传递到下一个。新增中间件不需要改动已有代码。

**预设中间件链（按执行顺序）：**
1. `RouterMiddleware` — 解析模型名，查路由表，确定目标供应商（优先级列表）
2. `ProtocolAdapter` — Anthropic ↔ OpenAI 协议转换
3. `ThinkingRectifier` — Thinking block 修补
4. `CircuitBreaker` — 按路由粒度熔断，连续失败则跳过该路由条目
5. `Forwarder` — 实际发送请求到供应商 API
6. `UsageTracker` — 提取用量，写入 JSONL + 内存索引

### 2.4 路由模型

**优先级列表模式：**

```toml
[routes."claude-sonnet-4-6"]
targets = [
  { provider = "kimi", model = "K2.6", priority = 1 },
  { provider = "zhipu", model = "glm-4", priority = 2 },
]
```

- 按优先级依次尝试
- 某条目标失败（超时/5xx），自动尝试下一个优先级
- CircuitBreaker 按路由条目粒度熔断：`(claude-sonnet-4-6, kimi, K2.6)` 作为一个熔断单元
- 熔断后该条目被跳过，不影响同一供应商的其他路由

### 2.5 非功能设计决策

| 质量目标 | 场景 | 验证条件 | 设计决策 |
|---------|------|---------|---------|
| Q2. 单二进制 | 无网络 CentOS 部署 | `scp ecc && ./ecc start` 即可 | 静态编译 + `include_str!` 嵌入前端/预设；reqwest rustls 后端 |
| Q4. 低延迟 | CC 流式请求经 ecc 转发 | 代理增加延迟 < 5ms | tokio async + 零拷贝流式转发；协议转换原地修改 |
| Q5. 高可靠 | 主供应商 500/超时 | 3 秒内自动切换备用 | 优先级列表 + 按路由粒度 CircuitBreaker |
| Q7. 易上手 | 新用户首次使用 | 5 分钟内完成配置 | 预设模板嵌入；Dashboard 引导流程；自动检测 CC 配置 |

---

## 3. 细化架构（RA — 5 视图）

> **状态：已完成**

### 3.1 逻辑架构

```
ecc
├── gateway/                    # Gateway Layer
│   ├── listener                # HTTP 监听（tokio + hyper）
│   ├── dispatcher              # 请求分发 → 构造 RequestContext
│   └── admin                   # Web Dashboard HTTP 服务
│
├── proxy/                      # Proxy Layer
│   ├── middleware               # trait Middleware + Pipeline 执行器
│   ├── router                   # 路由解析（优先级列表 + 日期后缀回退）
│   ├── protocol                 # 协议转换引擎
│   │   ├── anthropic            # Anthropic 原生协议
│   │   └── openai               # OpenAI 兼容协议
│   ├── rectifier                # Thinking block 修补
│   ├── circuit_breaker          # 按路由粒度熔断器
│   ├── forwarder                # HTTP 转发（reqwest）
│   └── context                  # RequestContext 定义
│
├── data/                       # Data Layer
│   ├── config                   # TOML 配置读写
│   ├── usage                    # JSONL 用量记录 + 内存索引
│   ├── preset                   # 预设模板（编译时嵌入）
│   └── pricing                  # 模型单价数据
│
├── web/                        # 前端静态资源
│   ├── dashboard.html
│   ├── app.js
│   └── style.css
│
└── main.rs                     # 入口：解析参数 → 启动服务
```

**RequestContext 定义：**

```rust
struct RequestContext {
    // 请求标识
    id: Uuid,
    timestamp: Instant,

    // 原始请求
    method: Method,
    path: String,
    headers: HeaderMap,
    body: Bytes,

    // 解析后的状态（中间件逐步填充）
    requested_model: Option<String>,
    resolved_target: Option<RouteTarget>,  // 当前选中的优先级条目
    fallback_targets: Vec<RouteTarget>,     // 剩余备选（一次性解析）
    protocol: Protocol,                     // Anthropic / OpenAI

    // 响应
    response_status: Option<u16>,
    usage: Option<TokenUsage>,

    // 重试
    retry_count: u8,
    max_retries: u8,
}
```

### 3.2 物理架构

```
┌─────────────────────────────────────────────┐
│              ecc 单进程二进制                  │
│                                              │
│  ┌─────────────┐     ┌─────────────────┐    │
│  │ Proxy Server │     │  Admin Server    │    │
│  │  :4000       │     │  :4001           │    │
│  │  (hyper)     │     │  (hyper)         │    │
│  └──────┬───────┘     └────────┬─────────┘    │
│         │                      │              │
│         │    tokio runtime      │              │
│         │   (多任务并发)         │              │
│         │                      │              │
│  ┌──────▼──────────────────────▼──────────┐   │
│  │           共享状态 (独立锁)              │   │
│  │  Arc<RwLock<RouteTable>>       读多写少  │   │
│  │  Arc<Mutex<CircuitBreakers>>   读写各半  │   │
│  │  Arc<Mutex<UsageIndex>>        写多读少  │   │
│  └───────────────────────────────────────┘   │
│                                              │
│  ┌───────────────────────────────────────┐   │
│  │              文件系统                   │   │
│  │  ~/.config/ecc/                        │   │
│  │  ├── providers.toml                    │   │
│  │  ├── routes.toml                       │   │
│  │  ├── usage/                            │   │
│  │  │   ├── 2026-05-02.jsonl              │   │
│  │  │   └── ...                           │   │
│  │  └── config.toml  (全局设置+配额)       │   │
│  └───────────────────────────────────────┘   │
└──────────────────────────────────────────────┘
```

**锁策略：每状态独立锁，避免互相阻塞。**

### 3.3 运行时架构

```
┌──────────────────────── tokio runtime ────────────────────────┐
│                                                                │
│  ┌─── 主任务 ──────────────────────────────────────────────┐   │
│  │  task: proxy_server (listen :4000)                       │   │
│  │    │                                                     │   │
│  │    ├── spawn: handle_request_1                           │   │
│  │    │     ├── 中间件链执行 (async)                         │   │
│  │    │     ├── forwarder: 上游 HTTP 请求 (async)            │   │
│  │    │     └── SSE 流式回传 (hyper body stream, 自带背压)   │   │
│  │    │                                                     │   │
│  │    ├── spawn: handle_request_2 ...                       │   │
│  │    └── spawn: handle_request_N ...                       │   │
│  └──────────────────────────────────────────────────────────┘   │
│                                                                │
│  ┌─── 主任务 ──────────────────────────────────────────────┐   │
│  │  task: admin_server (listen :4001)                       │   │
│  │    ├── GET  /          → Dashboard HTML                  │   │
│  │    ├── GET  /api/*     → 查询状态/用量                    │   │
│  │    ├── POST /api/*     → 修改配置/路由                    │   │
│  │    └── GET  /api/events → SSE 实时推送（告警）             │   │
│  │    └── POST /api/playground → Playground 测试对话          │   │
│  └──────────────────────────────────────────────────────────┘   │
│                                                                │
│  ┌─── 后台任务 ────────────────────────────────────────────┐   │
│  │  task: usage_flusher                                     │   │
│  │    每 30s 将内存用量缓冲 → JSONL 文件                      │   │
│  └──────────────────────────────────────────────────────────┘   │
│                                                                │
│  ┌─── 后台任务 ────────────────────────────────────────────┐   │
│  │  task: circuit_breaker_reset                             │   │
│  │    定期检查熔断器冷却期，到期则进入半开状态                 │   │
│  └──────────────────────────────────────────────────────────┘   │
└────────────────────────────────────────────────────────────────┘
```

**并发安全：**

| 并发场景 | 竞争资源 | 保护机制 |
|---------|---------|---------|
| 多请求同时读路由表 | `Arc<RwLock<RouteTable>>` | RwLock 读锁并发 |
| Dashboard 修改路由时请求正在查表 | 同上 | RwLock 写锁短暂独占 |
| 多请求同时更新熔断器计数 | `Arc<Mutex<CircuitBreakers>>` | Mutex 短暂持锁 |
| 多请求同时写用量缓冲 | `Arc<Mutex<UsageIndex>>` | Mutex + 批量 flush |
| flusher 写文件时 Dashboard 读用量 | JSONL 文件 | append-only + 按天文件，无冲突 |

**流式转发：使用 hyper 原生 body stream，自带背压控制，无需额外 channel。**

### 3.4 开发架构

**Workspace 多 crate 结构（4 个子 crate）：**

```
ecc/
├── Cargo.toml                  # workspace 根
├── Cargo.lock
│
├── crates/
│   ├── ecc-core/               # 核心库（代理、路由、协议转换）
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── context.rs      # RequestContext
│   │       ├── middleware.rs    # trait Middleware + Pipeline
│   │       ├── router.rs       # 路由解析 + 优先级列表
│   │       ├── protocol/
│   │       │   ├── mod.rs
│   │       │   ├── anthropic.rs
│   │       │   └── openai.rs
│   │       ├── rectifier.rs    # Thinking block 修补
│   │       ├── circuit_breaker.rs
│   │       ├── forwarder.rs    # HTTP 转发
│   │       └── usage.rs        # 用量记录 + 内存索引
│   │
│   ├── ecc-config/             # 配置管理（TOML 读写、预设模板）
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── provider.rs     # 供应商配置
│   │       ├── route.rs        # 路由配置
│   │       ├── preset.rs       # 预设模板（include_str! 嵌入）
│   │       └── pricing.rs      # 模型单价
│   │
│   ├── ecc-gateway/            # HTTP 监听 + Dashboard
│   │   ├── Cargo.toml
│   │   └── src/
│   │       ├── lib.rs
│   │       ├── proxy_server.rs # :4000 代理服务
│   │       ├── admin_server.rs # :4001 Dashboard 服务
│   │       ├── api.rs          # Dashboard REST API
│   │       └── playground.rs   # Playground API（测试供应商）
│   │
│   └── ecc-web/                # 前端静态资源
│       ├── Cargo.toml
│       └── src/
│           ├── lib.rs          # include_str! 嵌入 HTML/JS/CSS
│           └── static/
│               ├── dashboard.html
│               ├── app.js
│               └── style.css
│
├── presets/                    # 供应商预设模板源文件
│   ├── deepseek.toml
│   ├── kimi.toml
│   ├── zhipu.toml
│   └── ...
│
├── src/                        # 二进制入口
│   └── main.rs
│
└── tests/                      # 集成测试
    ├── proxy_test.rs
    ├── protocol_test.rs
    └── usage_test.rs
```

**Workspace 依赖关系：**

```
ecc (binary)
├── ecc-core
│   └── ecc-config
├── ecc-gateway
│   ├── ecc-core
│   ├── ecc-config
│   └── ecc-web
└── ecc-web (嵌入前端资源)
```

### 3.5 数据架构

**存储分布：**

```
┌──────── 编译时嵌入 ──────────────┐  ┌──────── 运行时文件 ─────────────────┐
│                                   │  │                                      │
│  presets/*.toml                    │  │  ~/.config/ecc/                      │
│  ──include_str!──→ 二进制内只读    │  │  ├── config.toml     全局设置+配额    │
│  (deepseek, kimi, zhipu, ...)     │  │  ├── providers.toml  供应商配置      │
│                                   │  │  ├── routes.toml     路由表          │
│                                   │  │  ├── overrides/      用户预设覆盖     │
│                                   │  │  │   └── deepseek.toml (可选)        │
│                                   │  │  └── usage/          按天滚动 JSONL   │
│                                   │  │      ├── 2026-05-02.jsonl            │
│                                   │  │      └── ...                         │
└───────────────────────────────────┘  └──────────────────────────────────────┘
```

**数据流矩阵：**

| 数据 | 生产者 | 存储 | 消费者 | 一致性要求 |
|------|--------|------|--------|-----------|
| 供应商配置 | Dashboard API | providers.toml | Router | 写后立即可见 |
| 路由表 | Dashboard API | routes.toml | Router | 写后立即可见 |
| 预设模板 | 开发时编写 | 二进制嵌入 | Dashboard 展示 | 不可变 |
| 预设覆盖 | Dashboard API | overrides/*.toml | Preset 加载时合并 | 写后下次加载可见 |
| 全局配置 | Dashboard API | config.toml | 配额检查 | 写后立即可见 |
| 用量记录 | Forwarder | 内存缓冲 → JSONL | Dashboard / 配额告警 | 允许丢最后 30s |
| 熔断器状态 | CircuitBreaker | 内存 | Router 决策 | 允许短暂不一致 |
| 费用数据 | UsageTracker | 内存索引 + pricing | Dashboard | 最终一致 |

**用量记录格式（JSONL）：**

```json
{"ts":"2026-05-02T14:32:01.234Z","req_id":"a1b2c3","model":"claude-sonnet-4-6","provider":"kimi","target":"K2.6","input_tok":1520,"output_tok":832,"latency_ms":1247,"status":200,"cost_usd":0.0034}
```

**配额告警：超限仅告警（Dashboard SSE 推送通知），不自动干预。**

```
请求完成 → UsageTracker 写入用量
                  ↓
           累加当日/当月用量到内存计数器
                  ↓
           检查 config.toml 中的阈值
                  ↓
           超限 → Dashboard SSE 推送告警
```

---

## 4. 风险登记

### 未解决权衡

| # | 权衡 | 影响 | 缓解措施 |
|---|------|------|---------|
| R1 | **JSONL vs SQLite**：用量统计全用文件存储，大量历史数据查询（趋势图、按月汇总）性能未验证 | 如果用户积累数月数据，Dashboard 加载趋势图可能变慢 | 内存索引缓存热门查询；必要时迁移到 SQLite（ecc-config crate 预留接口） |
| R2 | **trait Middleware 的抽象成本**：注册式管道引入间接调用，增加调试难度 | 链中某个中间件失败时，调用栈不如显式函数链直观 | 每个 Middleware 实现 Debug trait，记录结构化日志（请求 ID + 中间件名） |
| R3 | **预设模板编译嵌入 vs 远程更新**：内置预设无法热更新，新增供应商需发新版本 | 用户无法获取最新的预设配置 | 提供覆盖机制（overrides/）；后续可加远程预设仓库 |

### 假设

| # | 假设 | 如果不成立 |
|---|------|-----------|
| A1 | 个人开发者场景，单机并发请求数 < 10 | 如果需要支持团队共享代理，需要加入认证层和网络监听配置 |
| A2 | JSONL 按天滚动，单日文件 < 10MB（约 5 万条请求） | 如果超过，需要考虑文件分割或迁移 SQLite |
| A3 | 供应商 API 响应格式相对稳定 | 如果频繁变动，Protocol Adapter 需要版本化 |
| A4 | 用户机器有浏览器可访问 Dashboard | 无头服务器场景需要保留 CLI 作为后备 |

### 开放问题

| # | 问题 | 决策时机 |
|---|------|---------|
| O1 | 是否需要支持 HTTP/2 上游连接？hyper 支持 h2，但 reqwest 默认 http1 | 实现阶段，测试上游供应商的实际支持情况 |
| O2 | Dashboard 前端是否需要打包 Chart.js 到二进制中（离线可用）？CDN 依赖网络 | 实现阶段，取决于 Q2（离线部署）的优先级 |
| O3 | 熔断器的具体参数（连续失败次数 N、冷却期时长）是否可配置？ | RA 阶段细化 config.toml schema 时决定 |
| O4 | 是否需要 daemon 模式（后台运行）还是仅 foreground？ | 实现阶段，取决于目标部署方式（systemd service / 直接运行） |

---

## 附录

### A. 原始需求来源

- ecc wiki：`/media/lee/SOURCE/003-code/code/ecc/wiki/`
- 现有源码：`/media/lee/SOURCE/003-code/code/ecc/raw/easy_cc_switch/`
- 竞品参考：cc-switch（Rust/Tauri）
