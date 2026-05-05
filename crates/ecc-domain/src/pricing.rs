use serde::{Deserialize, Serialize};

/// 每百万 token 价格，支持缓存命中优惠价。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Pricing {
    pub input_per_m: f64,
    pub output_per_m: f64,
    #[serde(default)]
    pub cache_read_per_m: Option<f64>,
}

impl Pricing {
    /// 计算实际费用（含缓存分解）。
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cost_no_cache() {
        let p = Pricing { input_per_m: 1.0, output_per_m: 2.0, cache_read_per_m: None };
        let cost = p.calculate(1520, 0, 832);
        assert!((cost - (0.00152 + 0.001664)).abs() < 1e-10);
    }

    #[test]
    fn cost_with_cache() {
        let p = Pricing { input_per_m: 0.28, output_per_m: 0.42, cache_read_per_m: Some(0.028) };
        let cost = p.calculate(1000, 800, 500);
        assert!((cost - (0.000056 + 0.0000224 + 0.00021)).abs() < 1e-10);
    }
}
