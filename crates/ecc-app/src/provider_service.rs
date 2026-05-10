use std::collections::HashMap;

use ecc_domain::mapping::ModelMapping;
use ecc_domain::preset::QuotaAdapter;
use ecc_domain::provider::{AuthType, Protocol, Provider};
use ecc_domain::pricing::Pricing;
use ecc_domain::repository::{
    ConfigRepository, ProviderRepository, RepositoryError, RouteRepository,
};

pub struct CreateProviderCommand {
    pub name: String,
    pub base_url: String,
    pub auth_token: String,
    pub auth_type: AuthType,
    pub protocol: Protocol,
    pub is_coding_plan: bool,
    pub model_mappings: Vec<ModelMapping>,
    pub pricing: HashMap<String, Pricing>,
    pub quota_adapter: Option<QuotaAdapter>,
}

pub struct UpdateProviderCommand {
    pub base_url: Option<String>,
    pub auth_token: Option<String>,
    pub auth_type: Option<AuthType>,
    pub protocol: Option<Protocol>,
    pub is_coding_plan: Option<bool>,
    pub model_mappings: Option<Vec<ModelMapping>>,
    pub pricing: Option<HashMap<String, Pricing>>,
    pub quota_adapter: Option<Option<QuotaAdapter>>,
}

pub struct ProviderService<P, C, R>
where
    P: ProviderRepository,
    C: ConfigRepository,
    R: RouteRepository,
{
    provider_repo: P,
    config_repo: C,
    route_repo: R,
}

impl<P, C, R> ProviderService<P, C, R>
where
    P: ProviderRepository,
    C: ConfigRepository,
    R: RouteRepository,
{
    pub fn new(provider_repo: P, config_repo: C, route_repo: R) -> Self {
        Self { provider_repo, config_repo, route_repo }
    }

    pub fn list_providers(&self) -> Result<Vec<Provider>, RepositoryError> {
        self.provider_repo.list()
    }

    pub fn get_provider(&self, name: &str) -> Result<Option<Provider>, RepositoryError> {
        self.provider_repo.get(name)
    }

    pub fn create_provider(&self, cmd: CreateProviderCommand) -> Result<Provider, RepositoryError> {
        if cmd.name.is_empty() {
            return Err(RepositoryError::NotFound("provider name is empty".into()));
        }
        if self.provider_repo.get(&cmd.name)?.is_some() {
            return Err(RepositoryError::NotFound(format!(
                "provider '{}' already exists",
                cmd.name
            )));
        }

        let provider = Provider {
            name: cmd.name,
            base_url: cmd.base_url,
            auth_token: cmd.auth_token,
            auth_type: cmd.auth_type,
            protocol: cmd.protocol,
            is_coding_plan: cmd.is_coding_plan,
            model_mappings: cmd.model_mappings,
            pricing: cmd.pricing,
            quota_adapter: cmd.quota_adapter,
        };

        self.provider_repo.save(&provider)?;
        self.route_repo.rebuild()?;
        Ok(provider)
    }

    pub fn update_provider(
        &self,
        name: &str,
        cmd: UpdateProviderCommand,
    ) -> Result<Provider, RepositoryError> {
        let mut provider = self
            .provider_repo
            .get(name)?
            .ok_or_else(|| RepositoryError::NotFound(format!("provider '{name}' not found")))?;

        if let Some(v) = cmd.base_url {
            provider.base_url = v;
        }
        if let Some(v) = cmd.auth_token {
            provider.auth_token = v;
        }
        if let Some(v) = cmd.auth_type {
            provider.auth_type = v;
        }
        if let Some(v) = cmd.protocol {
            provider.protocol = v;
        }
        if let Some(v) = cmd.is_coding_plan {
            provider.is_coding_plan = v;
        }
        if let Some(v) = cmd.model_mappings {
            provider.model_mappings = v;
        }
        if let Some(v) = cmd.pricing {
            provider.pricing = v;
        }
        if let Some(v) = cmd.quota_adapter {
            provider.quota_adapter = v;
        }

        self.provider_repo.save(&provider)?;
        self.route_repo.rebuild()?;
        Ok(provider)
    }

    pub fn delete_provider(&self, name: &str) -> Result<(), RepositoryError> {
        self.provider_repo.get(name)?.ok_or_else(|| {
            RepositoryError::NotFound(format!("provider '{name}' not found"))
        })?;
        self.provider_repo.delete(name)?;
        self.route_repo.rebuild()?;
        Ok(())
    }

    pub fn set_default_provider(&self, name: &str) -> Result<(), RepositoryError> {
        self.config_repo.set_default_provider(name)
    }

    pub fn get_default_provider(&self) -> Result<Option<Provider>, RepositoryError> {
        let name = self.config_repo.get_default_provider()?;
        match name {
            Some(n) => self.provider_repo.get(&n),
            None => Ok(None),
        }
    }
}
