use std::sync::Arc;

use chrono::{DateTime, Utc};
use ecc_domain::repository::{ProviderUsage, RepositoryError, UsageRecord, UsageRepository};

pub struct UsageService<U: UsageRepository> {
    usage_repo: Arc<U>,
}

impl<U: UsageRepository> UsageService<U> {
    pub fn new(usage_repo: Arc<U>) -> Self {
        Self { usage_repo }
    }

    pub fn query(
        &self,
        start: &DateTime<Utc>,
        end: &DateTime<Utc>,
    ) -> Result<Vec<UsageRecord>, RepositoryError> {
        self.usage_repo.query(start, end)
    }

    pub fn aggregate(
        &self,
        start: &DateTime<Utc>,
        end: &DateTime<Utc>,
    ) -> Result<Vec<ProviderUsage>, RepositoryError> {
        self.usage_repo.aggregate_by_provider(start, end)
    }
}
