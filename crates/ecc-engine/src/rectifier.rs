use crate::context::RequestContext;
use crate::middleware::{Middleware, Next, PipelineResult};

/// Forces thinking to enabled and injects budget if missing.
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
            if let Some(body) = ctx.upstream_body.as_mut() {
                if let Ok(mut obj) = serde_json::from_slice::<serde_json::Value>(body) {
                    if let Some(thinking) = obj.get_mut("thinking") {
                        if thinking.get("type").and_then(|t| t.as_str()) == Some("adaptive") {
                            thinking["type"] = serde_json::Value::String("enabled".into());
                            if thinking.get("budget_tokens").is_none() {
                                thinking["budget_tokens"] = serde_json::Value::Number(serde_json::Number::from(10000));
                            }
                            *body = serde_json::to_vec(&obj).unwrap_or_default().into();
                        }
                    }
                }
            }
            next.run(ctx).await
        })
    }
}
