use chrono::{DateTime, Utc};
use ecc_domain::repository::{RepositoryError, SessionRecord};

/// Port consumed by SessionRecorder middleware.
pub trait SessionPort: Send + Sync {
    fn record(&self, record: SessionRecord) -> Result<(), RepositoryError>;
    fn find_latest_by_prefix(&self, base_hash: &str) -> Result<Option<(String, DateTime<Utc>)>, RepositoryError>;
}

impl<T: ecc_domain::repository::SessionRepository + Send + Sync> SessionPort for T {
    fn record(&self, record: SessionRecord) -> Result<(), RepositoryError> {
        ecc_domain::repository::SessionRepository::record(self, record)
    }

    fn find_latest_by_prefix(&self, base_hash: &str) -> Result<Option<(String, DateTime<Utc>)>, RepositoryError> {
        ecc_domain::repository::SessionRepository::find_latest_by_prefix(self, base_hash)
    }
}
