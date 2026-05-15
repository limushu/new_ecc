# ECC

An LLM API gateway designed for Claude Code. Route requests to any AI provider, monitor usage, and manage conversations — all from a single dashboard.

`Rust` `Claude Code` `Multi-Provider` `Dashboard`

---

## Features

- **Multi-Provider Routing** — Map Claude model names to any AI backend (OpenAI, Anthropic, Gemini, DeepSeek, local models, etc.)
- **Protocol Conversion** — Automatic Anthropic Messages API ↔ OpenAI Chat Completions API translation, transparent to clients
- **Model Mapping** — Map `claude-sonnet-4-20250514` to any provider-specific model name
- **Force Thinking** — Automatically upgrades `thinking.type: "adaptive"` to `"enabled"` with configurable budget, ensuring extended thinking is always captured
- **Circuit Breaker** — Protects against cascading failures with automatic cooldown and retry
- **Usage Analytics** — Token usage tracking with timeline charts, per-provider/model drilldown, and cost estimation
- **Session Recording** — Auto-captures complete conversations with thinking, tool use (Edit/Write/Read/Bash), grouped by session with time-gap heuristic
- **Playground** — Test any configured provider directly from the dashboard
- **Quota Query** — Real-time quota balance polling for supported providers
- **Web Dashboard** — Single-page admin panel with Routes, Usage, Sessions, and Playground tabs

## Architecture

```
┌─────────────────────────────────────────────────┐
│                   ECC Gateway                    │
│                                                  │
│  Claude Code ──► :9090 Proxy                     │
│                   │                              │
│                   ▼                              │
│            ┌──────────┐                          │
│            │ Pipeline  │                          │
│            └────┬─────┘                          │
│                 │                                │
│   Router → Converter → ThinkingRectifier         │
│        → CircuitBreaker → Forwarder              │
│        → UsageTracker → SessionRecorder          │
│                 │                                │
│                 ▼                                │
│          Upstream Provider                       │
│                                                  │
│  Browser ──► :8080 Admin Dashboard               │
└─────────────────────────────────────────────────┘
```

### DDD Layered Crates

| Crate | Layer | Responsibility |
|-------|-------|---------------|
| `ecc-domain` | Domain | Entities, repository traits, error types |
| `ecc-infra` | Infrastructure | SQLite implementations, schema, seeding |
| `ecc-engine` | Engine | Middleware pipeline, ports, protocol converters |
| `ecc-app` | Application | Business services (provider, usage, session) |
| `ecc-api` | API | HTTP servers (proxy + admin) |
| `ecc-web` | Web | Static frontend assets (HTML/CSS/JS) |

### Middleware Pipeline

Every request passes through this pipeline in order:

```
Request
  → Router          — Resolve claude model → provider + target model
  → Converter       — Anthropic ↔ OpenAI protocol translation
  → ThinkingRectifier — Force thinking.type = "enabled"
  → CircuitBreaker  — Skip failing providers
  → Forwarder       — Proxy to upstream, collect streaming chunks
  → UsageTracker    — Record token counts and latency
  → SessionRecorder — Capture full request/response conversation
  → Response
```

## Quick Start

### Prerequisites

- Rust 1.75+ (with cargo)
- SQLite 3.x (bundled via libsqlite3-sys)

### Build & Run

```bash
git clone https://github.com/limushu/new_ecc.git
cd new_ecc
cargo build --release

# Run with defaults
./target/release/ecc

# Or configure via environment variables
ECC_DB_PATH=data/ecc.db ECC_PROXY_PORT=9090 ECC_ADMIN_PORT=8080 ./target/release/ecc
```

### Configure Claude Code

Point Claude Code to the proxy:

```bash
export ANTHROPIC_BASE_URL=http://localhost:9090
```

Or in your shell profile:

```bash
# ~/.zshrc or ~/.bashrc
export ANTHROPIC_BASE_URL=http://localhost:9090
```

Open the dashboard at `http://localhost:8080`, add a provider, and start using Claude Code as usual.

## Configuration

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `ECC_DB_PATH` | `ecc.db` | SQLite database file path |
| `ECC_PROXY_PORT` | `9090` | LLM API proxy listen port |
| `ECC_ADMIN_PORT` | `8080` | Admin dashboard listen port |
| `ECC_CRYPTO_SEED` | `ecc-default-seed` | Encryption seed for sensitive fields |

### Provider Fields

| Field | Description |
|-------|-------------|
| `name` | Unique provider identifier |
| `base_url` | Upstream API endpoint URL |
| `auth_token` | API key or bearer token (encrypted at rest) |
| `auth_type` | `bearer` (default) or `x-api-key` |
| `protocol` | `anthropic` (default) or `openai` |
| `is_coding_plan` | Mark as coding plan for quota tracking |

### Model Mapping

Each route maps a Claude model name to a provider-specific model:

```
claude-sonnet-4-20250514 → provider "deepseek" → model "deepseek-chat-v3"
claude-opus-4-20250514   → provider "openai"   → model "gpt-4.1"
```

Multiple providers can serve the same Claude model. The router selects based on priority and circuit breaker state.

## API Reference

### Providers

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/api/providers` | List all providers |
| `POST` | `/api/providers` | Create a provider |
| `PUT` | `/api/providers/{name}` | Update a provider |
| `DELETE` | `/api/providers/{name}` | Delete a provider |

### Routes

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/api/routes` | List all model routes |

### Presets

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/api/presets` | List provider presets |
| `POST` | `/api/presets` | Create a preset |
| `PUT` | `/api/presets/{name}` | Update a preset |
| `DELETE` | `/api/presets/{name}` | Delete a preset |

### Usage

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/api/usage?days=7` | Usage statistics summary |
| `GET` | `/api/usage/detail?provider=&model=&days=7` | Detailed usage records |

### Sessions

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/api/sessions` | List session groups |
| `GET` | `/api/sessions/{id}` | Get full conversation records |
| `DELETE` | `/api/sessions/{id}` | Delete a session |

### Other

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/api/quota` | Query all provider quotas |
| `POST` | `/api/playground` | Send a test completion request |

## Dashboard

The admin dashboard (`http://localhost:8080`) provides four tabs:

### Routes
Manage providers and model mappings. Add providers from presets or create custom configurations. Each provider card shows model routes, protocol, and auth type.

### Usage
Token usage timeline with bar charts, per-provider breakdown, and detailed records with pagination. Drill down by provider and model. View request counts, input/output tokens, and estimated costs.

### Sessions
Auto-recorded conversation history grouped by session. Each session shows the full conversation in a chat-style layout with:
- User messages (right, accent border)
- Assistant responses (left) with thinking sections (collapsible)
- Tool use summaries ([Edit], [Write], [Read], [Bash]) with code block formatting
- Token usage and latency per turn

Batch select and delete sessions.

### Playground
Send test requests to any configured provider. Configure model, system prompt, max tokens, and streaming mode. Responses stream in real-time.

## License

Private project. All rights reserved.
