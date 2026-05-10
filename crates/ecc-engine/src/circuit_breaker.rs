use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::context::RequestContext;
use crate::middleware::{Middleware, Next, PipelineError, PipelineResult};

pub struct CircuitBreakerConfig {
    pub failure_threshold: u32,
    pub cooldown: Duration,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            cooldown: Duration::from_secs(60),
        }
    }
}

struct CircuitState {
    failures: u32,
    opened_at: Option<Instant>,
}

pub struct CircuitBreaker {
    config: CircuitBreakerConfig,
    states: Mutex<HashMap<String, CircuitState>>,
}

impl CircuitBreaker {
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            config,
            states: Mutex::new(HashMap::new()),
        }
    }

    pub fn is_open(&self, key: &str) -> bool {
        let states = self.states.lock().unwrap();
        if let Some(state) = states.get(key) {
            if let Some(opened_at) = state.opened_at {
                if opened_at.elapsed() < self.config.cooldown {
                    return true;
                }
            }
        }
        false
    }

    pub fn record_success(&self, key: &str) {
        let mut states = self.states.lock().unwrap();
        states.remove(key);
    }

    pub fn record_failure(&self, key: &str) {
        let mut states = self.states.lock().unwrap();
        let state = states.entry(key.to_string()).or_insert(CircuitState {
            failures: 0,
            opened_at: None,
        });
        state.failures += 1;
        if state.failures >= self.config.failure_threshold {
            state.opened_at = Some(Instant::now());
        }
    }
}

impl Middleware for CircuitBreaker {
    fn handle<'a>(
        &'a self,
        ctx: &'a mut RequestContext,
        next: Next<'a>,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = PipelineResult> + Send + 'a>> {
        Box::pin(async move {
            let key = ctx
                .provider_config
                .as_ref()
                .map(|p| p.name.clone())
                .unwrap_or_default();

            if !key.is_empty() && self.is_open(&key) {
                return Err(PipelineError::Aborted(format!("circuit open: {key}")));
            }

            let result = next.run(ctx).await;

            match &result {
                Ok(()) => self.record_success(&key),
                Err(_) => self.record_failure(&key),
            }

            result
        })
    }
}
