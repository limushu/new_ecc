use std::collections::HashMap;
use std::sync::Arc;

use ecc_domain::mapping::RouteTarget;
use ecc_domain::repository::{ProviderRepository, RouteRepository, RepositoryError};

use crate::cache::RouteCache;
use crate::provider_repo::ProviderRepo;

pub struct RouteRepo {
    cache: RouteCache,
    provider_repo: Arc<ProviderRepo>,
}

impl RouteRepo {
    pub fn new(provider_repo: Arc<ProviderRepo>) -> Result<Self, RepositoryError> {
        let repo = Self {
            cache: RouteCache::new(),
            provider_repo,
        };
        repo.rebuild()?;
        Ok(repo)
    }

    pub fn cache(&self) -> &RouteCache {
        &self.cache
    }
}

impl RouteRepository for RouteRepo {
    fn get_routes(&self, claude_model: &str) -> Result<Option<Vec<RouteTarget>>, RepositoryError> {
        Ok(self.cache.get(claude_model))
    }

    fn list_routes(&self) -> Result<HashMap<String, Vec<RouteTarget>>, RepositoryError> {
        Ok(self.cache.list_all())
    }

    fn rebuild(&self) -> Result<(), RepositoryError> {
        let providers = self.provider_repo.list()?;
        let mut route_map = HashMap::new();
        for provider in &providers {
            for mapping in &provider.model_mappings {
                route_map
                    .entry(mapping.claude_model.clone())
                    .or_insert_with(Vec::new)
                    .push(RouteTarget {
                        provider_name: provider.name.clone(),
                        provider_model: mapping.provider_model.clone(),
                    });
            }
        }
        self.cache.clear_and_fill(route_map);
        Ok(())
    }
}
