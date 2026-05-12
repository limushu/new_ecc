use rusqlite::Connection;

use ecc_domain::repository::RepositoryError;

pub fn init_schema(conn: &Connection) -> Result<(), RepositoryError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS providers (
            name           TEXT PRIMARY KEY,
            base_url       TEXT NOT NULL,
            auth_token     TEXT NOT NULL,
            auth_type      TEXT NOT NULL DEFAULT 'bearer',
            protocol       TEXT NOT NULL DEFAULT 'anthropic',
            is_coding_plan INTEGER NOT NULL DEFAULT 0,
            quota_adapter  TEXT
        );
        CREATE TABLE IF NOT EXISTS model_mappings (
            provider_name  TEXT NOT NULL,
            claude_model   TEXT NOT NULL,
            provider_model TEXT NOT NULL,
            PRIMARY KEY (provider_name, claude_model)
        );
        CREATE TABLE IF NOT EXISTS pricing (
            provider_name    TEXT NOT NULL,
            model            TEXT NOT NULL,
            input_per_m      REAL NOT NULL,
            output_per_m     REAL NOT NULL,
            cache_read_per_m REAL,
            PRIMARY KEY (provider_name, model)
        );
        CREATE TABLE IF NOT EXISTS presets (
            name TEXT PRIMARY KEY,
            data TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS routes (
            claude_model   TEXT NOT NULL,
            provider_name  TEXT NOT NULL,
            provider_model TEXT NOT NULL,
            priority       INTEGER NOT NULL DEFAULT 1,
            PRIMARY KEY (claude_model, provider_name)
        );
        CREATE TABLE IF NOT EXISTS config (
            key   TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );
        CREATE TABLE IF NOT EXISTS usage_records (
            id                INTEGER PRIMARY KEY AUTOINCREMENT,
            timestamp         TEXT NOT NULL,
            provider_name     TEXT NOT NULL,
            target_model      TEXT NOT NULL,
            requested_model   TEXT NOT NULL,
            input_tokens      INTEGER NOT NULL DEFAULT 0,
            output_tokens     INTEGER NOT NULL DEFAULT 0,
            cache_read_tokens INTEGER NOT NULL DEFAULT 0,
            cost_usd          REAL NOT NULL DEFAULT 0.0,
            latency_ms        INTEGER NOT NULL DEFAULT 0,
            status            INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_usage_ts ON usage_records(timestamp);
        CREATE INDEX IF NOT EXISTS idx_routes_model ON routes(claude_model, priority);
        CREATE TABLE IF NOT EXISTS session_records (
            id               INTEGER PRIMARY KEY AUTOINCREMENT,
            session_id       TEXT NOT NULL,
            timestamp        TEXT NOT NULL,
            provider_name    TEXT NOT NULL,
            target_model     TEXT NOT NULL,
            requested_model  TEXT NOT NULL,
            request_body     TEXT NOT NULL,
            response_body    TEXT NOT NULL,
            assistant_text   TEXT NOT NULL DEFAULT '',
            thinking_text    TEXT NOT NULL DEFAULT '',
            input_tokens     INTEGER NOT NULL DEFAULT 0,
            output_tokens    INTEGER NOT NULL DEFAULT 0,
            latency_ms       INTEGER NOT NULL DEFAULT 0,
            status           INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_session_id ON session_records(session_id, timestamp);",
    ).map_err(|e| RepositoryError::Storage(e.into()))?;
    Ok(())
}
