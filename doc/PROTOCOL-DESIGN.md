# Protocol Conversion Module Design

## 核心问题

ecc 接收 Claude Code 发来的 **Anthropic Messages API** 格式请求，但上游供应商（DeepSeek、Kimi、GLM 等）各自使用不同协议。需要将请求转换为上游能理解的格式，再将上游响应转回 Anthropic 格式。

```
                    ┌─────────────────┐
  Claude Code ────► │  ecc (Anthropic │ ────► 上游供应商
  (Anthropic 格式)  │   原生格式)      │      (各种协议)
                    └─────────────────┘
```

## 协议差异分析

基于对 Anthropic Messages API 和 OpenAI Chat Completions API 官方文档的独立研究。

### 消息结构差异

| 概念 | Anthropic | OpenAI 兼容 |
|------|-----------|-------------|
| 系统提示词 | `system` 顶层字段（字符串或数组） | `messages[0].role = "system"` 消息 |
| thinking | `content` 中的 `{type:"thinking"}` 块 | `reasoning_content` 字段（DeepSeek 扩展） |
| tool 调用 | `content` 中的 `{type:"tool_use"}` 块 | `tool_calls` 数组，`content` 为 null |
| tool 结果 | `content` 中的 `{type:"tool_result"}` | `role:"tool"` 的独立消息 |
| 图片 | `{type:"image", source:{type:"base64",...}}` | `{type:"image_url", image_url:{url:"data:..."}}` |

### 请求字段映射

| Anthropic | OpenAI | Notes |
|-----------|--------|-------|
| `model` | `model` | 直接复制 |
| `max_tokens` | `max_tokens` | o-series 用 `max_completion_tokens` |
| `temperature` | `temperature` | 直接复制 |
| `top_p` | `top_p` | 直接复制 |
| `stop_sequences` | `stop` | 重命名 |
| `stream` | `stream` | 直接复制 |
| `tools` | `tools` | 包装为 `{type:"function",function:{name,description,parameters}}` |
| `system` | 第一条 `role:"system"` 消息 | 标准化 |
| `thinking.budget_tokens` | `reasoning_effort` | 启发式映射: ≤5k→low, ≤10k→medium, >10k→high |

### 响应字段映射

| OpenAI | Anthropic | Notes |
|--------|-----------|-------|
| `choices[0].message.content` | `content:[{type:"text",text:"..."}]` | 字符串→内容块数组 |
| `choices[0].message.tool_calls` | `content:[{type:"tool_use",id,name,input}]` | 解包 function 包装 |
| `choices[0].message.reasoning_content` | `content:[{type:"thinking",thinking:"..."}]` | DeepSeek 推理→思考 |
| `choices[0].finish_reason` | `stop_reason` | 值映射（见下表） |
| `usage.prompt_tokens` | `usage.input_tokens` | 重命名 |
| `usage.completion_tokens` | `usage.output_tokens` | 重命名 |
| `usage.prompt_tokens_details.cached_tokens` | `usage.cache_read_input_tokens` | 重命名 |

### finish_reason → stop_reason 映射

| OpenAI | Anthropic |
|--------|-----------|
| `stop` | `end_turn` |
| `length` | `max_tokens` |
| `tool_calls` | `tool_use` |
| `content_filter` | `end_turn` |

### 流式转换

OpenAI 流格式：`data: {json}\n` + `data: [DONE]\n`
Anthropic 流格式：有名事件 `event: message_start\ndata: {json}\n\n`

转换规则：

1. 第一个含 `delta.role` 的 chunk → `event: message_start`
2. `delta.content` → `event: content_block_delta` with `text_delta`
3. `delta.reasoning_content` → `event: content_block_delta` with `thinking_delta`
4. `delta.tool_calls[i]` 首次出现 → `event: content_block_start` with `tool_use` + `event: content_block_delta` with `input_json_delta`
5. `delta.tool_calls[i]` 后续 → `event: content_block_delta` with `input_json_delta`
6. `finish_reason` 出现 → `event: message_delta` with `stop_reason` + `event: message_stop`
7. Usage chunk → 并入 `message_start` 或 `message_delta` 的 usage
8. `data: [DONE]` → 消费（不转发）

## 架构设计

### 两层设计

**协议层**：根据 `RouteTarget` 的 provider 配置中声明的 `Protocol`（Anthropic / OpenAI）选择转换器。

- `Anthropic` 协议 → 直接透传，不转换（Kimi 用这个）
- `OpenAI` 协议 → 执行 Anthropic→OpenAI 请求转换 + OpenAI→Anthropic 响应转换

**供应商适配层**：每个供应商可能有字段级的差异。通过 `Provider` 配置中的扩展字段控制，而非硬编码供应商名称。

- GLM 可能不识别 `reasoning_effort`，需要过滤
- 有些供应商的 `stop_reason` 映射可能不同
- billing header 需要统一剥离（`x-anthropic-billing-header` 前缀）

### 转换原则

**宽容输出、严格输入**：
- 发送到上游时过滤掉上游不认识的字段
- 接收上游响应时宽容解析，缺少的字段给默认值

### 模块结构

```
protocol/
  mod.rs          — ProtocolConverter trait + ProtocolMiddleware
  anthropic.rs    — Anthropic 透传（不做转换）
  openai.rs       — Anthropic↔OpenAI 双向转换（请求、响应、流式）
```

### ProtocolConverter trait

```rust
pub trait ProtocolConverter: Send + Sync {
    /// 将 Anthropic 格式请求转为目标协议
    fn convert_request(&self, ctx: &RequestContext) -> Result<ConvertedRequest, MiddlewareError>;

    /// 将上游响应体转回 Anthropic 格式
    fn convert_response(&self, body: Bytes) -> Result<Bytes, MiddlewareError>;

    /// 将上游流式 chunk 转为 Anthropic SSE 事件
    fn convert_stream_chunk(&self, chunk: Bytes) -> Result<Vec<SseEvent>, MiddlewareError>;
}
```

### 扩展性策略

新增 OpenAI 兼容供应商时，只要遵循标准 OpenAI 协议就能直接用。遇到差异时加配置而非改代码。

具体供应商特化通过 `Provider` 配置的扩展字段控制（未来按需添加），而非在代码中硬编码供应商名称做条件分支。

## TDD 测试进度

| # | 测试行为 | 关键断言 | 状态 |
|---|---------|---------|------|
| T23 | Anthropic 请求→OpenAI 格式 | system→system message, tools→function tools | PASS |
| T24 | OpenAI 响应→Anthropic 格式 | content blocks 正确, stop_reason 映射正确 | PASS |
| T25 | OpenAI SSE 流→Anthropic SSE 流 | message_start / content_block_delta / message_stop 事件正确 | PASS |
| T26 | Tool calls 转换 | tool_use→tool_calls, tool_result→tool messages | PASS |
| T27 | Thinking block (reasoning_content) 转换 | DeepSeek reasoning→Anthropic thinking | PASS |
| T28 | 图片 base64 转换 | Anthropic image→OpenAI image_url | PASS |

## API 文档

详细的模块和接口文档通过 Rust 标准 doc comments 编写，使用 `cargo doc --open` 查看。主要文档入口：

- `ecc_core` crate — 模块总览
- `ecc_core::protocol` — 协议转换模块（含映射表和使用示例）
- `ecc_core::protocol::openai` — OpenAI 转换器详细映射
- `ecc_core::middleware` — 中间件管道执行和故障转移
- `ecc_core::router` — 路由解析规则
- `ecc_config` crate — 配置管理模块总览
