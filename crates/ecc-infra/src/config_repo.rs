use std::sync::Arc;

use rusqlite::{params, OptionalExtension};

use ecc_domain::repository::{ConfigRepository, RepositoryError};

use crate::store::{self, SqliteRepo};

pub struct ConfigRepo {
    store: Arc<SqliteRepo>,
}

impl ConfigRepo {
    pub fn new(store: Arc<SqliteRepo>) -> Self {
        Self { store }
    }
}

impl ConfigRepository for ConfigRepo {
    fn get_default_provider(&self) -> Result<Option<String>, RepositoryError> {
        let conn = self.store.conn()?;
        conn.query_row(
            "SELECT value FROM config WHERE key = 'default_provider'",
            [],
            |row| row.get("value"),
        )
        .optional()
        .map_err(store::db_err)
    }

    fn set_default_provider(&self, name: &str) -> Result<(), RepositoryError> {
        let conn = self.store.conn()?;
        conn.execute(
            "INSERT OR REPLACE INTO config (key, value) VALUES ('default_provider', ?1)",
            params![name],
        )
        .map_err(store::db_err)?;
        Ok(())
    }
}
