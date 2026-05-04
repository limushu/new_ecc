use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Price per million tokens, with optional cache hit pricing
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Pricing {
    pub input_per_m: f64,
    pub output_per_m: f64,
    #[serde(default)]
    pub cache_read_per_m: Option<f64>,
}

impl Pricing {
    /// Calculate cost given token usage with cache breakdown
    pub fn calculate(&self, input_tokens: u64, cache_read_tokens: u64, output_tokens: u64) -> f64 {
        let non_cached_input = input_tokens.saturating_sub(cache_read_tokens);
        let input_cost = (non_cached_input as f64 / 1_000_000.0) * self.input_per_m;
        let cache_cost = if cache_read_tokens > 0 {
            let cache_rate = self.cache_read_per_m.unwrap_or(self.input_per_m);
            (cache_read_tokens as f64 / 1_000_000.0) * cache_rate
        } else {
            0.0
        };
        let output_cost = (output_tokens as f64 / 1_000_000.0) * self.output_per_m;
        input_cost + cache_cost + output_cost
    }
}

/// Calculate cost in USD for a single request (simple case, no cache breakdown)
pub fn calculate_cost(input_tokens: u64, output_tokens: u64, pricing: &Pricing) -> f64 {
    pricing.calculate(input_tokens, 0, output_tokens)
}

/// Look up pricing by model name, returns None if model not found
pub fn get_pricing<'a>(pricing_map: &'a HashMap<String, Pricing>, model: &str) -> Option<&'a Pricing> {
    pricing_map.get(model)
}

/// Calculate cost, returns 0.0 for unknown models
pub fn calculate_cost_or_zero(
    input_tokens: u64,
    output_tokens: u64,
    pricing_map: &HashMap<String, Pricing>,
    model: &str,
) -> f64 {
    match get_pricing(pricing_map, model) {
        Some(p) => calculate_cost(input_tokens, output_tokens, p),
        None => 0.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_pricing() -> HashMap<String, Pricing> {
        let mut map = HashMap::new();
        map.insert(
            "deepseek-chat".to_string(),
            Pricing {
                input_per_m: 0.28,
                output_per_m: 0.42,
                cache_read_per_m: Some(0.028),
            },
        );
        map
    }

    #[test]
    fn t12_calculate_cost_no_cache() {
        let pricing = Pricing {
            input_per_m: 1.0,
            output_per_m: 2.0,
            cache_read_per_m: None,
        };
        let cost = calculate_cost(1520, 832, &pricing);
        let expected = 0.00152 + 0.001664;
        assert!((cost - expected).abs() < 1e-10);
    }

    #[test]
    fn t12_calculate_cost_with_cache() {
        let pricing = Pricing {
            input_per_m: 0.28,
            output_per_m: 0.42,
            cache_read_per_m: Some(0.028),
        };
        // 1000 total input, 800 cache hit, 200 non-cached
        // cache: 800/1M * 0.028 = 0.0000224
        // non-cached: 200/1M * 0.28 = 0.000056
        // output: 500/1M * 0.42 = 0.00021
        let cost = pricing.calculate(1000, 800, 500);
        let expected = 0.000056 + 0.0000224 + 0.00021;
        assert!((cost - expected).abs() < 1e-10);
    }

    #[test]
    fn t12_cache_cheaper_than_input() {
        let pricing = Pricing {
            input_per_m: 0.28,
            output_per_m: 0.42,
            cache_read_per_m: Some(0.028),
        };
        let with_cache = pricing.calculate(1000, 1000, 1000);
        let without_cache = pricing.calculate(1000, 0, 1000);
        assert!(with_cache < without_cache, "cache hit pricing should be cheaper");
    }

    #[test]
    fn t13_unknown_model_returns_zero() {
        let map = sample_pricing();
        let cost = calculate_cost_or_zero(1000, 1000, &map, "unknown-model");
        assert_eq!(cost, 0.0);
    }

    #[test]
    fn t12_found_model_cost() {
        let map = sample_pricing();
        let cost = calculate_cost_or_zero(1_000_000, 1_000_000, &map, "deepseek-chat");
        let expected = 0.28 + 0.42;
        assert!((cost - expected).abs() < 1e-10);
    }
}
