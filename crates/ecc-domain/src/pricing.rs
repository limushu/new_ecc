use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Pricing {
    pub input_per_m: f64,
    pub output_per_m: f64,
    pub cache_read_per_m: Option<f64>,
}

impl Pricing {
    pub fn calculate(&self, input_tokens: u64, cache_read_tokens: u64, output_tokens: u64) -> f64 {
        let input_cost = self.input_per_m * input_tokens as f64 / 1_000_000.0;
        let output_cost = self.output_per_m * output_tokens as f64 / 1_000_000.0;
        let cache_cost = self
            .cache_read_per_m
            .unwrap_or(0.0)
            * cache_read_tokens as f64
            / 1_000_000.0;
        input_cost + output_cost + cache_cost
    }
}
