//! Coding Plan quota query — Kimi, Zhipu GLM, MiniMax.
//!
//! Each provider is a declarative config entry. The shared query flow handles
//! HTTP request, auth header, and common error handling. Provider-specific
//! logic is limited to response parsing only.

use serde::Serialize;

// ── Data types ────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize)]
pub struct QuotaTier {
    pub name: String,
    pub utilization: f64,
    pub resets_at: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct QuotaResult {
    pub provider: String,
    pub success: bool,
    pub tiers: Vec<QuotaTier>,
    pub error: Option<String>,
}

// ── Provider config ───────────────────────────────────────────────────────

enum AuthStyle {
    Bearer,
    Raw,
}

struct ProviderConfig {
    id: &'static str,
    url: &'static str,
    auth: AuthStyle,
    extra_headers: &'static [(&'static str, &'static str)],
    parse: fn(&serde_json::Value) -> Vec<QuotaTier>,
}

fn kimi_parse(body: &serde_json::Value) -> Vec<QuotaTier> {
    let mut tiers = Vec::new();
    if let Some(limits) = body.get("limits").and_then(|v| v.as_array()) {
        for item in limits {
            if let Some(detail) = item.get("detail") {
                let limit = detail.get("limit").and_then(parse_f64).unwrap_or(1.0);
                let remaining = detail.get("remaining").and_then(parse_f64).unwrap_or(0.0);
                let resets_at = detail.get("resetTime").and_then(extract_reset_time);
                let used = (limit - remaining).max(0.0);
                let utilization = if limit > 0.0 { (used / limit) * 100.0 } else { 0.0 };
                tiers.push(QuotaTier { name: "five_hour".into(), utilization, resets_at });
            }
        }
    }
    if let Some(usage) = body.get("usage") {
        let limit = usage.get("limit").and_then(parse_f64).unwrap_or(1.0);
        let remaining = usage.get("remaining").and_then(parse_f64).unwrap_or(0.0);
        let resets_at = usage.get("resetTime").and_then(extract_reset_time);
        let used = (limit - remaining).max(0.0);
        let utilization = if limit > 0.0 { (used / limit) * 100.0 } else { 0.0 };
        tiers.push(QuotaTier { name: "weekly_limit".into(), utilization, resets_at });
    }
    tiers
}

fn zhipu_parse(body: &serde_json::Value) -> Vec<QuotaTier> {
    // Zhipu wraps in { success, data: { limits: [...] } }
    let data = match body.get("data") {
        Some(d) => d,
        None => return vec![],
    };
    let mut entries: Vec<(i64, f64, Option<String>)> = Vec::new();
    if let Some(limits) = data.get("limits").and_then(|v| v.as_array()) {
        for item in limits {
            let t = item.get("type").and_then(|v| v.as_str()).unwrap_or("");
            if !t.eq_ignore_ascii_case("TOKENS_LIMIT") { continue; }
            let pct = item.get("percentage").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let reset_ms = item.get("nextResetTime").and_then(|v| v.as_i64()).unwrap_or(i64::MAX);
            let reset_iso = if reset_ms == i64::MAX { None } else { millis_to_iso8601(reset_ms) };
            entries.push((reset_ms, pct, reset_iso));
        }
    }
    entries.sort_by_key(|(ms, _, _)| *ms);
    entries.into_iter().take(2).enumerate().map(|(i, (_, pct, resets_at))| QuotaTier {
        name: if i == 0 { "five_hour" } else { "weekly_limit" }.into(),
        utilization: pct,
        resets_at,
    }).collect()
}

fn minimax_parse(body: &serde_json::Value) -> Vec<QuotaTier> {
    let mut tiers = Vec::new();
    if let Some(items) = body.get("model_remains").and_then(|v| v.as_array()) {
        if let Some(item) = items.first() {
            let total = item.get("current_interval_total_count").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let used = item.get("current_interval_usage_count").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let end = item.get("end_time").and_then(|v| v.as_i64());
            if total > 0.0 {
                tiers.push(QuotaTier {
                    name: "five_hour".into(),
                    utilization: ((total - used) / total) * 100.0,
                    resets_at: end.and_then(millis_to_iso8601),
                });
            }
            let w_total = item.get("current_weekly_total_count").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let w_used = item.get("current_weekly_usage_count").and_then(|v| v.as_f64()).unwrap_or(0.0);
            let w_end = item.get("weekly_end_time").and_then(|v| v.as_i64());
            if w_total > 0.0 {
                tiers.push(QuotaTier {
                    name: "weekly_limit".into(),
                    utilization: ((w_total - w_used) / w_total) * 100.0,
                    resets_at: w_end.and_then(millis_to_iso8601),
                });
            }
        }
    }
    tiers
}

/// Provider registry — base_url substring → config
fn provider_for(base_url: &str) -> Option<&'static ProviderConfig> {
    static PROVIDERS: &[(&str, ProviderConfig)] = &[
        ("api.kimi.com/coding", ProviderConfig {
            id: "kimi",
            url: "https://api.kimi.com/coding/v1/usages",
            auth: AuthStyle::Bearer,
            extra_headers: &[("Accept", "application/json")],
            parse: kimi_parse,
        }),
        ("bigmodel.cn", ProviderConfig {
            id: "zhipu",
            url: "https://api.z.ai/api/monitor/usage/quota/limit",
            auth: AuthStyle::Raw,
            extra_headers: &[("Content-Type", "application/json"), ("Accept-Language", "en-US,en")],
            parse: zhipu_parse,
        }),
        ("api.z.ai", ProviderConfig {
            id: "zhipu",
            url: "https://api.z.ai/api/monitor/usage/quota/limit",
            auth: AuthStyle::Raw,
            extra_headers: &[("Content-Type", "application/json"), ("Accept-Language", "en-US,en")],
            parse: zhipu_parse,
        }),
        ("api.minimaxi.com", ProviderConfig {
            id: "minimax",
            url: "https://api.minimaxi.com/v1/api/openplatform/coding_plan/remains",
            auth: AuthStyle::Bearer,
            extra_headers: &[("Content-Type", "application/json")],
            parse: minimax_parse,
        }),
        ("api.minimax.io", ProviderConfig {
            id: "minimax",
            url: "https://api.minimax.io/v1/api/openplatform/coding_plan/remains",
            auth: AuthStyle::Bearer,
            extra_headers: &[("Content-Type", "application/json")],
            parse: minimax_parse,
        }),
    ];
    let lower = base_url.to_lowercase();
    PROVIDERS.iter().find(|(pat, _)| lower.contains(pat)).map(|(_, cfg)| cfg)
}

// ── Shared helpers ────────────────────────────────────────────────────────

fn millis_to_iso8601(ms: i64) -> Option<String> {
    chrono::DateTime::from_timestamp(ms / 1000, ((ms % 1000) * 1_000_000) as u32)
        .map(|dt| dt.to_rfc3339())
}

fn extract_reset_time(value: &serde_json::Value) -> Option<String> {
    value.as_str().map(|s| s.to_string()).or_else(|| {
        value.as_i64().and_then(|n| {
            let ms = if n < 1_000_000_000_000 { n * 1000 } else { n };
            millis_to_iso8601(ms)
        })
    })
}

fn parse_f64(value: &serde_json::Value) -> Option<f64> {
    value.as_f64().or_else(|| value.as_str().and_then(|s| s.parse().ok()))
}

// ── Public entry ──────────────────────────────────────────────────────────

pub async fn get_quota(
    client: &reqwest::Client,
    base_url: &str,
    api_key: &str,
) -> QuotaResult {
    if api_key.trim().is_empty() {
        return QuotaResult {
            provider: "unknown".into(), success: false, tiers: vec![],
            error: Some("No API key configured".into()),
        };
    }

    let cfg = match provider_for(base_url) {
        Some(c) => c,
        None => return QuotaResult {
            provider: "unknown".into(), success: false, tiers: vec![],
            error: Some("Provider does not support coding plan queries".into()),
        },
    };

    let mut req = client.get(cfg.url).timeout(std::time::Duration::from_secs(10));
    req = match cfg.auth {
        AuthStyle::Bearer => req.header("Authorization", format!("Bearer {api_key}")),
        AuthStyle::Raw => req.header("Authorization", api_key),
    };
    for (k, v) in cfg.extra_headers {
        req = req.header(*k, *v);
    }

    let resp = match req.send().await {
        Ok(r) => r,
        Err(e) => return QuotaResult {
            provider: cfg.id.into(), success: false, tiers: vec![],
            error: Some(format!("Network error: {e}")),
        },
    };

    let status = resp.status();
    if !status.is_success() {
        let body = resp.text().await.unwrap_or_default();
        return QuotaResult {
            provider: cfg.id.into(), success: false, tiers: vec![],
            error: Some(format!("API error (HTTP {status}): {body}")),
        };
    }

    let body: serde_json::Value = match resp.json().await {
        Ok(v) => v,
        Err(e) => return QuotaResult {
            provider: cfg.id.into(), success: false, tiers: vec![],
            error: Some(format!("Parse error: {e}")),
        },
    };

    // Zhipu wraps errors in { success: false, msg: "..." }
    if body.get("success").and_then(|v| v.as_bool()) == Some(false) {
        let msg = body.get("msg").and_then(|v| v.as_str()).unwrap_or("Unknown error");
        return QuotaResult {
            provider: cfg.id.into(), success: false, tiers: vec![],
            error: Some(format!("API error: {msg}")),
        };
    }
    // MiniMax wraps errors in { base_resp: { status_code: non-zero } }
    if let Some(br) = body.get("base_resp") {
        let code = br.get("status_code").and_then(|v| v.as_i64()).unwrap_or(-1);
        if code != 0 {
            let msg = br.get("status_msg").and_then(|v| v.as_str()).unwrap_or("Unknown error");
            return QuotaResult {
                provider: cfg.id.into(), success: false, tiers: vec![],
                error: Some(format!("API error (code {code}): {msg}")),
            };
        }
    }

    QuotaResult {
        provider: cfg.id.into(),
        success: true,
        tiers: (cfg.parse)(&body),
        error: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detect_kimi() { assert!(provider_for("https://api.kimi.com/coding/").is_some()); }

    #[test]
    fn detect_zhipu() { assert!(provider_for("https://open.bigmodel.cn/api/anthropic").is_some()); }

    #[test]
    fn detect_minimax() { assert!(provider_for("https://api.minimaxi.com/v1").is_some()); }

    #[test]
    fn detect_unknown() { assert!(provider_for("https://api.deepseek.com").is_none()); }

    #[test]
    fn kimi_parse_limits_and_usage() {
        let body = serde_json::json!({
            "limits": [{ "detail": { "limit": 100, "remaining": 75, "resetTime": "2026-05-05T00:00:00Z" } }],
            "usage": { "limit": 500, "remaining": 400, "resetTime": "2026-05-11T00:00:00Z" }
        });
        let tiers = kimi_parse(&body);
        assert_eq!(tiers.len(), 2);
        assert_eq!(tiers[0].name, "five_hour");
        assert_eq!(tiers[0].utilization, 25.0);
        assert_eq!(tiers[1].name, "weekly_limit");
        assert_eq!(tiers[1].utilization, 20.0);
    }

    #[test]
    fn zhipu_parse_two_tiers_sorted() {
        let body = serde_json::json!({
            "success": true,
            "data": { "limits": [
                { "type": "TOKENS_LIMIT", "percentage": 53.0, "nextResetTime": 2_000_000_000_000_i64 },
                { "type": "TOKENS_LIMIT", "percentage": 44.0, "nextResetTime": 1_000_000_000_000_i64 },
                { "type": "TIME_LIMIT", "percentage": 7.0 }
            ]}
        });
        let tiers = zhipu_parse(&body);
        assert_eq!(tiers.len(), 2);
        assert_eq!(tiers[0].name, "five_hour");
        assert_eq!(tiers[0].utilization, 44.0);
        assert_eq!(tiers[1].name, "weekly_limit");
        assert_eq!(tiers[1].utilization, 53.0);
    }

    #[test]
    fn minimax_parse_single_model() {
        let body = serde_json::json!({
            "model_remains": [{ "current_interval_total_count": 100, "current_interval_usage_count": 30, "end_time": 1_700_000_000_000_i64 }]
        });
        let tiers = minimax_parse(&body);
        assert_eq!(tiers.len(), 1);
        assert_eq!(tiers[0].name, "five_hour");
        assert_eq!(tiers[0].utilization, 70.0);
    }
}
