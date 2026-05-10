use std::sync::Arc;

use crate::context::{ProviderRef, RequestContext};
use crate::middleware::{Middleware, Next, PipelineError, PipelineResult};
use crate::port::RoutePort;

pub struct Router {
    route_port: Arc<dyn RoutePort>,
}

impl Router {
    pub fn new(route_port: Arc<dyn RoutePort>) -> Self {
        Self { route_port }
    }
}

impl Middleware for Router {
    fn handle<'a>(
        &'a self,
        ctx: &'a mut RequestContext,
        next: Next<'a>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = PipelineResult> + Send + 'a>> {
        Box::pin(async move {
            // Skip route lookup on retry — resolved_target already set by Pipeline::execute
            if ctx.provider_config.is_some() {
                return next.run(ctx).await;
            }

            ctx.extract_model();
            let model = match &ctx.requested_model {
                Some(m) => m.clone(),
                None => return Err(PipelineError::Aborted("no model in request body".into())),
            };

            // Try exact match, then strip date suffix (claude-haiku-4-5-20251001 → claude-haiku-4-5)
            let routes = self.find_routes(&model)?;

            let targets = match routes {
                Some(t) if !t.is_empty() => t,
                _ => return Err(PipelineError::Aborted(format!("no route for model: {model}"))),
            };

            let primary = targets[0].clone();
            ctx.fallback_targets = targets[1..].to_vec();

            // Load provider config
            let provider = self
                .route_port
                .get_provider(&primary.provider_name)
                .map_err(|e| PipelineError::Internal(e.to_string()))?
                .ok_or_else(|| PipelineError::Aborted(format!("provider not found: {}", primary.provider_name)))?;

            ctx.resolved_target = Some(primary);
            ctx.provider_config = Some(ProviderRef {
                name: provider.name.clone(),
                base_url: provider.base_url.clone(),
                auth_token: provider.auth_token.clone(),
                auth_type: provider.auth_type,
                protocol: provider.protocol,
                pricing: provider.pricing,
            });

            next.run(ctx).await
        })
    }
}

impl Router {
    fn find_routes(&self, model: &str) -> std::result::Result<Option<Vec<ecc_domain::mapping::RouteTarget>>, PipelineError> {
        if let Ok(Some(routes)) = self.route_port.find_routes(model) {
            return Ok(Some(routes));
        }
        // Strip date suffix: claude-haiku-4-5-20251001 → claude-haiku-4-5
        if let Some(stripped) = model.rsplit_once('-').and_then(|(prefix, date)| {
            date.chars().all(char::is_numeric).then_some(prefix)
        }) {
            return self.route_port.find_routes(stripped)
                .map_err(|e| PipelineError::Internal(e.to_string()));
        }
        Ok(None)
    }
}
