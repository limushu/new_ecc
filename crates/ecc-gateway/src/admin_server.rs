//! Admin server — Dashboard HTML + REST API for configuration and usage.
//!
//! Serves the embedded web dashboard and provides REST endpoints for:
//! - Provider CRUD (`/api/providers`)
//! - Route CRUD (`/api/routes`)
//! - Preset templates (`/api/presets`)
//! - Usage statistics (`/api/usage`)
//! - Playground test requests (`/api/playground`)

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;

use http::StatusCode;
use http_body_util::BodyExt;
use http_body_util::Full;
use hyper::body::Incoming;
use hyper::{Request, Response};

use ecc_config::provider::{Provider, ProviderTable};
use ecc_config::route::{RouteEntry, RouteTable, RouteTarget};
use ecc_core::logging::{
    ADM_PROVIDER_CREATED, ADM_PROVIDER_DELETED, ADM_ROUTE_CREATED,
    ADM_ROUTE_DELETED, ADM_SAVE_ERROR,
};
use ecc_core::{ecc_error, ecc_info};

type BoxBody = Full<bytes::Bytes>;

fn full(data: bytes::Bytes) -> BoxBody {
    Full::new(data)
}

#[derive(Clone)]
pub struct AdminServer {
    providers: Arc<RwLock<ProviderTable>>,
    routes: Arc<RwLock<RouteTable>>,
    providers_path: PathBuf,
    routes_path: PathBuf,
    usage_dir: PathBuf,
    quota_cache: Arc<RwLock<HashMap<String, ecc_core::coding_plan::QuotaResult>>>,
}

impl AdminServer {
    pub fn new(
        providers: Arc<RwLock<ProviderTable>>,
        routes: Arc<RwLock<RouteTable>>,
        providers_path: PathBuf,
        routes_path: PathBuf,
        usage_dir: PathBuf,
    ) -> Self {
        let srv = Self {
            providers: providers.clone(),
            routes,
            providers_path,
            routes_path,
            usage_dir,
            quota_cache: Arc::new(RwLock::new(HashMap::new())),
        };

        // Background quota refresher: every 20s
        let cache = srv.quota_cache.clone();
        tokio::spawn(async move {
            let client = reqwest::Client::new();
            loop {
                tokio::time::sleep(std::time::Duration::from_secs(20)).await;
                let provs = providers.read().await;
                let mut new_cache = HashMap::new();
                for (name, provider) in &provs.providers {
                    if provider.auth_token.trim().is_empty() { continue; }
                    let result = ecc_core::coding_plan::get_quota(
                        &client, &provider.base_url, &provider.auth_token,
                    ).await;
                    new_cache.insert(name.clone(), result);
                }
                drop(provs);
                let mut cache = cache.write().await;
                *cache = new_cache;
            }
        });

        srv
    }

    pub async fn handle(&self, req: Request<Incoming>) -> Response<BoxBody> {
        let path = req.uri().path().to_string();
        let method = req.method().clone();

        // Add CORS headers to all responses
        let mut resp = match (method.as_str(), path.as_str()) {
            // Static files
            ("GET", "/") => self.dashboard(),
            ("GET", "/style.css") => self.style_css(),
            ("GET", "/app.js") => self.app_js(),

            // Providers
            ("GET", "/api/providers") => self.list_providers().await,
            ("POST", "/api/providers") => self.create_provider(req).await,
            ("PUT", p) if p.starts_with("/api/providers/") => {
                let name = p.trim_start_matches("/api/providers/");
                let name = percent_decode(name);
                self.update_provider(req, &name).await
            }
            ("DELETE", p) if p.starts_with("/api/providers/") => {
                let name = p.trim_start_matches("/api/providers/");
                let name = percent_decode(name);
                self.delete_provider(&name).await
            }

            // Routes
            ("GET", "/api/routes") => self.list_routes().await,
            ("POST", "/api/routes") => self.create_route(req).await,
            ("DELETE", p) if p.starts_with("/api/routes/") => {
                let model = p.trim_start_matches("/api/routes/");
                let model = percent_decode(model);
                self.delete_route(&model).await
            }

            // Presets
            ("GET", "/api/presets") => self.list_presets().await,

            // Usage
            ("GET", "/api/usage/detail") => {
                let query = req.uri().query().unwrap_or("");
                self.usage_detail(query).await
            }
            ("GET", p) if p.starts_with("/api/usage") => {
                let query = req.uri().query().unwrap_or("");
                self.usage_stats(query).await
            }

            // Playground
            ("POST", "/api/playground") => self.playground(req).await,

            // Quota (all providers at once)
            ("GET", "/api/quota") => self.query_all_quotas().await,
            ("GET", p) if p.starts_with("/api/quota/") => {
                let name = p.trim_start_matches("/api/quota/");
                let name = percent_decode(name);
                self.query_quota(&name).await
            }

            // CORS
            ("OPTIONS", _) => cors_preflight(),

            // SPA fallback — serve dashboard for any unknown GET path
            ("GET", _) => self.dashboard(),

            _ => error_response(StatusCode::NOT_FOUND, "Not found"),
        };

        // Add CORS headers to all responses
        resp.headers_mut().insert(
            http::header::ACCESS_CONTROL_ALLOW_ORIGIN,
            http::HeaderValue::from_static("*"),
        );
        resp
    }

    // --- Static files ---

    fn dashboard(&self) -> Response<BoxBody> {
        Response::builder()
            .header("content-type", "text/html; charset=utf-8")
            .body(full(bytes::Bytes::from(ecc_web::DASHBOARD_HTML)))
            .unwrap()
    }

    fn style_css(&self) -> Response<BoxBody> {
        Response::builder()
            .header("content-type", "text/css")
            .header("cache-control", "no-cache, no-store, must-revalidate")
            .body(full(bytes::Bytes::from(ecc_web::STYLE_CSS)))
            .unwrap()
    }

    fn app_js(&self) -> Response<BoxBody> {
        Response::builder()
            .header("content-type", "application/javascript")
            .header("cache-control", "no-cache, no-store, must-revalidate")
            .body(full(bytes::Bytes::from(ecc_web::APP_JS)))
            .unwrap()
    }

    // --- Provider CRUD ---

    async fn list_providers(&self) -> Response<BoxBody> {
        let providers = self.providers.read().await;
        let json = serde_json::to_string(&*providers).unwrap_or_default();
        json_response(json)
    }

    async fn create_provider(&self, req: Request<Incoming>) -> Response<BoxBody> {
        let body = match read_body(req).await {
            Ok(b) => b,
            Err(resp) => return resp,
        };

        #[derive(serde::Deserialize)]
        struct CreateProvider {
            name: String,
            base_url: String,
            auth_token: String,
            #[serde(default)]
            auth_type: ecc_config::provider::AuthType,
            #[serde(default)]
            protocol: ecc_config::provider::Protocol,
            #[serde(default)]
            is_coding_plan: bool,
        }

        let input: CreateProvider = match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(e) => return error_response(StatusCode::BAD_REQUEST, &format!("Invalid JSON: {e}")),
        };

        if input.name.is_empty() {
            return error_response(StatusCode::BAD_REQUEST, "Name is required");
        }

        let mut table = self.providers.write().await;
        table.providers.insert(
            input.name.clone(),
            Provider {
                base_url: input.base_url,
                auth_token: input.auth_token,
                auth_type: input.auth_type,
                protocol: input.protocol,
                is_coding_plan: input.is_coding_plan,
            },
        );

        if let Err(e) = ecc_config::provider::save_providers(&self.providers_path, &table) {
            ecc_error!(ADM_SAVE_ERROR, "Failed to save providers: {e}");
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "Failed to save");
        }

        ecc_info!(ADM_PROVIDER_CREATED, provider = %input.name, "provider created");
        json_response_with_status(StatusCode::CREATED, r#"{"ok":true}"#.to_string())
    }

    async fn update_provider(&self, req: Request<Incoming>, name: &str) -> Response<BoxBody> {
        let body = match read_body(req).await {
            Ok(b) => b,
            Err(resp) => return resp,
        };

        #[derive(serde::Deserialize)]
        struct UpdateProvider {
            #[serde(default)]
            base_url: Option<String>,
            #[serde(default)]
            auth_token: Option<String>,
            #[serde(default)]
            auth_type: Option<ecc_config::provider::AuthType>,
            #[serde(default)]
            protocol: Option<ecc_config::provider::Protocol>,
            #[serde(default)]
            is_coding_plan: Option<bool>,
        }

        let input: UpdateProvider = match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(e) => return error_response(StatusCode::BAD_REQUEST, &format!("Invalid JSON: {e}")),
        };

        let mut table = self.providers.write().await;
        let provider = match table.providers.get_mut(name) {
            Some(p) => p,
            None => return error_response(StatusCode::NOT_FOUND, "Provider not found"),
        };
        if let Some(v) = input.base_url { provider.base_url = v; }
        if let Some(v) = input.auth_token { provider.auth_token = v; }
        if let Some(v) = input.auth_type { provider.auth_type = v; }
        if let Some(v) = input.protocol { provider.protocol = v; }
        if let Some(v) = input.is_coding_plan { provider.is_coding_plan = v; }

        if let Err(e) = ecc_config::provider::save_providers(&self.providers_path, &table) {
            ecc_error!(ADM_SAVE_ERROR, "Failed to save providers: {e}");
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "Failed to save");
        }

        json_response(r#"{"ok":true}"#.to_string())
    }

    async fn delete_provider(&self, name: &str) -> Response<BoxBody> {
        let mut table = self.providers.write().await;
        if table.providers.remove(name).is_none() {
            return error_response(StatusCode::NOT_FOUND, "Provider not found");
        }

        // Also remove routes that reference this provider
        let mut routes = self.routes.write().await;
        routes.routes.retain(|_, entry| {
            entry.targets.retain(|t| t.provider != name);
            !entry.targets.is_empty()
        });

        if let Err(e) = ecc_config::provider::save_providers(&self.providers_path, &table) {
            ecc_error!(ADM_SAVE_ERROR, "Failed to save providers: {e}");
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "Failed to save");
        }
        if let Err(e) = ecc_config::route::save_routes(&self.routes_path, &routes) {
            ecc_error!(ADM_SAVE_ERROR, "Failed to save routes: {e}");
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "Failed to save routes");
        }

        ecc_info!(ADM_PROVIDER_DELETED, provider = %name, "provider deleted");
        json_response(r#"{"ok":true}"#.to_string())
    }

    // --- Route CRUD ---

    async fn list_routes(&self) -> Response<BoxBody> {
        let routes = self.routes.read().await;
        let json = serde_json::to_string(&*routes).unwrap_or_default();
        json_response(json)
    }

    async fn create_route(&self, req: Request<Incoming>) -> Response<BoxBody> {
        let body = match read_body(req).await {
            Ok(b) => b,
            Err(resp) => return resp,
        };

        #[derive(serde::Deserialize)]
        struct CreateRoute {
            model: String,
            provider: String,
            target_model: String,
            #[serde(default = "default_priority")]
            priority: u8,
        }

        fn default_priority() -> u8 {
            1
        }

        let input: CreateRoute = match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(e) => return error_response(StatusCode::BAD_REQUEST, &format!("Invalid JSON: {e}")),
        };

        if input.model.is_empty() || input.provider.is_empty() {
            return error_response(StatusCode::BAD_REQUEST, "Model and provider are required");
        }

        // Verify provider exists
        {
            let providers = self.providers.read().await;
            if !providers.providers.contains_key(&input.provider) {
                return error_response(StatusCode::BAD_REQUEST, "Provider not found");
            }
        }

        let model_key = input.model.clone();
        let mut table = self.routes.write().await;
        let target = RouteTarget {
            provider: input.provider,
            model: input.target_model,
            priority: input.priority,
        };

        table
            .routes
            .entry(input.model)
            .and_modify(|entry| {
                entry.targets.push(target.clone());
                entry.targets.sort_by_key(|t| t.priority);
            })
            .or_insert(RouteEntry {
                targets: vec![target],
            });

        if let Err(e) = ecc_config::route::save_routes(&self.routes_path, &table) {
            ecc_error!(ADM_SAVE_ERROR, "Failed to save routes: {e}");
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "Failed to save");
        }

        ecc_info!(ADM_ROUTE_CREATED, model = %model_key, "route created");
        json_response_with_status(StatusCode::CREATED, r#"{"ok":true}"#.to_string())
    }

    async fn delete_route(&self, model: &str) -> Response<BoxBody> {
        let mut table = self.routes.write().await;
        if table.routes.remove(model).is_none() {
            return error_response(StatusCode::NOT_FOUND, "Route not found");
        }

        if let Err(e) = ecc_config::route::save_routes(&self.routes_path, &table) {
            ecc_error!(ADM_SAVE_ERROR, "Failed to save routes: {e}");
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "Failed to save");
        }

        ecc_info!(ADM_ROUTE_DELETED, model = %model, "route deleted");
        json_response(r#"{"ok":true}"#.to_string())
    }

    // --- Presets ---

    async fn list_presets(&self) -> Response<BoxBody> {
        let presets = ecc_config::preset::list_builtin_presets();
        let json = serde_json::to_string(&serde_json::json!({ "presets": presets }))
            .unwrap_or_default();
        json_response(json)
    }

    // --- Usage ---

    async fn usage_stats(&self, query: &str) -> Response<BoxBody> {
        let params: HashMap<String, String> = parse_query(query);
        let date = params.get("date").cloned().unwrap_or_else(|| {
            chrono::Utc::now().format("%Y-%m-%d").to_string()
        });

        let store = ecc_core::usage::UsageStore::new(self.usage_dir.clone(), 0);
        match store.read_daily(&date) {
            Ok(records) => {
                let stats = ecc_core::usage::aggregate_daily(&records);
                let json = serde_json::to_string(&serde_json::json!({
                    "date": date,
                    "total_requests": stats.total_requests,
                    "total_input_tokens": stats.total_input_tokens,
                    "total_output_tokens": stats.total_output_tokens,
                    "total_cost_usd": stats.total_cost_usd,
                    "by_provider": stats.by_provider,
                    "records": records,
                }))
                .unwrap_or_default();
                json_response(json)
            }
            Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &format!("{e}")),
        }
    }

    async fn usage_detail(&self, query: &str) -> Response<BoxBody> {
        let params: HashMap<String, String> = parse_query(query);
        let provider = params.get("provider").cloned().unwrap_or_default();
        let target_model = params.get("model").cloned().unwrap_or_default();
        let days: usize = params.get("days").and_then(|d| d.parse().ok()).unwrap_or(7);

        if provider.is_empty() || target_model.is_empty() {
            return error_response(StatusCode::BAD_REQUEST, "provider and model are required");
        }

        let store = ecc_core::usage::UsageStore::new(self.usage_dir.clone(), 0);
        let today = chrono::Utc::now().date_naive();
        let mut all_records = Vec::new();

        for i in 0..days {
            let date = (today - chrono::TimeDelta::days(i as i64))
                .format("%Y-%m-%d")
                .to_string();
            if let Ok(mut recs) = store.read_daily(&date) {
                all_records.append(&mut recs);
            }
        }

        let filtered: Vec<_> = all_records
            .into_iter()
            .filter(|r| r.provider == provider && r.target_model == target_model)
            .collect();

        // Aggregate by hour for today
        let mut hourly: HashMap<String, serde_json::Value> = HashMap::new();
        let today_str = today.format("%Y-%m-%d").to_string();
        for rec in &filtered {
            if rec.ts.starts_with(&today_str) {
                let hour = if rec.ts.len() >= 13 {
                    rec.ts[11..13].to_string()
                } else {
                    "00".to_string()
                };
                let entry = hourly.entry(hour.clone()).or_insert_with(|| {
                    serde_json::json!({"hour": hour, "requests": 0u64, "input": 0u64, "output": 0u64, "cost": 0.0f64})
                });
                entry["requests"] = serde_json::json!(entry["requests"].as_u64().unwrap() + 1);
                entry["input"] = serde_json::json!(entry["input"].as_u64().unwrap() + rec.input_tokens);
                entry["output"] = serde_json::json!(entry["output"].as_u64().unwrap() + rec.output_tokens);
                entry["cost"] = serde_json::json!(entry["cost"].as_f64().unwrap() + rec.cost_usd);
            }
        }

        // Aggregate by day
        let mut daily: HashMap<String, serde_json::Value> = HashMap::new();
        for rec in &filtered {
            let day = rec.ts[..10].to_string();
            let entry = daily.entry(day.clone()).or_insert_with(|| {
                serde_json::json!({"date": day, "requests": 0u64, "input": 0u64, "output": 0u64, "cost": 0.0f64})
            });
            entry["requests"] = serde_json::json!(entry["requests"].as_u64().unwrap() + 1);
            entry["input"] = serde_json::json!(entry["input"].as_u64().unwrap() + rec.input_tokens);
            entry["output"] = serde_json::json!(entry["output"].as_u64().unwrap() + rec.output_tokens);
            entry["cost"] = serde_json::json!(entry["cost"].as_f64().unwrap() + rec.cost_usd);
        }

        // Summary
        let total_requests = filtered.len() as u64;
        let total_input: u64 = filtered.iter().map(|r| r.input_tokens).sum();
        let total_output: u64 = filtered.iter().map(|r| r.output_tokens).sum();
        let total_cost: f64 = filtered.iter().map(|r| r.cost_usd).sum();

        // Recent records (last 50)
        let mut recent = filtered.clone();
        recent.sort_by(|a, b| b.ts.cmp(&a.ts));
        recent.truncate(50);

        let mut hourly_list: Vec<_> = hourly.into_values().collect();
        hourly_list.sort_by(|a, b| a["hour"].as_str().cmp(&b["hour"].as_str()));

        let mut daily_list: Vec<_> = daily.into_values().collect();
        daily_list.sort_by(|a, b| a["date"].as_str().cmp(&b["date"].as_str()));

        let json = serde_json::json!({
            "provider": provider,
            "target_model": target_model,
            "summary": {
                "total_requests": total_requests,
                "total_input_tokens": total_input,
                "total_output_tokens": total_output,
                "total_cost_usd": total_cost,
            },
            "hourly": hourly_list,
            "daily": daily_list,
            "recent": recent,
        });

        match serde_json::to_string(&json) {
            Ok(s) => json_response(s),
            Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &format!("{e}")),
        }
    }

    // --- Playground ---

    async fn playground(&self, req: Request<Incoming>) -> Response<BoxBody> {
        let body = match read_body(req).await {
            Ok(b) => b,
            Err(resp) => return resp,
        };

        #[derive(serde::Deserialize)]
        struct PlaygroundReq {
            provider: String,
            #[allow(dead_code)]
            model: String,
            target_model: String,
            #[serde(default)]
            message: String,
        }

        let input: PlaygroundReq = match serde_json::from_slice(&body) {
            Ok(v) => v,
            Err(e) => return error_response(StatusCode::BAD_REQUEST, &format!("Invalid JSON: {e}")),
        };

        let providers = self.providers.read().await;
        let provider = match providers.providers.get(&input.provider) {
            Some(p) => p.clone(),
            None => return error_response(StatusCode::NOT_FOUND, "Provider not found"),
        };
        drop(providers);

        let message = if input.message.is_empty() { "Hello" } else { &input.message };

        let client = reqwest::Client::new();
        let start = std::time::Instant::now();
        let resp = crate::playground::test_provider(
            &client,
            &provider.base_url,
            &provider.auth_token,
            &provider.auth_type.to_str(),
            &provider.protocol.to_str(),
            &input.target_model,
            message,
        )
        .await;

        let latency_ms = start.elapsed().as_millis() as u64;
        let status = resp.status().as_u16();
        let (parts, body_bytes) = resp.into_parts();

        // Extract token counts from response
        let (input_tokens, output_tokens, cache_read_tokens) = extract_usage_from_json(&body_bytes);

        // Record usage
        let store = ecc_core::usage::UsageStore::new(self.usage_dir.clone(), 0);
        let _ = store.record(ecc_core::usage::UsageRecord {
            ts: chrono::Utc::now().to_rfc3339(),
            req_id: format!("pg-{:08x}", std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().subsec_nanos()),
            model: input.model.clone(),
            provider: input.provider.clone(),
            target_model: input.target_model.clone(),
            input_tokens,
            cache_read_tokens,
            output_tokens,
            latency_ms,
            status,
            cost_usd: 0.0,
        });
        let _ = store.flush();

        Response::from_parts(parts, full(body_bytes))
    }

    // --- Quota ---

    async fn query_quota(&self, name: &str) -> Response<BoxBody> {
        let cache = self.quota_cache.read().await;
        match cache.get(name) {
            Some(result) => match serde_json::to_string(result) {
                Ok(json) => json_response(json),
                Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &format!("{e}")),
            },
            None => error_response(StatusCode::NOT_FOUND, "No quota data for this provider"),
        }
    }

    async fn query_all_quotas(&self) -> Response<BoxBody> {
        let cache = self.quota_cache.read().await;
        match serde_json::to_string(&*cache) {
            Ok(json) => json_response(json),
            Err(e) => error_response(StatusCode::INTERNAL_SERVER_ERROR, &format!("{e}")),
        }
    }
}

// --- Helpers ---

/// Extract token usage from a JSON response body.
/// Handles both Anthropic format (`usage.input_tokens`) and OpenAI format (`usage.prompt_tokens`).
fn extract_usage_from_json(body: &[u8]) -> (u64, u64, u64) {
    let val: serde_json::Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return (0, 0, 0),
    };
    let usage = match val.get("usage") {
        Some(u) => u,
        None => return (0, 0, 0),
    };
    // Anthropic: input_tokens, output_tokens, cache_read_input_tokens
    // OpenAI: prompt_tokens, completion_tokens
    let input = usage.get("input_tokens")
        .or_else(|| usage.get("prompt_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let output = usage.get("output_tokens")
        .or_else(|| usage.get("completion_tokens"))
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    let cache = usage.get("cache_read_input_tokens")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);
    (input, output, cache)
}

async fn read_body(req: Request<Incoming>) -> Result<bytes::Bytes, Response<BoxBody>> {
    req.into_body()
        .collect()
        .await
        .map(|b| b.to_bytes())
        .map_err(|e| error_response(StatusCode::BAD_REQUEST, &format!("Failed to read body: {e}")))
}

fn json_response(body: String) -> Response<BoxBody> {
    Response::builder()
        .header("content-type", "application/json")
        .body(full(bytes::Bytes::from(body)))
        .unwrap()
}

fn json_response_with_status(status: StatusCode, body: String) -> Response<BoxBody> {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(full(bytes::Bytes::from(body)))
        .unwrap()
}

fn cors_preflight() -> Response<BoxBody> {
    Response::builder()
        .status(StatusCode::OK)
        .header("access-control-allow-origin", "*")
        .header("access-control-allow-methods", "GET, POST, DELETE, OPTIONS")
        .header("access-control-allow-headers", "content-type")
        .body(full(bytes::Bytes::new()))
        .unwrap()
}

fn error_response(status: StatusCode, msg: &str) -> Response<BoxBody> {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(full(bytes::Bytes::from(
            serde_json::json!({"error": msg}).to_string(),
        )))
        .unwrap()
}

fn percent_decode(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.bytes();
    while let Some(b) = chars.next() {
        if b == b'%' {
            let hi = chars.next().unwrap_or(b'0');
            let lo = chars.next().unwrap_or(b'0');
            let val = hex_val(hi) << 4 | hex_val(lo);
            result.push(val as char);
        } else if b == b'+' {
            result.push(' ');
        } else {
            result.push(b as char);
        }
    }
    result
}

fn hex_val(b: u8) -> u8 {
    match b {
        b'0'..=b'9' => b - b'0',
        b'a'..=b'f' => b - b'a' + 10,
        b'A'..=b'F' => b - b'A' + 10,
        _ => 0,
    }
}

fn parse_query(query: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for pair in query.split('&') {
        if let Some((k, v)) = pair.split_once('=') {
            map.insert(k.to_string(), v.to_string());
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_admin_with_dir() -> (AdminServer, tempfile::TempDir) {
        let dir = tempfile::tempdir().unwrap();
        let providers = Arc::new(RwLock::new(ProviderTable::default()));
        let routes = Arc::new(RwLock::new(RouteTable::default()));
        let admin = AdminServer::new(
            providers,
            routes,
            dir.path().join("providers.toml"),
            dir.path().join("routes.toml"),
            dir.path().join("usage"),
        );
        (admin, dir)
    }

    async fn body_string(resp: Response<BoxBody>) -> String {
        let bytes = resp.into_body().collect().await.unwrap().to_bytes();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn t51_dashboard_html() {
        let (admin, _dir) = make_admin_with_dir();
        let resp = admin.dashboard();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        assert!(body.contains("<html"), "Dashboard should contain <html>");
    }

    #[tokio::test]
    async fn t52_list_providers_empty() {
        let (admin, _dir) = make_admin_with_dir();
        let resp = admin.list_providers().await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        let val: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(val["providers"].as_object().unwrap().is_empty());
    }

    #[tokio::test]
    async fn t53_create_and_list_provider() {
        let (admin, dir) = make_admin_with_dir();

        // Directly test create logic by writing to the table
        let mut table = admin.providers.write().await;
        table.providers.insert(
            "deepseek".to_string(),
            Provider {
                base_url: "https://api.deepseek.com".to_string(),
                auth_token: "sk-test-123".to_string(),
                auth_type: ecc_config::provider::AuthType::Bearer,
                protocol: ecc_config::provider::Protocol::OpenAI,
                is_coding_plan: false,
            },
        );
        ecc_config::provider::save_providers(&dir.path().join("providers.toml"), &table).unwrap();
        drop(table);

        // Verify list returns it
        let resp = admin.list_providers().await;
        let body = body_string(resp).await;
        let val: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(val["providers"].as_object().unwrap().contains_key("deepseek"));

        // Verify persisted to disk
        let loaded = ecc_config::provider::load_providers(&dir.path().join("providers.toml")).unwrap();
        assert_eq!(loaded.providers["deepseek"].base_url, "https://api.deepseek.com");
    }

    #[tokio::test]
    async fn t53_delete_provider_cascades_routes() {
        let (admin, _dir) = make_admin_with_dir();

        // Set up provider
        let mut ptable = ProviderTable::default();
        ptable.providers.insert(
            "test-provider".to_string(),
            Provider {
                base_url: "https://api.test.com".to_string(),
                auth_token: "sk-test".to_string(),
                auth_type: ecc_config::provider::AuthType::Bearer,
                protocol: ecc_config::provider::Protocol::OpenAI,
                is_coding_plan: false,
            },
        );
        *admin.providers.write().await = ptable;

        // Set up route referencing this provider
        let mut route_table = RouteTable::default();
        route_table.routes.insert(
            "claude-sonnet-4-6".to_string(),
            RouteEntry {
                targets: vec![RouteTarget {
                    provider: "test-provider".to_string(),
                    model: "test-model".to_string(),
                    priority: 1,
                }],
            },
        );
        *admin.routes.write().await = route_table;

        // Delete provider
        let resp = admin.delete_provider("test-provider").await;
        assert_eq!(resp.status(), StatusCode::OK);

        // Provider removed
        let loaded = admin.providers.read().await;
        assert!(!loaded.providers.contains_key("test-provider"));

        // Routes referencing this provider also removed
        let routes = admin.routes.read().await;
        assert!(routes.routes.is_empty());
    }

    #[tokio::test]
    async fn t53_delete_provider_not_found() {
        let (admin, _dir) = make_admin_with_dir();
        let resp = admin.delete_provider("nonexistent").await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn t54_list_routes_empty() {
        let (admin, _dir) = make_admin_with_dir();
        let resp = admin.list_routes().await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        let val: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(val["routes"].as_object().unwrap().is_empty());
    }

    #[tokio::test]
    async fn t54_list_routes_with_data() {
        let (admin, _dir) = make_admin_with_dir();
        let mut route_table = RouteTable::default();
        route_table.routes.insert(
            "claude-sonnet-4-6".to_string(),
            RouteEntry {
                targets: vec![RouteTarget {
                    provider: "deepseek".to_string(),
                    model: "deepseek-chat".to_string(),
                    priority: 1,
                }],
            },
        );
        *admin.routes.write().await = route_table;

        let resp = admin.list_routes().await;
        let body = body_string(resp).await;
        let val: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert!(val["routes"].as_object().unwrap().contains_key("claude-sonnet-4-6"));
    }

    #[tokio::test]
    async fn t55_delete_route() {
        let (admin, _dir) = make_admin_with_dir();

        let mut route_table = RouteTable::default();
        route_table.routes.insert(
            "claude-sonnet-4-6".to_string(),
            RouteEntry {
                targets: vec![RouteTarget {
                    provider: "deepseek".to_string(),
                    model: "deepseek-chat".to_string(),
                    priority: 1,
                }],
            },
        );
        *admin.routes.write().await = route_table;

        let resp = admin.delete_route("claude-sonnet-4-6").await;
        assert_eq!(resp.status(), StatusCode::OK);

        let routes = admin.routes.read().await;
        assert!(!routes.routes.contains_key("claude-sonnet-4-6"));
    }

    #[tokio::test]
    async fn t55_delete_route_not_found() {
        let (admin, _dir) = make_admin_with_dir();
        let resp = admin.delete_route("nonexistent").await;
        assert_eq!(resp.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn t_list_presets() {
        let (admin, _dir) = make_admin_with_dir();
        let resp = admin.list_presets().await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        let val: serde_json::Value = serde_json::from_str(&body).unwrap();
        let presets = val["presets"].as_array().unwrap();
        assert!(presets.len() >= 3, "Should have at least 3 built-in presets");
        let names: Vec<&str> = presets.iter().filter_map(|p| p["name"].as_str()).collect();
        assert!(names.contains(&"DeepSeek"));
        assert!(names.contains(&"Kimi"));
        assert!(names.contains(&"GLM"));
    }

    #[tokio::test]
    async fn t_usage_stats_empty() {
        let (admin, _dir) = make_admin_with_dir();
        let resp = admin.usage_stats("date=2026-05-03").await;
        assert_eq!(resp.status(), StatusCode::OK);
        let body = body_string(resp).await;
        let val: serde_json::Value = serde_json::from_str(&body).unwrap();
        assert_eq!(val["total_requests"], 0);
    }

    #[test]
    fn t50_options_cors() {
        let resp = cors_preflight();
        assert_eq!(resp.status(), StatusCode::OK);
        assert!(resp.headers().contains_key("access-control-allow-origin"));
    }

    #[test]
    fn t_percent_decode_spaces() {
        assert_eq!(percent_decode("hello%20world"), "hello world");
        assert_eq!(percent_decode("claude-sonnet-4-6"), "claude-sonnet-4-6");
        assert_eq!(percent_decode("my%2Fmodel"), "my/model");
    }

    #[test]
    fn t_parse_query() {
        let map = parse_query("date=2026-05-03&foo=bar");
        assert_eq!(map.get("date").unwrap(), "2026-05-03");
        assert_eq!(map.get("foo").unwrap(), "bar");
    }

    #[test]
    fn t_parse_query_empty() {
        let map = parse_query("");
        assert!(map.is_empty());
    }
}
