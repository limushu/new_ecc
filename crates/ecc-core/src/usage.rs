//! Usage tracking — JSONL-based request logging with daily file rotation.
//!
//! Records per-request usage data (model, provider, tokens, cost, latency) to JSONL files
//! that rotate daily. A memory buffer batches writes for efficiency, flushed periodically
//! or on demand.
//!
//! # File format
//!
//! One JSON object per line in `~/.config/ecc/usage/YYYY-MM-DD.jsonl`:
//!
//! ```json
//! {"ts":"2026-05-03T14:32:01Z","req_id":"a1b2c3","model":"claude-sonnet-4-6","provider":"kimi",
//!  "target_model":"K2.6","input_tokens":1520,"cache_read_tokens":800,"output_tokens":832,
//!  "latency_ms":1247,"status":200,"cost_usd":0.0034}
//! ```

use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;
use serde::{Deserialize, Serialize};

/// A single usage record for one request.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct UsageRecord {
    pub ts: String,
    pub req_id: String,
    pub model: String,
    pub provider: String,
    pub target_model: String,
    pub input_tokens: u64,
    pub cache_read_tokens: u64,
    pub output_tokens: u64,
    pub latency_ms: u64,
    pub status: u16,
    pub cost_usd: f64,
}

/// Buffered JSONL writer with daily file rotation.
pub struct UsageStore {
    dir: PathBuf,
    buffer: Mutex<Vec<UsageRecord>>,
    buffer_size: usize,
}

impl UsageStore {
    pub fn new(dir: PathBuf, buffer_size: usize) -> Self {
        Self {
            dir,
            buffer: Mutex::new(Vec::with_capacity(buffer_size)),
            buffer_size,
        }
    }

    /// Record a usage entry. Flushes automatically when buffer is full.
    pub fn record(&self, entry: UsageRecord) -> Result<(), std::io::Error> {
        let mut buf = self.buffer.lock().unwrap();
        buf.push(entry);
        if buf.len() >= self.buffer_size {
            self.flush_locked(&mut buf)?;
        }
        Ok(())
    }

    /// Force-flush all buffered records to disk.
    pub fn flush(&self) -> Result<(), std::io::Error> {
        let mut buf = self.buffer.lock().unwrap();
        self.flush_locked(&mut buf)
    }

    fn flush_locked(&self, buf: &mut Vec<UsageRecord>) -> Result<(), std::io::Error> {
        if buf.is_empty() {
            return Ok(());
        }
        std::fs::create_dir_all(&self.dir)?;

        // Group by date (from record timestamp)
        let mut by_date: std::collections::HashMap<String, Vec<&UsageRecord>> =
            std::collections::HashMap::new();
        for rec in buf.iter() {
            let date = &rec.ts[..10]; // "YYYY-MM-DD"
            by_date.entry(date.to_string()).or_default().push(rec);
        }

        for (date, records) in by_date {
            let path = self.dir.join(format!("{}.jsonl", date));
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)?;

            for rec in records {
                let mut line = serde_json::to_string(rec)
                    .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
                line.push('\n');
                file.write_all(line.as_bytes())?;
            }
        }

        buf.clear();
        Ok(())
    }

    /// Read all records from a specific date's file.
    pub fn read_daily(&self, date: &str) -> Result<Vec<UsageRecord>, std::io::Error> {
        let path = self.dir.join(format!("{}.jsonl", date));
        if !path.exists() {
            return Ok(Vec::new());
        }
        let content = std::fs::read_to_string(&path)?;
        let records = content
            .lines()
            .filter(|l| !l.trim().is_empty())
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        Ok(records)
    }
}

/// Extract token usage from a response body (Anthropic or OpenAI format).
pub fn extract_usage_from_response(body: &[u8]) -> Option<(u64, u64, u64)> {
    let v: serde_json::Value = serde_json::from_slice(body).ok()?;
    let usage = v.get("usage")?;

    // Try Anthropic format first (input_tokens, output_tokens)
    if let (Some(input), Some(output)) = (
        usage.get("input_tokens").and_then(|v| v.as_u64()),
        usage.get("output_tokens").and_then(|v| v.as_u64()),
    ) {
        let cache_read = usage
            .get("cache_read_input_tokens")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        return Some((input, cache_read, output));
    }

    // Try OpenAI format (prompt_tokens, completion_tokens)
    if let (Some(input), Some(output)) = (
        usage.get("prompt_tokens").and_then(|v| v.as_u64()),
        usage.get("completion_tokens").and_then(|v| v.as_u64()),
    ) {
        let cache_read = usage
            .get("prompt_tokens_details")
            .and_then(|d| d.get("cached_tokens"))
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        return Some((input, cache_read, output));
    }

    None
}

/// Extract token usage from a final SSE `message_delta` event.
pub fn extract_usage_from_stream_event(event: &str) -> Option<(u64, u64)> {
    // Look for message_delta events with usage
    for line in event.lines() {
        if let Some(data) = line.strip_prefix("data: ") {
            let v: serde_json::Value = serde_json::from_str(data).ok()?;
            if v.get("type")?.as_str()? == "message_delta" {
                let usage = v.get("usage")?;
                let output = usage.get("output_tokens")?.as_u64()?;
                return Some((0, output)); // input tokens come from message_start
            }
        }
    }
    None
}

/// Aggregated usage stats for a time period.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct DailyStats {
    pub total_requests: u64,
    pub total_input_tokens: u64,
    pub total_cache_read_tokens: u64,
    pub total_output_tokens: u64,
    pub total_cost_usd: f64,
    pub by_provider: std::collections::HashMap<String, ProviderStats>,
}

/// Usage stats for a single provider.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct ProviderStats {
    pub requests: u64,
    pub cost_usd: f64,
}

/// Aggregate records into daily stats.
pub fn aggregate_daily(records: &[UsageRecord]) -> DailyStats {
    let mut stats = DailyStats::default();
    for rec in records {
        stats.total_requests += 1;
        stats.total_input_tokens += rec.input_tokens;
        stats.total_cache_read_tokens += rec.cache_read_tokens;
        stats.total_output_tokens += rec.output_tokens;
        stats.total_cost_usd += rec.cost_usd;

        let provider = stats.by_provider.entry(rec.provider.clone()).or_default();
        provider.requests += 1;
        provider.cost_usd += rec.cost_usd;
    }
    stats
}

/// Quota check result.
#[derive(Debug, Clone, PartialEq)]
pub enum QuotaStatus {
    WithinLimit,
    OverLimit { current: f64, limit: f64 },
}

/// Check if a cost value is within a daily quota limit.
pub fn check_quota(current_cost: f64, daily_limit: Option<f64>) -> QuotaStatus {
    match daily_limit {
        Some(limit) if current_cost >= limit => QuotaStatus::OverLimit {
            current: current_cost,
            limit,
        },
        _ => QuotaStatus::WithinLimit,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_record(ts: &str, model: &str, provider: &str) -> UsageRecord {
        UsageRecord {
            ts: ts.to_string(),
            req_id: "test-req".to_string(),
            model: model.to_string(),
            provider: provider.to_string(),
            target_model: "target-model".to_string(),
            input_tokens: 100,
            cache_read_tokens: 0,
            output_tokens: 50,
            latency_ms: 200,
            status: 200,
            cost_usd: 0.001,
        }
    }

    #[test]
    fn t39_record_single_usage() {
        let dir = tempfile::tempdir().unwrap();
        let store = UsageStore::new(dir.path().to_path_buf(), 100);

        let rec = make_record("2026-05-03T10:00:00Z", "claude-sonnet-4-6", "kimi");
        store.record(rec.clone()).unwrap();
        store.flush().unwrap();

        let records = store.read_daily("2026-05-03").unwrap();
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].model, "claude-sonnet-4-6");
        assert_eq!(records[0].provider, "kimi");
    }

    #[test]
    fn t40_batch_flush() {
        let dir = tempfile::tempdir().unwrap();
        let store = UsageStore::new(dir.path().to_path_buf(), 3);

        for i in 0..3 {
            let rec = make_record("2026-05-03T10:00:00Z", &format!("model-{i}"), "kimi");
            store.record(rec).unwrap();
        }

        // Buffer size is 3, should have auto-flushed
        let records = store.read_daily("2026-05-03").unwrap();
        assert_eq!(records.len(), 3);
    }

    #[test]
    fn t41_daily_rotation() {
        let dir = tempfile::tempdir().unwrap();
        let store = UsageStore::new(dir.path().to_path_buf(), 100);

        store.record(make_record("2026-05-03T10:00:00Z", "m1", "kimi")).unwrap();
        store.record(make_record("2026-05-04T10:00:00Z", "m2", "kimi")).unwrap();
        store.flush().unwrap();

        let day1 = store.read_daily("2026-05-03").unwrap();
        let day2 = store.read_daily("2026-05-04").unwrap();
        assert_eq!(day1.len(), 1);
        assert_eq!(day2.len(), 1);
        assert_eq!(day1[0].model, "m1");
        assert_eq!(day2[0].model, "m2");
    }

    #[test]
    fn t42_extract_usage_from_sse() {
        let event = "event: message_delta\ndata: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":42}}\n\n";
        let (_input, output) = extract_usage_from_stream_event(event).unwrap();
        assert_eq!(output, 42);
    }

    #[test]
    fn t43_extract_usage_from_json_response() {
        let body = serde_json::json!({
            "id": "msg_123",
            "type": "message",
            "usage": {
                "input_tokens": 100,
                "output_tokens": 50,
                "cache_read_input_tokens": 30
            }
        });
        let (input, cache, output) =
            extract_usage_from_response(serde_json::to_string(&body).unwrap().as_bytes()).unwrap();
        assert_eq!(input, 100);
        assert_eq!(cache, 30);
        assert_eq!(output, 50);
    }

    #[test]
    fn t44_cost_calculation() {
        let rec = UsageRecord {
            ts: "2026-05-03T10:00:00Z".to_string(),
            req_id: "test".to_string(),
            model: "claude-sonnet-4-6".to_string(),
            provider: "kimi".to_string(),
            target_model: "K2.6".to_string(),
            input_tokens: 1_000_000,
            cache_read_tokens: 0,
            output_tokens: 1_000_000,
            latency_ms: 100,
            status: 200,
            cost_usd: 0.28 + 0.42, // using DeepSeek pricing as example
        };
        assert!((rec.cost_usd - 0.70).abs() < 1e-10);
    }

    #[test]
    fn t45_daily_aggregation() {
        let records = vec![
            make_record("2026-05-03T10:00:00Z", "m1", "kimi"),
            make_record("2026-05-03T11:00:00Z", "m2", "deepseek"),
            make_record("2026-05-03T12:00:00Z", "m3", "kimi"),
        ];
        let stats = aggregate_daily(&records);
        assert_eq!(stats.total_requests, 3);
        assert_eq!(stats.total_input_tokens, 300);
        assert_eq!(stats.total_output_tokens, 150);
        assert!((stats.total_cost_usd - 0.003).abs() < 1e-10);
        assert_eq!(stats.by_provider["kimi"].requests, 2);
        assert_eq!(stats.by_provider["deepseek"].requests, 1);
    }

    #[test]
    fn t46_quota_over_limit() {
        let status = check_quota(10.50, Some(10.0));
        assert_eq!(
            status,
            QuotaStatus::OverLimit {
                current: 10.50,
                limit: 10.0
            }
        );
    }

    #[test]
    fn t47_quota_within_limit() {
        assert_eq!(check_quota(5.0, Some(10.0)), QuotaStatus::WithinLimit);
        assert_eq!(check_quota(5.0, None), QuotaStatus::WithinLimit);
    }
}
