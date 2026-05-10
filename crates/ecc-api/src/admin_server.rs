use std::collections::HashMap;
use std::sync::Arc;

use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use hyper::body::Incoming;
use hyper::{Request, Response};
use tokio::sync::RwLock;

use ecc_app::provider_service::{CreateProviderCommand, ProviderService, UpdateProviderCommand};
use ecc_app::preset_service::PresetService;
use ecc_app::quota_service::QuotaService;
use ecc_app::usage_service::UsageService;
use ecc_app::PlaygroundService;
use ecc_app::{PlaygroundRequest, PlaygroundResponse};
use ecc_domain::repository::{
    ConfigRepository, PresetRepository, ProviderRepository, QuotaInfo, RouteRepository,
    UsageRepository,
};

type BoxBody = Full<Bytes>;

pub struct AdminServer<P, C, R, U, PR>
where
    P: ProviderRepository,
    C: ConfigRepository,
    R: RouteRepository,
    U: UsageRepository,
    PR: PresetRepository,
{
    provider_service: Arc<ProviderService<P, C, R>>,
    preset_service: Arc<PresetService<PR>>,
    usage_service: Arc<UsageService<U>>,
    #[allow(dead_code)]
    quota_service: Arc<QuotaService>,
    playground_service: Arc<PlaygroundService>,
    quota_cache: Arc<RwLock<HashMap<String, QuotaInfo>>>,
    client: reqwest::Client,
}

impl<P, C, R, U, PR> AdminServer<P, C, R, U, PR>
where
    P: ProviderRepository + 'static,
    C: ConfigRepository + 'static,
    R: RouteRepository + 'static,
    U: UsageRepository + 'static,
    PR: PresetRepository + 'static,
{
    pub fn new(
        provider_service: Arc<ProviderService<P, C, R>>,
        preset_service: Arc<PresetService<PR>>,
        usage_service: Arc<UsageService<U>>,
        quota_service: Arc<QuotaService>,
        playground_service: Arc<PlaygroundService>,
        client: reqwest::Client,
    ) -> Self {
        Self {
            provider_service,
            preset_service,
            usage_service,
            quota_service,
            playground_service,
            quota_cache: Arc::new(RwLock::new(HashMap::new())),
            client,
        }
    }

    pub async fn handle(&self, req: Request<Incoming>) -> Response<BoxBody> {
        let (parts, body) = req.into_parts();
        let path = parts.uri.path().to_string();

        if parts.method == "OPTIONS" {
            return cors_ok();
        }

        let body_bytes = body
            .collect()
            .await
            .map(|b| b.to_bytes())
            .unwrap_or_default();

        let resp = match Route::match_path(&parts.method, &path) {
            // Static
            Route::Dashboard => serve(ecc_web::DASHBOARD_HTML, "text/html"),
            Route::StyleCss => serve(ecc_web::STYLE_CSS, "text/css"),
            Route::AppJs => serve(ecc_web::APP_JS, "application/javascript"),

            // Providers
            Route::ListProviders => {
                handle_result(self.provider_service.list_providers())
            }
            Route::CreateProvider => {
                let cmd = parse_body::<CreateProviderCommand>(&body_bytes);
                handle_result(cmd.and_then(|c| self.provider_service.create_provider(c)))
            }
            Route::UpdateProvider(name) => {
                let cmd = parse_body::<UpdateProviderCommand>(&body_bytes);
                handle_result(cmd.and_then(|c| self.provider_service.update_provider(&name, c)))
            }
            Route::DeleteProvider(name) => {
                handle_result(self.provider_service.delete_provider(&name))
            }

            // Presets
            Route::ListPresets => {
                handle_result(self.preset_service.list_presets())
            }
            Route::CreatePreset => {
                let preset = parse_body::<ecc_domain::preset::Preset>(&body_bytes);
                handle_result(preset.and_then(|p| self.preset_service.create_preset(p)))
            }
            Route::UpdatePreset(name) => {
                let preset = parse_body::<ecc_domain::preset::Preset>(&body_bytes);
                handle_result(preset.and_then(|mut p| {
                    p.name = name.clone();
                    self.preset_service.update_preset(&name, p)
                }))
            }
            Route::DeletePreset(name) => {
                handle_result(self.preset_service.delete_preset(&name))
            }

            // Routes (read-only, derived from providers)
            Route::ListRoutes => {
                handle_result(self.provider_service.list_providers().map(|list| {
                    list.iter()
                        .map(|p| (p.name.clone(), p.model_mappings.clone()))
                        .collect::<HashMap<_, _>>()
                }))
            }

            // Usage
            Route::UsageStats => {
                let end = chrono::Utc::now();
                let start = end - chrono::Duration::days(30);
                handle_result(self.usage_service.aggregate(&start, &end))
            }
            Route::UsageDetail => {
                let params = parse_query(parts.uri.query().unwrap_or(""));
                let provider = params.get("provider").map(|s| s.as_str()).unwrap_or("");
                let model = params.get("model").map(|s| s.as_str()).unwrap_or("");
                let days: u32 = params.get("days").and_then(|s| s.parse().ok()).unwrap_or(7);
                let end = chrono::Utc::now();
                let start = end - chrono::Duration::days(days as i64);
                handle_result(
                    self.usage_service
                        .query(&start, &end)
                        .map(|records| records.into_iter().filter(|r| {
                            (provider.is_empty() || r.provider_name == provider)
                                && (model.is_empty() || r.target_model == model)
                        }).collect::<Vec<_>>()),
                )
            }

            // Quota (from cache)
            Route::QueryAllQuotas => {
                let cache = self.quota_cache.read().await;
                json_ok(&*cache)
            }
            Route::QueryQuota(name) => {
                let cache = self.quota_cache.read().await;
                match cache.get(&name) {
                    Some(info) => json_ok(info),
                    None => json_error(404, &format!("quota for '{name}' not cached")),
                }
            }

            // Playground
            Route::Playground => {
                let req = parse_body::<PlaygroundRequest>(&body_bytes);
                match req {
                    Ok(r) => {
                        match self.playground_service.test_by_name(
                            &self.client,
                            self.provider_service.provider_repo(),
                            &r.provider,
                            &r.model,
                            &r.message,
                        ).await {
                            Ok(result) => json_ok(&PlaygroundResponse {
                                status: result.status,
                                body: String::from_utf8_lossy(&result.body).to_string(),
                                latency_ms: result.latency_ms,
                                input_tokens: result.input_tokens,
                                output_tokens: result.output_tokens,
                                cache_read_tokens: result.cache_read_tokens,
                            }),
                            Err(e) => json_error(400, &e.to_string()),
                        }
                    }
                    Err(e) => json_error(400, &e.to_string()),
                }
            }

            Route::NotFound => json_error(404, "not found"),
        };

        with_cors(resp)
    }
}

// -- Routing --

enum Route {
    Dashboard,
    StyleCss,
    AppJs,
    ListProviders,
    CreateProvider,
    UpdateProvider(String),
    DeleteProvider(String),
    ListPresets,
    CreatePreset,
    UpdatePreset(String),
    DeletePreset(String),
    ListRoutes,
    UsageStats,
    UsageDetail,
    QueryAllQuotas,
    QueryQuota(String),
    Playground,
    NotFound,
}

impl Route {
    fn match_path(method: &http::Method, path: &str) -> Self {
        match (method.as_str(), path) {
            ("GET", "/") => Self::Dashboard,
            ("GET", "/style.css") => Self::StyleCss,
            ("GET", "/app.js") => Self::AppJs,
            ("GET", "/api/providers") => Self::ListProviders,
            ("POST", "/api/providers") => Self::CreateProvider,
            ("GET", "/api/routes") => Self::ListRoutes,
            ("GET", "/api/presets") => Self::ListPresets,
            ("POST", "/api/presets") => Self::CreatePreset,
            ("GET", "/api/usage") => Self::UsageStats,
            ("GET", p) if p.starts_with("/api/usage/detail") => Self::UsageDetail,
            ("GET", "/api/quota") => Self::QueryAllQuotas,
            ("POST", "/api/playground") => Self::Playground,
            ("PUT", p) if p.starts_with("/api/providers/") => {
                Self::UpdateProvider(p.trim_start_matches("/api/providers/").to_string())
            }
            ("DELETE", p) if p.starts_with("/api/providers/") => {
                Self::DeleteProvider(p.trim_start_matches("/api/providers/").to_string())
            }
            ("PUT", p) if p.starts_with("/api/presets/") => {
                Self::UpdatePreset(p.trim_start_matches("/api/presets/").to_string())
            }
            ("DELETE", p) if p.starts_with("/api/presets/") => {
                Self::DeletePreset(p.trim_start_matches("/api/presets/").to_string())
            }
            ("GET", p) if p.starts_with("/api/quota/") => {
                Self::QueryQuota(p.trim_start_matches("/api/quota/").to_string())
            }
            _ => Self::NotFound,
        }
    }
}

// -- Helpers --

fn serve(content: &str, content_type: &str) -> Response<BoxBody> {
    Response::builder()
        .header("content-type", format!("{content_type}; charset=utf-8"))
        .body(Full::new(Bytes::from(content.to_string())))
        .unwrap()
}

fn json_ok(data: &impl serde::Serialize) -> Response<BoxBody> {
    Response::builder()
        .status(200)
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(serde_json::to_string(data).unwrap_or_default())))
        .unwrap()
}

fn json_error(status: u16, msg: &str) -> Response<BoxBody> {
    Response::builder()
        .status(status)
        .header("content-type", "application/json")
        .body(Full::new(Bytes::from(
            serde_json::json!({ "error": msg }).to_string(),
        )))
        .unwrap()
}

fn handle_result<T: serde::Serialize>(
    result: Result<T, ecc_domain::repository::RepositoryError>,
) -> Response<BoxBody> {
    match result {
        Ok(data) => json_ok(&data),
        Err(e) => json_error(500, &e.to_string()),
    }
}

fn with_cors(mut resp: Response<BoxBody>) -> Response<BoxBody> {
    let h = resp.headers_mut();
    h.insert("access-control-allow-origin", "*".parse().unwrap());
    h.insert("access-control-allow-methods", "GET, POST, PUT, DELETE, OPTIONS".parse().unwrap());
    h.insert("access-control-allow-headers", "content-type, authorization, x-api-key, anthropic-version".parse().unwrap());
    resp
}

fn cors_ok() -> Response<BoxBody> {
    with_cors(Response::builder().status(204).body(Full::new(Bytes::new())).unwrap())
}

fn parse_query(query: &str) -> HashMap<String, String> {
    query
        .split('&')
        .filter_map(|pair| {
            let mut kv = pair.splitn(2, '=');
            Some((kv.next()?.to_string(), kv.next()?.to_string()))
        })
        .collect()
}

fn parse_body<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, ecc_domain::repository::RepositoryError> {
    serde_json::from_slice(bytes).map_err(|e| ecc_domain::repository::RepositoryError::Storage(e.into()))
}
