use ecc_domain::mapping::RouteTarget;
use ecc_domain::provider::Provider;
use ecc_domain::repository::RepositoryError;

/// Port consumed by Router middleware — resolves routes and fetches provider config.
pub trait RoutePort: Send + Sync {
    fn find_routes(&self, model: &str) -> Result<Option<Vec<RouteTarget>>, RepositoryError>;
    fn get_provider(&self, name: &str) -> Result<Option<Provider>, RepositoryError>;
}

/// Blanket impl: any type that implements both domain traits satisfies RoutePort.
impl<T> RoutePort for T
where
    T: ecc_domain::repository::RouteRepository + ecc_domain::repository::ProviderRepository + Send + Sync,
{
    fn find_routes(&self, model: &str) -> Result<Option<Vec<RouteTarget>>, RepositoryError> {
        ecc_domain::repository::RouteRepository::get_routes(self, model)
    }

    fn get_provider(&self, name: &str) -> Result<Option<Provider>, RepositoryError> {
        ecc_domain::repository::ProviderRepository::get(self, name)
    }
}
