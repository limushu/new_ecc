use std::collections::HashMap;
use std::time::Instant;

use bytes::Bytes;
use http::Method;
use http::header::HeaderMap;
use uuid::Uuid;

use ecc_domain::mapping::RouteTarget;
use ecc_domain::provider::{AuthType, Protocol};
use ecc_domain::Pricing;

/// Provider core config — engine hot path only needs these fields.
#[derive(Debug, Clone)]
pub struct ProviderRef {
    pub name: String,
    pub base_url: String,
    pub auth_token: String,
    pub auth_type: AuthType,
    pub protocol: Protocol,
    pub pricing: HashMap<String, Pricing>,
}

#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub cache_read_tokens: u64,
    pub output_tokens: u64,
}

/// Request frame — each layer reads/writes its own section, never modifies another layer's data.
pub struct RequestContext {
    pub id: Uuid,
    pub timestamp: Instant,

    // -- Layer 1: raw request (set by ProxyServer, read-only for all middleware) --
    pub method: Method,
    pub path: String,
    pub headers: HeaderMap,
    pub body: Bytes,

    // -- Layer 2: route resolution (written by Router) --
    pub requested_model: Option<String>,
    pub resolved_target: Option<RouteTarget>,
    pub fallback_targets: Vec<RouteTarget>,
    pub provider_config: Option<ProviderRef>,

    // -- Layer 3: protocol conversion (written by Converter) --
    pub upstream_url: Option<String>,
    pub upstream_headers: Option<Vec<(String, String)>>,
    pub upstream_body: Option<Bytes>,

    // -- Layer 4: response (written by Forwarder) --
    pub response_status: Option<u16>,
    pub response_body: Option<Bytes>,
    pub stream_chunks: Vec<Bytes>,
    pub usage: Option<TokenUsage>,

    // -- Retry control --
    pub retry_count: u8,
}

impl RequestContext {
    pub fn new(method: Method, path: String, headers: HeaderMap, body: Bytes) -> Self {
        Self {
            id: Uuid::new_v4(),
            timestamp: Instant::now(),
            method,
            path,
            headers,
            body,
            requested_model: None,
            resolved_target: None,
            fallback_targets: Vec::new(),
            provider_config: None,
            upstream_url: None,
            upstream_headers: None,
            upstream_body: None,
            response_status: None,
            response_body: None,
            stream_chunks: Vec::new(),
            usage: None,
            retry_count: 0,
        }
    }

    pub fn extract_model(&mut self) {
        if let Ok(obj) = serde_json::from_slice::<serde_json::Value>(&self.body) {
            self.requested_model = obj.get("model").and_then(|v| v.as_str()).map(|s| s.to_string());
        }
    }

    pub fn is_streaming(&self) -> bool {
        serde_json::from_slice::<serde_json::Value>(&self.body)
            .ok()
            .and_then(|v| v.get("stream").and_then(|s| s.as_bool()))
            .unwrap_or(false)
    }

    /// Get protocol from resolved provider, defaulting to Anthropic.
    pub fn protocol(&self) -> Protocol {
        self.provider_config
            .as_ref()
            .map(|c| c.protocol)
            .unwrap_or_default()
    }
}
