use bytes::Bytes;
use ecc_domain::provider::{AuthType, Protocol, Provider};

#[derive(serde::Deserialize)]
pub struct PlaygroundRequest {
    pub provider: String,
    pub model: String,
    pub message: String,
}

#[derive(serde::Serialize)]
pub struct PlaygroundResponse {
    pub status: u16,
    pub body: String,
    pub latency_ms: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
}

pub struct PlaygroundResult {
    pub status: u16,
    pub body: Bytes,
    pub latency_ms: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
}

pub struct PlaygroundService;

impl PlaygroundService {
    pub fn new() -> Self {
        Self
    }

    /// Test connectivity by provider name.
    pub async fn test_by_name(
        &self,
        client: &reqwest::Client,
        provider_repo: &dyn ecc_domain::repository::ProviderRepository,
        provider_name: &str,
        model: &str,
        message: &str,
    ) -> Result<PlaygroundResult, ecc_domain::repository::RepositoryError> {
        let provider = provider_repo
            .get(provider_name)?
            .ok_or_else(|| ecc_domain::repository::RepositoryError::NotFound(format!(
                "provider '{provider_name}' not found"
            )))?;
        Ok(self.test(client, &provider, model, message).await)
    }

    async fn test(
        &self,
        client: &reqwest::Client,
        provider: &Provider,
        model: &str,
        message: &str,
    ) -> PlaygroundResult {
        let url = format!("{}/v1/messages", provider.base_url.trim_end_matches('/'));

        let (body, content_type) = match provider.protocol {
            Protocol::OpenAI => {
                let body = serde_json::json!({
                    "model": model,
                    "messages": [{ "role": "user", "content": message }],
                    "max_tokens": 64,
                    "stream": false,
                });
                (serde_json::to_string(&body).unwrap(), "application/json")
            }
            Protocol::Anthropic => {
                let body = serde_json::json!({
                    "model": model,
                    "messages": [{ "role": "user", "content": message }],
                    "max_tokens": 64,
                    "stream": false,
                });
                (serde_json::to_string(&body).unwrap(), "application/json")
            }
        };

        let start = std::time::Instant::now();

        let mut req = client
            .post(&url)
            .header("Content-Type", content_type)
            .header("anthropic-version", "2023-06-01");

        req = match provider.auth_type {
            AuthType::Bearer => req.bearer_auth(&provider.auth_token),
            AuthType::ApiKey => req.header("x-api-key", &provider.auth_token),
        };

        let resp = match req.body(body).send().await {
            Ok(r) => r,
            Err(e) => {
                return PlaygroundResult {
                    status: 0,
                    body: Bytes::from(e.to_string()),
                    latency_ms: start.elapsed().as_millis() as u64,
                    input_tokens: 0,
                    output_tokens: 0,
                    cache_read_tokens: 0,
                };
            }
        };

        let status = resp.status().as_u16();
        let body_bytes = resp.bytes().await.unwrap_or_default();
        let latency_ms = start.elapsed().as_millis() as u64;

        let (input_tokens, output_tokens, cache_read_tokens) = extract_usage(&body_bytes);

        PlaygroundResult {
            status,
            body: body_bytes,
            latency_ms,
            input_tokens,
            output_tokens,
            cache_read_tokens,
        }
    }
}

fn extract_usage(body: &[u8]) -> (u64, u64, u64) {
    let obj: serde_json::Value = match serde_json::from_slice(body) {
        Ok(v) => v,
        Err(_) => return (0, 0, 0),
    };
    let usage = match obj.get("usage") {
        Some(u) => u,
        None => return (0, 0, 0),
    };
    (
        usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
        usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
        usage.get("cache_read_input_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
    )
}
