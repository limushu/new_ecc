use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::context::RequestContext;

/// Pipeline-level errors only. Business outcomes are communicated via RequestContext fields.
#[derive(Debug, thiserror::Error)]
pub enum PipelineError {
    #[error("middleware aborted: {0}")]
    Aborted(String),
    #[error("internal: {0}")]
    Internal(String),
}

pub type PipelineResult = std::result::Result<(), PipelineError>;

pub trait Middleware: Send + Sync {
    fn handle<'a>(
        &'a self,
        ctx: &'a mut RequestContext,
        next: Next<'a>,
    ) -> Pin<Box<dyn Future<Output = PipelineResult> + Send + 'a>>;
}

pub struct Next<'a> {
    middlewares: &'a [Arc<dyn Middleware>],
    index: usize,
}

impl<'a> Next<'a> {
    pub async fn run(self, ctx: &mut RequestContext) -> PipelineResult {
        if self.index >= self.middlewares.len() {
            return Ok(());
        }
        let mw = &self.middlewares[self.index];
        let next = Next {
            middlewares: self.middlewares,
            index: self.index + 1,
        };
        mw.handle(ctx, next).await
    }
}

pub struct Pipeline {
    middlewares: Vec<Arc<dyn Middleware>>,
    max_retries: u8,
}

impl Pipeline {
    pub fn new() -> Self {
        Self {
            middlewares: Vec::new(),
            max_retries: 2,
        }
    }

    pub fn with_max_retries(mut self, n: u8) -> Self {
        self.max_retries = n;
        self
    }

    pub fn add(mut self, middleware: Arc<dyn Middleware>) -> Self {
        self.middlewares.push(middleware);
        self
    }

    pub async fn execute(&self, ctx: &mut RequestContext) -> PipelineResult {
        loop {
            let next = Next {
                middlewares: &self.middlewares,
                index: 0,
            };
            match next.run(ctx).await {
                Ok(()) => return Ok(()),
                Err(PipelineError::Aborted(_))
                    if !ctx.fallback_targets.is_empty() && ctx.retry_count < self.max_retries =>
                {
                    let fallback = ctx.fallback_targets.remove(0);
                    ctx.resolved_target = Some(fallback);
                    ctx.provider_config = None;
                    ctx.upstream_url = None;
                    ctx.upstream_headers = None;
                    ctx.upstream_body = None;
                    ctx.response_status = None;
                    ctx.response_body = None;
                    ctx.usage = None;
                    ctx.retry_count += 1;
                    tracing::warn!(retry = ctx.retry_count, "retrying with fallback");
                }
                Err(e) => return Err(e),
            }
        }
    }
}
