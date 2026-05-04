//! Router middleware — resolves model names to upstream provider targets.
//!
//! Extracts the `model` field from the request body, looks it up in the route table,
//! and populates [`RequestContext`] with the resolved target and fallback options.
//!
//! # Route resolution
//!
//! 1. Parse `"model"` from request body JSON
//! 2. Look up in [`RouteTable`] via [`ecc_config::route::resolve_route`]
//! 3. If exact match fails, try stripping date suffix (e.g. `claude-haiku-4-5-20251001` → `claude-haiku-4-5`)
//! 4. Fill `resolved_target` with highest-priority entry, `fallback_targets` with the rest

use std::sync::Arc;
use tokio::sync::RwLock;

use ecc_config::route::RouteTable;

use crate::context::RequestContext;
use crate::middleware::{BoxFuture, Middleware, MiddlewareError, Next};
use crate::logging::{ROUTE_NOT_FOUND, ROUTE_RESOLVED};
use crate::{ecc_info, ecc_warn};

pub struct RouterMiddleware {
    route_table: Arc<RwLock<RouteTable>>,
}

impl RouterMiddleware {
    pub fn new(route_table: Arc<RwLock<RouteTable>>) -> Self {
        Self { route_table }
    }
}

impl Middleware for RouterMiddleware {
    fn handle<'a>(&'a self, ctx: &'a mut RequestContext, next: Next<'a>) -> BoxFuture<'a, Result<(), MiddlewareError>> {
        Box::pin(async move {
            // 1. Extract model name from request body
            if let Ok(data) = serde_json::from_slice::<serde_json::Value>(&ctx.body) {
                if let Some(model) = data.get("model").and_then(|m| m.as_str()) {
                    ctx.requested_model = Some(model.to_string());
                }
            }

            // 2. Look up route
            if let Some(ref model) = ctx.requested_model {
                let table = self.route_table.read().await;
                if let Some(entry) = ecc_config::route::resolve_route(&table, model) {
                    // Sort targets by priority (defensive, should already be sorted)
                    let mut targets: Vec<_> = entry.targets.iter().collect();
                    targets.sort_by_key(|t| t.priority);

                    if !targets.is_empty() {
                        ctx.resolved_target = Some(targets[0].clone());
                        ctx.fallback_targets = targets[1..].iter().map(|t| (*t).clone()).collect();
                        ecc_info!(ROUTE_RESOLVED,
                            model = %model,
                            provider = %targets[0].provider,
                            target_model = %targets[0].model,
                            fallbacks = ctx.fallback_targets.len(),
                            "route resolved"
                        );
                    }
                } else {
                    ecc_warn!(ROUTE_NOT_FOUND, model = %model, "no route found for model");
                }
            }

            // 3. Check if we resolved a target
            if ctx.resolved_target.is_none() {
                return Err(MiddlewareError::Custom(format!(
                    "No route for model '{}'",
                    ctx.requested_model.as_deref().unwrap_or("(none)")
                )));
            }

            next.run(ctx).await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use ecc_config::route::{RouteEntry, RouteTarget, RouteTable};
    use http::{HeaderMap, Method};
    use std::collections::HashMap;

    fn make_ctx_with_body(body: &str) -> RequestContext {
        RequestContext::new(
            Method::POST,
            "/v1/messages".to_string(),
            HeaderMap::new(),
            Bytes::from(body.to_string()),
        )
    }

    fn make_route_table() -> RouteTable {
        let mut routes = HashMap::new();
        routes.insert(
            "claude-sonnet-4-6".to_string(),
            RouteEntry {
                targets: vec![
                    RouteTarget { provider: "kimi".to_string(), model: "K2.6".to_string(), priority: 1 },
                    RouteTarget { provider: "zhipu".to_string(), model: "glm-4".to_string(), priority: 2 },
                ],
            },
        );
        routes.insert(
            "claude-haiku-4-5".to_string(),
            RouteEntry {
                targets: vec![
                    RouteTarget { provider: "deepseek".to_string(), model: "deepseek-chat".to_string(), priority: 1 },
                ],
            },
        );
        RouteTable { routes }
    }

    async fn make_pipeline(table: RouteTable) -> (crate::middleware::Pipeline, Arc<RwLock<RouteTable>>) {
        let shared = Arc::new(RwLock::new(table));
        let router = Arc::new(RouterMiddleware::new(Arc::clone(&shared)));
        let pipeline = crate::middleware::Pipeline::new().add(router);
        (pipeline, shared)
    }

    #[tokio::test]
    async fn t18_parse_model_from_body() {
        let (pipeline, _) = make_pipeline(make_route_table()).await;
        let mut ctx = make_ctx_with_body(r#"{"model":"claude-sonnet-4-6","messages":[]}"#);

        pipeline.execute(&mut ctx).await.unwrap();

        assert_eq!(ctx.requested_model, Some("claude-sonnet-4-6".to_string()));
    }

    #[tokio::test]
    async fn t19_resolve_target_from_route() {
        let (pipeline, _) = make_pipeline(make_route_table()).await;
        let mut ctx = make_ctx_with_body(r#"{"model":"claude-sonnet-4-6","messages":[]}"#);

        pipeline.execute(&mut ctx).await.unwrap();

        let target = ctx.resolved_target.unwrap();
        assert_eq!(target.provider, "kimi");
        assert_eq!(target.model, "K2.6");
    }

    #[tokio::test]
    async fn t20_fill_fallback_targets() {
        let (pipeline, _) = make_pipeline(make_route_table()).await;
        let mut ctx = make_ctx_with_body(r#"{"model":"claude-sonnet-4-6","messages":[]}"#);

        pipeline.execute(&mut ctx).await.unwrap();

        assert_eq!(ctx.fallback_targets.len(), 1);
        assert_eq!(ctx.fallback_targets[0].provider, "zhipu");
    }

    #[tokio::test]
    async fn t21_date_suffix_fallback() {
        let (pipeline, _) = make_pipeline(make_route_table()).await;
        let mut ctx = make_ctx_with_body(r#"{"model":"claude-haiku-4-5-20251001","messages":[]}"#);

        pipeline.execute(&mut ctx).await.unwrap();

        assert_eq!(ctx.resolved_target.unwrap().provider, "deepseek");
    }

    #[tokio::test]
    async fn t22_no_matching_route_returns_error() {
        let (pipeline, _) = make_pipeline(make_route_table()).await;
        let mut ctx = make_ctx_with_body(r#"{"model":"claude-opus-4-7","messages":[]}"#);

        let result = pipeline.execute(&mut ctx).await;
        assert!(result.is_err());
    }
}
