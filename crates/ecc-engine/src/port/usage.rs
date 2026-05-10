use ecc_domain::repository::{RepositoryError, UsageRecord};

/// Port consumed by UsageTracker middleware — records usage data.
pub trait UsagePort: Send + Sync {
    fn record(&self, record: UsageRecord) -> Result<(), RepositoryError>;
}

impl<T: ecc_domain::repository::UsageRepository + Send + Sync> UsagePort for T {
    fn record(&self, record: UsageRecord) -> Result<(), RepositoryError> {
        ecc_domain::repository::UsageRepository::record(self, record)
    }
}
