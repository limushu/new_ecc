//! Middleware pipeline — the core execution engine for request processing.
//!
//! Provides a chain-of-responsibility pattern where each [`Middleware`] processes a
//! [`RequestContext`] and passes it to the next via [`Next`].
//!
//! # Pipeline execution
//!
//! ```text
//! Router → Protocol → Rectifier → CircuitBreaker → Forwarder → UsageTracker
//! ```
//!
//! # Failover
//!
//! [`Pipeline::execute`] wraps the chain in a retry loop. When the chain fails and
//! fallback targets are available, it swaps in the next target and re-runs the chain
//! (up to `max_retries` times).

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use crate::context::RequestContext;
use crate::logging::FO_TRYING_FALLBACK;
use crate::ecc_warn;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

#[derive(Debug, thiserror::Error)]
pub enum MiddlewareError {
    #[error("{0}")]
    Custom(String),
}

/// The "pass to next" callback. Each middleware calls `next.run(ctx)` to continue the chain.
pub struct Next<'a> {
    remaining: &'a [Arc<dyn Middleware>],
}

impl<'a> Next<'a> {
    pub async fn run(self, ctx: &'a mut RequestContext) -> Result<(), MiddlewareError> {
        if self.remaining.is_empty() {
            return Ok(());
        }
        let (head, tail) = self.remaining.split_at(1);
        let next = Next { remaining: tail };
        head[0].handle(ctx, next).await
    }
}

/// A single processing unit in the request pipeline.
pub trait Middleware: Send + Sync {
    fn handle<'a>(&'a self, ctx: &'a mut RequestContext, next: Next<'a>) -> BoxFuture<'a, Result<(), MiddlewareError>>;
}

/// The orchestrator. Owns the ordered middleware list, drives execution,
/// and handles retry/failover when a middleware reports failure.
pub struct Pipeline {
    middlewares: Vec<Arc<dyn Middleware>>,
    max_retries: u8,
}

impl Pipeline {
    pub fn new() -> Self {
        Self { middlewares: Vec::new(), max_retries: 3 }
    }

    pub fn with_max_retries(mut self, n: u8) -> Self {
        self.max_retries = n;
        self
    }

    pub fn add(mut self, middleware: Arc<dyn Middleware>) -> Self {
        self.middlewares.push(middleware);
        self
    }

    /// Execute the middleware chain. On failure, tries fallback targets if available.
    pub async fn execute(&self, ctx: &mut RequestContext) -> Result<(), MiddlewareError> {
        ctx.max_retries = self.max_retries;

        loop {
            match self.run_chain(ctx).await {
                Ok(()) => return Ok(()),
                Err(e) => {
                    if ctx.fallback_targets.is_empty() || ctx.retry_count >= ctx.max_retries {
                        return Err(e);
                    }
                    ctx.resolved_target = Some(ctx.fallback_targets.remove(0));
                    ctx.retry_count += 1;
                    ecc_warn!(FO_TRYING_FALLBACK,
                        request_id = %ctx.id,
                        retry = ctx.retry_count,
                        "trying next fallback target"
                    );
                }
            }
        }
    }

    fn run_chain<'a>(&'a self, ctx: &'a mut RequestContext) -> BoxFuture<'a, Result<(), MiddlewareError>> {
        let next = Next { remaining: &self.middlewares };
        Box::pin(async move { next.run(ctx).await })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use http::{HeaderMap, Method};
    use std::sync::atomic::{AtomicU8, Ordering};
    use std::sync::Mutex;

    fn make_ctx() -> RequestContext {
        RequestContext::new(
            Method::POST,
            "/v1/messages".to_string(),
            HeaderMap::new(),
            Bytes::new(),
        )
    }

    struct Recorder {
        name: &'static str,
        log: Arc<Mutex<Vec<String>>>,
    }

    impl Recorder {
        fn new(name: &'static str, log: &Arc<Mutex<Vec<String>>>) -> Self {
            Self { name, log: Arc::clone(log) }
        }
    }

    impl Middleware for Recorder {
        fn handle<'a>(&'a self, ctx: &'a mut RequestContext, next: Next<'a>) -> BoxFuture<'a, Result<(), MiddlewareError>> {
            self.log.lock().unwrap().push(self.name.to_string());
            Box::pin(async move { next.run(ctx).await })
        }
    }

    struct ShortCircuit;

    impl Middleware for ShortCircuit {
        fn handle<'a>(&'a self, _ctx: &'a mut RequestContext, _next: Next<'a>) -> BoxFuture<'a, Result<(), MiddlewareError>> {
            Box::pin(async { Err(MiddlewareError::Custom("stopped".into())) })
        }
    }

    struct Writer;

    impl Middleware for Writer {
        fn handle<'a>(&'a self, ctx: &'a mut RequestContext, next: Next<'a>) -> BoxFuture<'a, Result<(), MiddlewareError>> {
            ctx.requested_model = Some("written".to_string());
            Box::pin(async move { next.run(ctx).await })
        }
    }

    struct Reader {
        observed: Arc<Mutex<Option<String>>>,
    }

    impl Middleware for Reader {
        fn handle<'a>(&'a self, ctx: &'a mut RequestContext, next: Next<'a>) -> BoxFuture<'a, Result<(), MiddlewareError>> {
            *self.observed.lock().unwrap() = ctx.requested_model.clone();
            Box::pin(async move { next.run(ctx).await })
        }
    }

    struct FailNTimes {
        n: AtomicU8,
    }

    impl FailNTimes {
        fn new(n: u8) -> Self {
            Self { n: AtomicU8::new(n) }
        }
    }

    impl Middleware for FailNTimes {
        fn handle<'a>(&'a self, _ctx: &'a mut RequestContext, _next: Next<'a>) -> BoxFuture<'a, Result<(), MiddlewareError>> {
            let remaining = self.n.fetch_sub(1, Ordering::SeqCst);
            Box::pin(async move {
                if remaining > 0 {
                    Err(MiddlewareError::Custom("transient".into()))
                } else {
                    Ok(())
                }
            })
        }
    }

    #[tokio::test]
    async fn t15_pipeline_executes_in_order() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let pipeline = Pipeline::new()
            .add(Arc::new(Recorder::new("A", &log)))
            .add(Arc::new(Recorder::new("B", &log)))
            .add(Arc::new(Recorder::new("C", &log)));

        let mut ctx = make_ctx();
        pipeline.execute(&mut ctx).await.unwrap();

        assert_eq!(*log.lock().unwrap(), vec!["A", "B", "C"]);
    }

    #[tokio::test]
    async fn t16_middleware_short_circuits_no_fallback() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let pipeline = Pipeline::new()
            .add(Arc::new(Recorder::new("A", &log)))
            .add(Arc::new(ShortCircuit))
            .add(Arc::new(Recorder::new("C", &log)));

        let mut ctx = make_ctx();
        let result = pipeline.execute(&mut ctx).await;

        assert!(result.is_err());
        assert_eq!(*log.lock().unwrap(), vec!["A"]);
    }

    #[tokio::test]
    async fn t17_middleware_can_modify_context() {
        let observed = Arc::new(Mutex::new(None));
        let pipeline = Pipeline::new()
            .add(Arc::new(Writer))
            .add(Arc::new(Reader { observed: Arc::clone(&observed) }));

        let mut ctx = make_ctx();
        pipeline.execute(&mut ctx).await.unwrap();

        assert_eq!(*observed.lock().unwrap(), Some("written".to_string()));
    }

    #[tokio::test]
    async fn t36_pipeline_failover_to_fallback() {
        let pipeline = Pipeline::new()
            .with_max_retries(3)
            .add(Arc::new(FailNTimes::new(1)));

        let mut ctx = make_ctx();
        ctx.fallback_targets.push(ecc_config::route::RouteTarget {
            provider: "backup".to_string(),
            model: "backup-model".to_string(),
            priority: 2,
        });

        let result = pipeline.execute(&mut ctx).await;
        assert!(result.is_ok());
        assert_eq!(ctx.retry_count, 1);
    }

    #[tokio::test]
    async fn t37_pipeline_all_fallbacks_exhausted() {
        let pipeline = Pipeline::new()
            .with_max_retries(3)
            .add(Arc::new(ShortCircuit));

        let mut ctx = make_ctx();
        ctx.fallback_targets.push(ecc_config::route::RouteTarget {
            provider: "backup".to_string(),
            model: "backup-model".to_string(),
            priority: 2,
        });

        let result = pipeline.execute(&mut ctx).await;
        assert!(result.is_err());
    }
}
