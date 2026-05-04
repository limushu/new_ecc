//! ecc-config — configuration management for ecc.
//!
//! Handles all persistent configuration: providers, routes, presets, and pricing.
//!
//! - [`provider`] — provider definitions (base URL, auth, protocol)
//! - [`route`] — route table with priority-based target lists and date-suffix fallback
//! - [`preset`] — built-in provider presets (DeepSeek, Kimi, GLM) with user overrides
//! - [`pricing`] — per-model pricing data and cost calculation

pub mod pricing;
pub mod preset;
pub mod provider;
pub mod route;
