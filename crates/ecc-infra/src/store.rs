use std::path::Path;

use r2d2::Pool;
use r2d2_sqlite::SqliteConnectionManager;

use ecc_domain::repository::RepositoryError;

use crate::crypto;
use crate::schema;

pub struct SqliteRepo {
    pool: Pool<SqliteConnectionManager>,
    pub crypto_key: Vec<u8>,
}

impl SqliteRepo {
    pub fn open(path: &Path, crypto_seed: &str) -> Result<Self, RepositoryError> {
        let manager = SqliteConnectionManager::file(path);
        let pool = Pool::builder()
            .max_size(4)
            .build(manager)
            .map_err(|e| RepositoryError::Storage(e.into()))?;

        // Init schema on the first connection
        let conn = pool.get().map_err(|e| RepositoryError::Storage(e.into()))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .map_err(|e| RepositoryError::Storage(e.into()))?;
        schema::init_schema(&conn)?;

        Ok(Self {
            pool,
            crypto_key: crypto::derive_key(crypto_seed),
        })
    }

    pub fn open_in_memory() -> Result<Self, RepositoryError> {
        let manager = SqliteConnectionManager::memory();
        let pool = Pool::builder()
            .max_size(1)
            .build(manager)
            .map_err(|e| RepositoryError::Storage(e.into()))?;

        let conn = pool.get().map_err(|e| RepositoryError::Storage(e.into()))?;
        schema::init_schema(&conn)?;

        Ok(Self {
            pool,
            crypto_key: crypto::derive_key("test-seed"),
        })
    }

    /// Get a connection from the pool.
    pub fn conn(&self) -> Result<r2d2::PooledConnection<SqliteConnectionManager>, RepositoryError> {
        self.pool.get().map_err(|e| RepositoryError::Storage(e.into()))
    }

    pub fn encrypt_token(&self, plain: &str) -> Result<String, RepositoryError> {
        crypto::encrypt(&self.crypto_key, plain).map_err(|e| RepositoryError::Storage(e.into()))
    }

    pub fn decrypt_token(&self, enc: &str) -> Result<String, RepositoryError> {
        crypto::decrypt(&self.crypto_key, enc).map_err(|e| RepositoryError::Storage(e.into()))
    }
}

/// Convert any error into RepositoryError::Storage.
pub fn db_err(e: impl Into<Box<dyn std::error::Error + Send + Sync>>) -> RepositoryError {
    RepositoryError::Storage(e.into())
}
