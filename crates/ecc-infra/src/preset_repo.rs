use std::sync::Arc;

use rusqlite::{params, OptionalExtension};

use ecc_domain::preset::Preset;
use ecc_domain::repository::{PresetRepository, RepositoryError};

use crate::store::{self, SqliteRepo};

pub struct PresetRepo {
    store: Arc<SqliteRepo>,
}

impl PresetRepo {
    pub fn new(store: Arc<SqliteRepo>) -> Self {
        Self { store }
    }
}

impl PresetRepository for PresetRepo {
    fn list_presets(&self) -> Result<Vec<Preset>, RepositoryError> {
        let conn = self.store.conn()?;
        let mut stmt = conn.prepare("SELECT data FROM presets").map_err(store::db_err)?;
        let rows = stmt.query_map([], |row| row.get::<_, String>("data")).map_err(store::db_err)?;
        let mut presets = Vec::new();
        for row in rows {
            let json = row.map_err(store::db_err)?;
            presets.push(serde_json::from_str(&json).map_err(store::db_err)?);
        }
        Ok(presets)
    }

    fn get_preset(&self, name: &str) -> Result<Option<Preset>, RepositoryError> {
        let conn = self.store.conn()?;
        let result: Option<String> = conn
            .query_row(
                "SELECT data FROM presets WHERE name = ?1",
                params![name],
                |row| row.get("data"),
            )
            .optional()
            .map_err(store::db_err)?;
        match result {
            Some(json) => Ok(Some(serde_json::from_str(&json).map_err(store::db_err)?)),
            None => Ok(None),
        }
    }

    fn save_preset(&self, preset: &Preset) -> Result<(), RepositoryError> {
        let conn = self.store.conn()?;
        let json = serde_json::to_string(preset).map_err(store::db_err)?;
        conn.execute(
            "INSERT OR REPLACE INTO presets (name, data) VALUES (?1, ?2)",
            params![preset.name, json],
        )
        .map_err(store::db_err)?;
        Ok(())
    }

    fn delete_preset(&self, name: &str) -> Result<(), RepositoryError> {
        let conn = self.store.conn()?;
        let rows = conn
            .execute("DELETE FROM presets WHERE name = ?1", params![name])
            .map_err(store::db_err)?;
        if rows == 0 {
            return Err(RepositoryError::NotFound(format!("preset '{name}'")));
        }
        Ok(())
    }

    fn is_presets_empty(&self) -> Result<bool, RepositoryError> {
        let conn = self.store.conn()?;
        let count: i64 = conn
            .query_row("SELECT COUNT(*) as cnt FROM presets", [], |row| row.get("cnt"))
            .map_err(store::db_err)?;
        Ok(count == 0)
    }

    fn seed_presets(&self, presets: &[Preset]) -> Result<(), RepositoryError> {
        let conn = self.store.conn()?;
        for preset in presets {
            let json = serde_json::to_string(preset).map_err(store::db_err)?;
            conn.execute(
                "INSERT OR IGNORE INTO presets (name, data) VALUES (?1, ?2)",
                params![preset.name, json],
            )
            .map_err(store::db_err)?;
        }
        Ok(())
    }
}
