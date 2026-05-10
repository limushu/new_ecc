use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use ecc_domain::provider::Provider;
use ecc_domain::repository::{ProviderRepository, QuotaInfo, QuotaTier, RepositoryError};
use tokio::sync::RwLock;

pub struct QuotaService {
    cache: Arc<RwLock<HashMap<String, QuotaInfo>>>,
}

impl QuotaService {
    pub fn new() -> Self {
        Self {
            cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Spawn a background task that refreshes quota data periodically.
    /// Returns after the first refresh completes.
    pub async fn spawn(
        &self,
        client: reqwest::Client,
        provider_repo: Arc<dyn ProviderRepository>,
    ) {
        self.refresh(&client, &*provider_repo).await;
        let cache = self.cache.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(60));
            loop {
                interval.tick().await;
                let providers = match provider_repo.list() {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!("quota refresh: failed to list providers: {e}");
                        continue;
                    }
                };
                let coding: Vec<Provider> =
                    providers.into_iter().filter(|p| p.is_coding_plan).collect();
                let results = query_all_concurrent(&client, &coding).await;
                let mut guard = cache.write().await;
                *guard = results;
            }
        });
    }

    /// Read all cached quota data.
    pub async fn get_all(&self) -> HashMap<String, QuotaInfo> {
        self.cache.read().await.clone()
    }

    /// Read cached quota for a single provider.
    pub async fn get(&self, name: &str) -> Option<QuotaInfo> {
        self.cache.read().await.get(name).cloned()
    }

    /// Query quota for a single provider by name.
    pub async fn query_by_name(
        &self,
        client: &reqwest::Client,
        provider_repo: &dyn ProviderRepository,
        name: &str,
    ) -> Result<QuotaInfo, RepositoryError> {
        let provider = provider_repo
            .get(name)?
            .ok_or_else(|| RepositoryError::NotFound(format!("provider '{name}' not found")))?;
        Ok(query_one(client, &provider).await)
    }

    async fn refresh(
        &self,
        client: &reqwest::Client,
        provider_repo: &dyn ProviderRepository,
    ) {
        let providers = match provider_repo.list() {
            Ok(p) => p,
            Err(e) => {
                tracing::warn!("quota refresh: failed to list providers: {e}");
                return;
            }
        };
        let coding: Vec<Provider> =
            providers.into_iter().filter(|p| p.is_coding_plan).collect();
        let results = query_all_concurrent(client, &coding).await;
        let mut guard = self.cache.write().await;
        *guard = results;
    }
}

/// Query all providers concurrently.
async fn query_all_concurrent(
    client: &reqwest::Client,
    providers: &[Provider],
) -> HashMap<String, QuotaInfo> {
    let futures: Vec<_> = providers
        .iter()
        .map(|p| {
            let client = client.clone();
            let p = p.clone();
            async move {
                let info = query_one(&client, &p).await;
                (p.name.clone(), info)
            }
        })
        .collect();
    let results = futures::future::join_all(futures).await;
    results.into_iter().collect()
}

async fn query_one(client: &reqwest::Client, provider: &Provider) -> QuotaInfo {
    let adapter = match &provider.quota_adapter {
        Some(a) => a,
        None => {
            return QuotaInfo {
                provider_name: provider.name.clone(),
                success: false,
                tiers: vec![],
                error: Some("quota_adapter not configured".into()),
            };
        }
    };

    let mut req = client.get(&adapter.quota_api_url);

    match adapter.auth_style.as_str() {
        "raw" => req = req.header("Authorization", &provider.auth_token),
        _ => req = req.bearer_auth(&provider.auth_token),
    }

    for (k, v) in &adapter.extra_headers {
        req = req.header(k.as_str(), v.as_str());
    }

    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => {
            return QuotaInfo {
                provider_name: provider.name.clone(),
                success: false,
                tiers: vec![],
                error: Some(e.to_string()),
            };
        }
    };

    let status = resp.status();
    let body = match resp.text().await {
        Ok(b) => b,
        Err(e) => {
            return QuotaInfo {
                provider_name: provider.name.clone(),
                success: false,
                tiers: vec![],
                error: Some(format!("read body: {e}")),
            };
        }
    };

    if !status.is_success() {
        return QuotaInfo {
            provider_name: provider.name.clone(),
            success: false,
            tiers: vec![],
            error: Some(format!("upstream {status}: {}", &body[..body.len().min(200)])),
        };
    }

    let json: serde_json::Value = match serde_json::from_str(&body) {
        Ok(v) => v,
        Err(e) => {
            return QuotaInfo {
                provider_name: provider.name.clone(),
                success: false,
                tiers: vec![],
                error: Some(format!("parse json: {e}")),
            };
        }
    };

    // Error check
    if let Some(check) = adapter.response_mapping.get("error_check") {
        if let (Some(field), Some(expected)) = (check.get("field"), check.get("value")) {
            if let Some(actual) = json_path(&json, field.as_str().unwrap_or("")) {
                let is_not_equal = check.get("not_equal").and_then(|v| v.as_bool()).unwrap_or(false);
                let matches = actual == *expected;
                if is_not_equal && matches || !is_not_equal && !matches {
                    return QuotaInfo {
                        provider_name: provider.name.clone(),
                        success: false,
                        tiers: vec![],
                        error: Some(format!("error_check failed: {} == {:?}", field, actual)),
                    };
                }
            }
        }
    }

    let tiers_spec = match adapter.response_mapping.get("tiers") {
        Some(t) => t,
        None => {
            return QuotaInfo {
                provider_name: provider.name.clone(),
                success: true,
                tiers: vec![],
                error: None,
            };
        }
    };

    let tiers = match tiers_spec.as_array() {
        Some(arr) => arr,
        None => {
            return QuotaInfo {
                provider_name: provider.name.clone(),
                success: true,
                tiers: vec![],
                error: None,
            };
        }
    };

    let mut result = vec![];
    for tier in tiers {
        let name = tier.get("name").and_then(|v| v.as_str()).unwrap_or("unknown").to_string();

        // Three modes for computing utilization (0-100 percentage):
        // 1. percentage_path: direct 0-100 value from API
        // 2. limit_path + remaining_path: (1 - remaining/limit) * 100
        // 3. total_path + used_path: (used/total) * 100
        let utilization = if let Some(pct_path) = tier.get("percentage_path").and_then(|v| v.as_str()) {
            json_path_f64(&json, pct_path)
        } else if let Some(limit_path) = tier.get("limit_path").and_then(|v| v.as_str()) {
            let limit = json_path_f64(&json, limit_path);
            let remaining = tier.get("remaining_path").and_then(|v| v.as_str()).map(|p| json_path_f64(&json, p)).unwrap_or(0.0);
            if limit > 0.0 { (1.0 - (remaining / limit)) * 100.0 } else { 0.0 }
        } else {
            let total = tier.get("total_path").and_then(|v| v.as_str()).map(|p| json_path_f64(&json, p)).unwrap_or(0.0);
            let used = tier.get("used_path").and_then(|v| v.as_str()).map(|p| json_path_f64(&json, p)).unwrap_or(0.0);
            if total > 0.0 { (used / total) * 100.0 } else { 0.0 }
        };

        let resets_at = tier.get("resets_at_path").and_then(|v| v.as_str()).and_then(|p| {
            json_path(&json, p).and_then(|v| {
                match v {
                    serde_json::Value::String(s) => Some(s.clone()),
                    serde_json::Value::Number(n) => {
                        let ts = n.as_i64().unwrap_or(0);
                        if ts > 1_000_000_000_000 {
                            DateTime::from_timestamp_millis(ts).map(|dt: DateTime<Utc>| dt.to_rfc3339())
                        } else {
                            DateTime::from_timestamp(ts, 0).map(|dt: DateTime<Utc>| dt.to_rfc3339())
                        }
                    }
                    _ => None,
                }
            })
        });

        result.push(QuotaTier {
            name,
            utilization,
            resets_at,
        });
    }

    QuotaInfo {
        provider_name: provider.name.clone(),
        success: true,
        tiers: result,
        error: None,
    }
}

/// Resolve a dot-notation JSON path like "data.limits[0].detail.limit"
fn json_path<'a>(root: &'a serde_json::Value, path: &str) -> Option<serde_json::Value> {
    let mut current = root;
    for segment in path.split('.') {
        if segment.is_empty() {
            continue;
        }
        // Handle array index: "limits[0]"
        if let Some(bracket) = segment.find('[') {
            let key = &segment[..bracket];
            let idx_str = &segment[bracket + 1..segment.len() - 1];
            let idx: usize = idx_str.parse().ok()?;
            if !key.is_empty() {
                current = current.get(key)?;
            }
            current = current.get(idx)?;
        } else {
            current = current.get(segment)?;
        }
    }
    Some(current.clone())
}

fn json_path_f64(root: &serde_json::Value, path: &str) -> f64 {
    json_path(root, path)
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0)
}
