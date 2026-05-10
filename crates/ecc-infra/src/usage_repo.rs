use std::sync::Arc;

use chrono::{DateTime, Utc};
use rusqlite::params;

use ecc_domain::repository::{ProviderUsage, RepositoryError, UsageRecord, UsageRepository};

use crate::store::{self, SqliteRepo};

fn parse_ts(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_default()
}

/// SQLite stores integers as i64. Cast to u64 for domain types.
fn u(v: i64) -> u64 {
    v as u64
}

pub struct UsageRepo {
    store: Arc<SqliteRepo>,
}

impl UsageRepo {
    pub fn new(store: Arc<SqliteRepo>) -> Self {
        Self { store }
    }
}

impl UsageRepository for UsageRepo {
    fn record(&self, record: UsageRecord) -> Result<(), RepositoryError> {
        let conn = self.store.conn()?;
        conn.execute(
            "INSERT INTO usage_records (timestamp, provider_name, target_model, requested_model, input_tokens, output_tokens, cache_read_tokens, cost_usd, latency_ms, status) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
            params![
                record.timestamp.to_rfc3339(),
                record.provider_name,
                record.target_model,
                record.requested_model,
                record.input_tokens as i64,
                record.output_tokens as i64,
                record.cache_read_tokens as i64,
                record.cost_usd,
                record.latency_ms as i64,
                record.status as i64,
            ],
        ).map_err(store::db_err)?;
        Ok(())
    }

    fn query(
        &self,
        start: &DateTime<Utc>,
        end: &DateTime<Utc>,
    ) -> Result<Vec<UsageRecord>, RepositoryError> {
        let conn = self.store.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, timestamp, provider_name, target_model, requested_model, input_tokens, output_tokens, cache_read_tokens, cost_usd, latency_ms, status FROM usage_records WHERE timestamp >= ?1 AND timestamp <= ?2 ORDER BY timestamp DESC",
            )
            .map_err(store::db_err)?;

        let rows = stmt
            .query_map(params![start.to_rfc3339(), end.to_rfc3339()], |row| {
                let ts: String = row.get("timestamp")?;
                Ok(UsageRecord {
                    id: row.get::<_, i64>("id")?.to_string(),
                    timestamp: parse_ts(&ts),
                    provider_name: row.get("provider_name")?,
                    target_model: row.get("target_model")?,
                    requested_model: row.get("requested_model")?,
                    input_tokens: u(row.get::<_, i64>("input_tokens")?),
                    output_tokens: u(row.get::<_, i64>("output_tokens")?),
                    cache_read_tokens: u(row.get::<_, i64>("cache_read_tokens")?),
                    cost_usd: row.get("cost_usd")?,
                    latency_ms: u(row.get::<_, i64>("latency_ms")?),
                    status: row.get::<_, i64>("status")? as u16,
                })
            })
            .map_err(store::db_err)?;

        let mut records = Vec::new();
        for row in rows {
            records.push(row.map_err(store::db_err)?);
        }
        Ok(records)
    }

    fn aggregate_by_provider(
        &self,
        start: &DateTime<Utc>,
        end: &DateTime<Utc>,
    ) -> Result<Vec<ProviderUsage>, RepositoryError> {
        let conn = self.store.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT provider_name, COUNT(*) as total_requests, SUM(input_tokens) as total_input_tokens, SUM(output_tokens) as total_output_tokens, SUM(cache_read_tokens) as total_cache_read_tokens, SUM(cost_usd) as total_cost_usd FROM usage_records WHERE timestamp >= ?1 AND timestamp <= ?2 GROUP BY provider_name",
            )
            .map_err(store::db_err)?;

        let rows = stmt
            .query_map(params![start.to_rfc3339(), end.to_rfc3339()], |row| {
                Ok(ProviderUsage {
                    provider_name: row.get("provider_name")?,
                    total_requests: u(row.get::<_, i64>("total_requests")?),
                    total_input_tokens: u(row.get::<_, i64>("total_input_tokens")?),
                    total_output_tokens: u(row.get::<_, i64>("total_output_tokens")?),
                    total_cache_read_tokens: u(row.get::<_, i64>("total_cache_read_tokens")?),
                    total_cost_usd: row.get("total_cost_usd")?,
                })
            })
            .map_err(store::db_err)?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(store::db_err)?);
        }
        Ok(result)
    }
}
