use crate::context::RequestContext;
use crate::middleware::{Middleware, Next, PipelineResult};

/// Fixes thinking block issues in Anthropic responses.
/// Passthrough for now — can be extended for specific provider quirks.
pub struct ThinkingRectifier;

impl ThinkingRectifier {
    pub fn new() -> Self {
        Self
    }
}

impl Middleware for ThinkingRectifier {
    fn handle<'a>(
        &'a self,
        ctx: &'a mut RequestContext,
        next: Next<'a>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = PipelineResult> + Send + 'a>> {
        Box::pin(async move {
            next.run(ctx).await
        })
    }
}
