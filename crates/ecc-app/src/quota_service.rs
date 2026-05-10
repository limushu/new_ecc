use std::collections::HashMap;

use chrono::{DateTime, Utc};
use ecc_domain::provider::Provider;
use ecc_domain::repository::{QuotaInfo, QuotaTier};

pub struct QuotaService;

impl QuotaService {
    pub fn new() -> Self {
        Self
    }

    pub async fn query(
        &self,
        client: &reqwest::Client,
        provider: &Provider,
    ) -> QuotaInfo {
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

            let (limit, remaining, used) = if let Some(limit_path) = tier.get("limit_path").and_then(|v| v.as_str()) {
                let limit = json_path_f64(&json, limit_path);
                let remaining = tier.get("remaining_path").and_then(|v| v.as_str()).map(|p| json_path_f64(&json, p)).unwrap_or(0.0);
                (limit, remaining, 0.0)
            } else {
                let total = tier.get("total_path").and_then(|v| v.as_str()).map(|p| json_path_f64(&json, p)).unwrap_or(0.0);
                let used = tier.get("used_path").and_then(|v| v.as_str()).map(|p| json_path_f64(&json, p)).unwrap_or(0.0);
                (total, 0.0, used)
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

            let utilization = if limit > 0.0 && remaining > 0.0 {
                1.0 - (remaining / limit)
            } else if limit > 0.0 {
                used / limit
            } else {
                0.0
            };

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

    pub async fn query_all<'a>(
        &self,
        client: &reqwest::Client,
        providers: impl Iterator<Item = &'a Provider>,
    ) -> HashMap<String, QuotaInfo> {
        let mut results = HashMap::new();
        for provider in providers {
            let info = self.query(client, provider).await;
            results.insert(provider.name.clone(), info);
        }
        results
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
