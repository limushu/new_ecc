use std::sync::Arc;

use chrono::{DateTime, Utc};
use rusqlite::params;

use ecc_domain::repository::{RepositoryError, SessionRecord, SessionRepository, SessionSummary};

use crate::store::{self, SqliteRepo};

fn parse_ts(s: &str) -> DateTime<Utc> {
    DateTime::parse_from_rfc3339(s)
        .map(|dt| dt.with_timezone(&Utc))
        .unwrap_or_default()
}

fn u(v: i64) -> u64 {
    v as u64
}

pub struct SessionRepo {
    store: Arc<SqliteRepo>,
}

impl SessionRepo {
    pub fn new(store: Arc<SqliteRepo>) -> Self {
        Self { store }
    }
}

impl SessionRepository for SessionRepo {
    fn record(&self, record: SessionRecord) -> Result<(), RepositoryError> {
        let conn = self.store.conn()?;
        conn.execute(
            "INSERT INTO session_records (session_id, timestamp, provider_name, target_model, requested_model, request_body, response_body, assistant_text, thinking_text, input_tokens, output_tokens, latency_ms, status) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            params![
                record.session_id,
                record.timestamp.to_rfc3339(),
                record.provider_name,
                record.target_model,
                record.requested_model,
                record.request_body,
                record.response_body,
                record.assistant_text,
                record.thinking_text,
                record.input_tokens as i64,
                record.output_tokens as i64,
                record.latency_ms as i64,
                record.status as i64,
            ],
        ).map_err(store::db_err)?;
        Ok(())
    }

    fn list_sessions(&self, limit: u64) -> Result<Vec<SessionSummary>, RepositoryError> {
        let conn = self.store.conn()?;
        let mut stmt = conn.prepare(
            "SELECT session_id, MIN(timestamp) as first_ts, MAX(timestamp) as last_ts, COUNT(*) as total_turns, provider_name, requested_model FROM session_records GROUP BY session_id ORDER BY MAX(timestamp) DESC LIMIT ?1",
        ).map_err(store::db_err)?;

        let rows = stmt
            .query_map(params![limit as i64], |row| {
                Ok(SessionSummary {
                    session_id: row.get("session_id")?,
                    first_timestamp: parse_ts(&row.get::<_, String>("first_ts")?),
                    last_timestamp: parse_ts(&row.get::<_, String>("last_ts")?),
                    total_turns: u(row.get::<_, i64>("total_turns")?),
                    provider_name: row.get("provider_name")?,
                    requested_model: row.get("requested_model")?,
                })
            })
            .map_err(store::db_err)?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(store::db_err)?);
        }
        Ok(result)
    }

    fn get_session(&self, session_id: &str) -> Result<Vec<SessionRecord>, RepositoryError> {
        let conn = self.store.conn()?;
        let mut stmt = conn
            .prepare(
                "SELECT id, session_id, timestamp, provider_name, target_model, requested_model, request_body, response_body, assistant_text, thinking_text, input_tokens, output_tokens, latency_ms, status FROM session_records WHERE session_id = ?1 ORDER BY timestamp ASC",
            )
            .map_err(store::db_err)?;

        let rows = stmt
            .query_map(params![session_id], |row| {
                let ts: String = row.get("timestamp")?;
                Ok(SessionRecord {
                    id: row.get::<_, i64>("id")?.to_string(),
                    session_id: row.get("session_id")?,
                    timestamp: parse_ts(&ts),
                    provider_name: row.get("provider_name")?,
                    target_model: row.get("target_model")?,
                    requested_model: row.get("requested_model")?,
                    request_body: row.get("request_body")?,
                    response_body: row.get("response_body")?,
                    assistant_text: row.get("assistant_text")?,
                    thinking_text: row.get("thinking_text")?,
                    input_tokens: u(row.get::<_, i64>("input_tokens")?),
                    output_tokens: u(row.get::<_, i64>("output_tokens")?),
                    latency_ms: u(row.get::<_, i64>("latency_ms")?),
                    status: row.get::<_, i64>("status")? as u16,
                })
            })
            .map_err(store::db_err)?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(store::db_err)?);
        }
        Ok(result)
    }

    fn delete_session(&self, session_id: &str) -> Result<(), RepositoryError> {
        let conn = self.store.conn()?;
        conn.execute("DELETE FROM session_records WHERE session_id = ?1", params![session_id])
            .map_err(store::db_err)?;
        Ok(())
    }

    fn find_latest_by_prefix(&self, base_hash: &str) -> Result<Option<(String, DateTime<Utc>)>, RepositoryError> {
        let conn = self.store.conn()?;
        let like = base_hash.to_string() + "%";
        let mut stmt = conn.prepare(
            "SELECT session_id, MAX(timestamp) as last_ts FROM session_records WHERE session_id LIKE ?1 GROUP BY session_id ORDER BY MAX(timestamp) DESC LIMIT 1",
        ).map_err(store::db_err)?;

        let rows = stmt
            .query_map(params![like], |row| {
                let sid: String = row.get("session_id")?;
                let ts: String = row.get("last_ts")?;
                Ok((sid, ts))
            })
            .map_err(store::db_err)?;

        for row in rows {
            let (sid, ts) = row.map_err(store::db_err)?;
            return Ok(Some((sid, parse_ts(&ts))));
        }
        Ok(None)
    }
}
