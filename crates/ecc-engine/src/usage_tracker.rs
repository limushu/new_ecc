use std::sync::Arc;

use crate::context::RequestContext;
use crate::middleware::{Middleware, Next, PipelineResult};
use crate::port::UsagePort;
use ecc_domain::repository::UsageRecord;

pub struct UsageTracker {
    usage_port: Arc<dyn UsagePort>,
}

impl UsageTracker {
    pub fn new(usage_port: Arc<dyn UsagePort>) -> Self {
        Self { usage_port }
    }
}

impl Middleware for UsageTracker {
    fn handle<'a>(
        &'a self,
        ctx: &'a mut RequestContext,
        next: Next<'a>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = PipelineResult> + Send + 'a>> {
        Box::pin(async move {
            let result = next.run(ctx).await;

            // Record usage regardless of success/failure
            if let (Some(target), Some(provider)) = (&ctx.resolved_target, &ctx.provider_config) {
                let usage = ctx.usage.as_ref();
                let input_tokens = usage.map(|u| u.input_tokens).unwrap_or(0);
                let output_tokens = usage.map(|u| u.output_tokens).unwrap_or(0);
                let cache_read_tokens = usage.map(|u| u.cache_read_tokens).unwrap_or(0);

                let cost = provider
                    .pricing
                    .get(&target.provider_model)
                    .map(|p| p.calculate(input_tokens, cache_read_tokens, output_tokens))
                    .unwrap_or(0.0);

                let record = UsageRecord {
                    id: uuid::Uuid::new_v4().to_string(),
                    timestamp: chrono::Utc::now(),
                    provider_name: provider.name.clone(),
                    target_model: target.provider_model.clone(),
                    requested_model: ctx.requested_model.clone().unwrap_or_default(),
                    input_tokens,
                    output_tokens,
                    cache_read_tokens,
                    cost_usd: cost,
                    latency_ms: ctx.timestamp.elapsed().as_millis() as u64,
                    status: ctx.response_status.unwrap_or(0),
                };

                if let Err(e) = self.usage_port.record(record) {
                    tracing::warn!("failed to record usage: {e}");
                }
            }

            result
        })
    }
}
