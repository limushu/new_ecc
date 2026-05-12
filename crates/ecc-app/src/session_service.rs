use std::sync::Arc;

use ecc_domain::repository::{RepositoryError, SessionRecord, SessionRepository, SessionSummary};

pub struct SessionService<S: SessionRepository> {
    session_repo: Arc<S>,
}

impl<S: SessionRepository> SessionService<S> {
    pub fn new(session_repo: Arc<S>) -> Self {
        Self { session_repo }
    }

    pub fn list_sessions(&self, limit: u64) -> Result<Vec<SessionSummary>, RepositoryError> {
        self.session_repo.list_sessions(limit)
    }

    pub fn get_session(&self, session_id: &str) -> Result<Vec<SessionRecord>, RepositoryError> {
        self.session_repo.get_session(session_id)
    }

    pub fn delete_session(&self, session_id: &str) -> Result<(), RepositoryError> {
        self.session_repo.delete_session(session_id)
    }
}
