# ECC v2 Roadmap

> Rust rewrite of [easy_cc_switch](https://github.com/limushu/ccswitch) — route Claude Code requests to any AI provider.

## v0.1 — Foundation (Done)

- [x] Cargo workspace: ecc-config, ecc-core, ecc-gateway, ecc-web
- [x] Provider CRUD (TOML persistence)
- [x] Route CRUD with priority-based failover
- [x] Anthropic ↔ OpenAI protocol conversion
- [x] Proxy forwarding with circuit breaker
- [x] Dashboard UI (dark/light theme, tabs, toast)
- [x] Playground: test provider connectivity
- [x] Usage tracking (JSONL daily rotation)
- [x] Usage charts (Chart.js bar + timeline with drill-down)
- [x] Coding plan quota query (Kimi, Zhipu GLM, MiniMax)
- [x] Quota ring charts on provider cards with 20s cached refresh
- [x] Preset templates (DeepSeek, Kimi, GLM)
- [x] SPA routing with hash persistence

## v0.2 — UX Polish & Detail View (Planned)

### Provider Card Operations
- [ ] `+` button to add model mapping to existing provider
- [ ] `✏️` button to edit provider info (base_url, auth_token, protocol)
- [ ] Inline editing without modal for quick changes

### Route Item Operations
- [ ] `✏️` button to edit model mapping (claude_model, target_model)
- [ ] `📊` button to drill into provider+model usage detail

### Provider+Model Detail Page
- [ ] Request trend chart (time series of requests/tokens/cost)
- [ ] Quota/balance display (5h ring + weekly ring)
- [ ] Recent request list (time, tokens, status, latency)
- [ ] Session/conversation history for this provider+model

### Data Model
- [ ] Usage records indexed by `provider + target_model` combination
- [ ] Session storage: save playground conversations per provider+model
- [ ] API: `GET /api/usage/detail?provider=X&model=Y`

## v0.3 — Production Ready (Future)

- [ ] Token cost calculation from pricing data
- [ ] Provider health monitoring (periodic ping)
- [ ] Rate limiting
- [ ] Config import/export
- [ ] Multi-user support
- [ ] CLI interface for headless operation

## Architecture

```
ecc (binary)
├── ecc-config   — Provider, Route, Preset, Pricing (TOML)
├── ecc-core     — Engine: middleware pipeline, protocol, usage, coding_plan
├── ecc-gateway  — HTTP servers: proxy (4010) + admin (4011)
└── ecc-web      — Embedded static assets (HTML/CSS/JS)
```
