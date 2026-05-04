//! ecc-core — the proxy engine for ecc (easy cc switch).
//!
//! This crate contains the core request processing logic:
//!
//! - [`context`] — [`context::RequestContext`] definition, the shared state flowing through the pipeline
//! - [`middleware`] — [`middleware::Middleware`] trait, [`Pipeline`](middleware::Pipeline) executor with failover
//! - [`router`] — route resolution from model name to provider target
//! - [`protocol`] — Anthropic ↔ OpenAI protocol conversion
//! - [`rectifier`] — thinking block repair for downstream compatibility
//! - [`circuit_breaker`] — per-route-granularity circuit breaking
//! - [`forwarder`] — HTTP forwarding to upstream providers
//! - [`usage`] — JSONL usage recording and cost calculation

pub mod circuit_breaker;
pub mod coding_plan;
pub mod context;
pub mod forwarder;
pub mod logging;
pub mod middleware;
pub mod protocol;
pub mod rectifier;
pub mod router;
pub mod usage;
