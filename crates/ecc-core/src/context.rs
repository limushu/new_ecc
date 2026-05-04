//! Request context — the shared mutable state that flows through the middleware pipeline.
//!
//! Each incoming request creates a [`RequestContext`] that is passed through every middleware
//! in the pipeline. Middlewares read from and write to this context to communicate:
//!
//! - **RouterMiddleware** fills `requested_model`, `resolved_target`, `fallback_targets`
//! - **ProtocolMiddleware** fills `upstream_url`, `upstream_headers`, `upstream_body` (future)
//! - **Forwarder** reads upstream fields, fills `response_status` and `usage`
//! - **UsageTracker** reads usage fields and records them

use std::time::Instant;

use bytes::Bytes;
use http::{HeaderMap, Method};
use uuid::Uuid;

use ecc_config::provider::Protocol;
use ecc_config::route::RouteTarget;

#[derive(Debug, Clone, PartialEq)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub cache_read_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug)]
pub struct RequestContext {
    pub id: Uuid,
    pub timestamp: Instant,

    // Original request
    pub method: Method,
    pub path: String,
    pub headers: HeaderMap,
    pub body: Bytes,

    // Resolved by middleware chain
    pub requested_model: Option<String>,
    pub resolved_target: Option<RouteTarget>,
    pub fallback_targets: Vec<RouteTarget>,
    pub protocol: Protocol,

    // Upstream request (populated by protocol converter)
    pub upstream_body: Option<Bytes>,

    // Response
    pub response_status: Option<u16>,
    pub usage: Option<TokenUsage>,

    // Retry
    pub retry_count: u8,
    pub max_retries: u8,
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
            protocol: Protocol::default(),
            upstream_body: None,
            response_status: None,
            usage: None,
            retry_count: 0,
            max_retries: 3,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn t14_context_construction_defaults() {
        let ctx = RequestContext::new(
            Method::POST,
            "/v1/messages".to_string(),
            HeaderMap::new(),
            Bytes::new(),
        );
        assert!(ctx.requested_model.is_none());
        assert!(ctx.resolved_target.is_none());
        assert!(ctx.fallback_targets.is_empty());
        assert_eq!(ctx.protocol, Protocol::Anthropic);
        assert!(ctx.response_status.is_none());
        assert!(ctx.usage.is_none());
        assert_eq!(ctx.retry_count, 0);
        assert_eq!(ctx.max_retries, 3);
    }
}
