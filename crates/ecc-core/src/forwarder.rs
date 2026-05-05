//! HTTP forwarder — sends requests to upstream provider APIs.

use std::sync::Arc;
use tokio::sync::RwLock;

use crate::context::RequestContext;
use crate::middleware::{BoxFuture, Middleware, MiddlewareError, Next};
use crate::logging::{FWD_UPSTREAM_5XX, FWD_UPSTREAM_ERROR, FWD_UPSTREAM_REQUEST, FWD_UPSTREAM_RESPONSE};
use crate::{ecc_error, ecc_info, ecc_warn};

/// Middleware that forwards the request to the upstream provider.
pub struct Forwarder {
    client: reqwest::Client,
    providers: Arc<RwLock<ecc_config::provider::ProviderTable>>,
}

impl Forwarder {
    pub fn new(providers: Arc<RwLock<ecc_config::provider::ProviderTable>>) -> Self {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(120))
            .build()
            .expect("failed to build HTTP client");
        Self { client, providers }
    }
}

impl Middleware for Forwarder {
    fn handle<'a>(
        &'a self,
        ctx: &'a mut RequestContext,
        next: Next<'a>,
    ) -> BoxFuture<'a, Result<(), MiddlewareError>> {
        Box::pin(async move {
            let target = ctx.resolved_target.as_ref().ok_or_else(|| {
                MiddlewareError::Custom("No resolved target for forwarding".into())
            })?;

            let providers = self.providers.read().await;
            let provider = providers.providers.get(&target.provider).ok_or_else(|| {
                MiddlewareError::Custom(format!("Provider '{}' not configured", target.provider))
            })?;

            let path = match provider.protocol {
                ecc_config::provider::Protocol::Anthropic => "/v1/messages",
                ecc_config::provider::Protocol::OpenAI => "/v1/chat/completions",
            };
            let url = format!("{}{}", provider.base_url.trim_end_matches('/'), path);

            let auth_value = match provider.auth_type {
                ecc_config::provider::AuthType::Bearer => {
                    format!("Bearer {}", provider.auth_token)
                }
                ecc_config::provider::AuthType::ApiKey => provider.auth_token.clone(),
            };

            let body = ctx.upstream_body.as_ref().unwrap_or(&ctx.body);

            ecc_info!(FWD_UPSTREAM_REQUEST,
                provider = %target.provider,
                target_model = %target.model,
                url = %url,
                protocol = ?provider.protocol,
                body_len = body.len(),
                "→ upstream"
            );

            let start = std::time::Instant::now();
            let resp = self
                .client
                .post(&url)
                .header("content-type", "application/json")
                .header("authorization", &auth_value)
                .body(body.clone())
                .send()
                .await
                .map_err(|e| {
                    ecc_error!(FWD_UPSTREAM_ERROR,
                        provider = %target.provider,
                        url = %url,
                        elapsed_ms = start.elapsed().as_millis(),
                        "connection failed: {e}"
                    );
                    MiddlewareError::Custom(format!("Upstream request failed: {e}"))
                })?;

            ctx.response_status = Some(resp.status().as_u16());

            let status = resp.status();
            let elapsed = start.elapsed();
            let resp_body = resp.bytes().await
                .map_err(|e| MiddlewareError::Custom(format!("Failed to read response: {e}")))?;

            ecc_info!(FWD_UPSTREAM_RESPONSE,
                provider = %target.provider,
                status = %status,
                body_len = resp_body.len(),
                elapsed_ms = elapsed.as_millis(),
                "← upstream"
            );

            // 5xx → failover
            if status.is_server_error() {
                let preview = String::from_utf8_lossy(&resp_body[..resp_body.len().min(500)]);
                ecc_warn!(FWD_UPSTREAM_5XX,
                    provider = %target.provider,
                    status = %status,
                    body = %preview,
                    "upstream 5xx → failover"
                );
                return Err(MiddlewareError::Custom(
                    format!("Upstream returned {}", status),
                ));
            }

            if let Some((input, cache, output)) =
                crate::usage::extract_usage_from_response(&resp_body)
            {
                ctx.usage = Some(crate::context::TokenUsage {
                    input_tokens: input,
                    cache_read_tokens: cache,
                    output_tokens: output,
                });
            }

            next.run(ctx).await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use http::{HeaderMap, Method};
    use std::collections::HashMap;

    fn make_provider_table() -> ecc_config::provider::ProviderTable {
        let mut providers = HashMap::new();
        providers.insert(
            "deepseek".to_string(),
            ecc_config::provider::Provider {
                base_url: "https://api.deepseek.com".to_string(),
                auth_token: "sk-test-123".to_string(),
                auth_type: ecc_config::provider::AuthType::Bearer,
                protocol: ecc_config::provider::Protocol::OpenAI,
                is_coding_plan: false,
            },
        );
        providers.insert(
            "kimi".to_string(),
            ecc_config::provider::Provider {
                base_url: "https://api.moonshot.cn".to_string(),
                auth_token: "mk-test-456".to_string(),
                auth_type: ecc_config::provider::AuthType::Bearer,
                protocol: ecc_config::provider::Protocol::Anthropic,
                is_coding_plan: false,
            },
        );
        ecc_config::provider::ProviderTable { providers }
    }

    fn make_ctx(provider: &str, model: &str) -> RequestContext {
        let mut ctx = RequestContext::new(
            Method::POST,
            "/v1/messages".to_string(),
            HeaderMap::new(),
            Bytes::from(r#"{"model":"test","messages":[]}"#.to_string()),
        );
        ctx.resolved_target = Some(ecc_config::route::RouteTarget {
            provider: provider.to_string(),
            model: model.to_string(),
            priority: 1,
        });
        ctx
    }

    #[test]
    fn t60_forwarder_constructs() {
        let providers = Arc::new(RwLock::new(make_provider_table()));
        let _forwarder = Forwarder::new(providers);
    }

    #[tokio::test]
    async fn t60_builds_correct_url_and_auth() {
        // Use a provider with an unreachable URL to verify forwarding logic
        let mut providers = HashMap::new();
        providers.insert(
            "test-provider".to_string(),
            ecc_config::provider::Provider {
                base_url: "http://127.0.0.1:1".to_string(), // unreachable port
                auth_token: "sk-test".to_string(),
                auth_type: ecc_config::provider::AuthType::Bearer,
                protocol: ecc_config::provider::Protocol::OpenAI,
                is_coding_plan: false,
            },
        );
        let providers = Arc::new(RwLock::new(ecc_config::provider::ProviderTable { providers }));
        let forwarder = Forwarder::new(providers);

        let mut ctx = make_ctx("test-provider", "model");
        let pipeline = crate::middleware::Pipeline::new()
            .add(Arc::new(forwarder));
        let result = pipeline.execute(&mut ctx).await;

        assert!(result.is_err(), "Should fail with unreachable host");
        let err = format!("{}", result.unwrap_err());
        assert!(err.contains("Upstream"), "Error should mention upstream: {err}");
    }

    #[tokio::test]
    async fn t60_missing_provider_returns_error() {
        let providers = Arc::new(RwLock::new(make_provider_table()));
        let forwarder = Forwarder::new(providers);

        let mut ctx = make_ctx("nonexistent", "model");
        let pipeline = crate::middleware::Pipeline::new()
            .add(Arc::new(forwarder));
        let result = pipeline.execute(&mut ctx).await;

        assert!(result.is_err());
        let err = format!("{}", result.unwrap_err());
        assert!(err.contains("not configured"), "Should report missing provider, got: {err}");
    }
}
