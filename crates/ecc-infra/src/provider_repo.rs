use std::collections::HashMap;
use std::sync::Arc;

use rusqlite::params;

use ecc_domain::mapping::ModelMapping;
use ecc_domain::provider::{AuthType, Protocol, Provider};
use ecc_domain::repository::{ProviderRepository, RepositoryError};
use ecc_domain::Pricing;

use crate::cache::ProviderCache;
use crate::store::{self, SqliteRepo};

pub struct ProviderRepo {
    store: Arc<SqliteRepo>,
    cache: ProviderCache,
}

impl ProviderRepo {
    pub fn new(store: Arc<SqliteRepo>) -> Result<Self, RepositoryError> {
        let repo = Self {
            store,
            cache: ProviderCache::new(),
        };
        repo.reload_cache()?;
        Ok(repo)
    }

    pub fn cache(&self) -> &ProviderCache {
        &self.cache
    }

    pub fn reload_cache(&self) -> Result<(), RepositoryError> {
        let providers = self.load_all_from_db()?;
        self.cache.clear_and_fill(
            providers.into_iter().map(|p| (p.name.clone(), p)).collect(),
        );
        Ok(())
    }

    fn load_all_from_db(&self) -> Result<Vec<Provider>, RepositoryError> {
        let conn = self.store.conn()?;

        let mut stmt = conn
            .prepare(
                "SELECT name, base_url, auth_token, auth_type, protocol, is_coding_plan, quota_adapter FROM providers",
            )
            .map_err(store::db_err)?;

        let rows = stmt.query_map([], |row| {
            Ok((
                row.get::<_, String>("name")?,
                row.get::<_, String>("base_url")?,
                row.get::<_, String>("auth_token")?,
                row.get::<_, String>("auth_type")?,
                row.get::<_, String>("protocol")?,
                row.get::<_, bool>("is_coding_plan")?,
                row.get::<_, Option<String>>("quota_adapter")?,
            ))
        }).map_err(store::db_err)?;

        let mut providers = Vec::new();
        for row in rows {
            let (name, base_url, enc_token, auth_s, proto_s, is_cp, quota_json) =
                row.map_err(store::db_err)?;
            let auth_token = self.store.decrypt_token(&enc_token)?;

            let mut provider = Provider {
                name,
                base_url,
                auth_token,
                auth_type: AuthType::from_str(&auth_s),
                protocol: Protocol::from_str(&proto_s),
                is_coding_plan: is_cp,
                model_mappings: Vec::new(),
                pricing: HashMap::new(),
                quota_adapter: quota_json.as_deref().and_then(|s| serde_json::from_str(s).ok()),
            };

            Self::load_mappings(&conn, &mut provider)?;
            Self::load_pricing(&conn, &mut provider)?;

            providers.push(provider);
        }

        Ok(providers)
    }

    fn load_mappings(conn: &rusqlite::Connection, provider: &mut Provider) -> Result<(), RepositoryError> {
        let mut stmt = conn
            .prepare("SELECT claude_model, provider_model FROM model_mappings WHERE provider_name = ?")
            .map_err(store::db_err)?;
        let rows = stmt.query_map(params![provider.name], |row| {
            Ok(ModelMapping {
                claude_model: row.get("claude_model")?,
                provider_model: row.get("provider_model")?,
            })
        }).map_err(store::db_err)?;
        for m in rows {
            provider.model_mappings.push(m.map_err(store::db_err)?);
        }
        Ok(())
    }

    fn load_pricing(conn: &rusqlite::Connection, provider: &mut Provider) -> Result<(), RepositoryError> {
        let mut stmt = conn
            .prepare("SELECT model, input_per_m, output_per_m, cache_read_per_m FROM pricing WHERE provider_name = ?")
            .map_err(store::db_err)?;
        let rows = stmt.query_map(params![provider.name], |row| {
            Ok((
                row.get::<_, String>("model")?,
                row.get::<_, f64>("input_per_m")?,
                row.get::<_, f64>("output_per_m")?,
                row.get::<_, Option<f64>>("cache_read_per_m")?,
            ))
        }).map_err(store::db_err)?;
        for p in rows {
            let (model, input, output, cache) = p.map_err(store::db_err)?;
            provider.pricing.insert(model, Pricing {
                input_per_m: input,
                output_per_m: output,
                cache_read_per_m: cache,
            });
        }
        Ok(())
    }

    fn save_to_db(&self, provider: &Provider) -> Result<(), RepositoryError> {
        let conn = self.store.conn()?;
        let enc_token = self.store.encrypt_token(&provider.auth_token)?;
        let quota_json = provider.quota_adapter.as_ref().map(|qa| serde_json::to_string(qa).unwrap_or_default());

        conn.execute(
            "INSERT OR REPLACE INTO providers (name, base_url, auth_token, auth_type, protocol, is_coding_plan, quota_adapter) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![provider.name, provider.base_url, enc_token, provider.auth_type.to_str(), provider.protocol.to_str(), provider.is_coding_plan, quota_json],
        ).map_err(store::db_err)?;

        conn.execute("DELETE FROM model_mappings WHERE provider_name = ?1", params![provider.name]).map_err(store::db_err)?;
        for m in &provider.model_mappings {
            conn.execute(
                "INSERT INTO model_mappings (provider_name, claude_model, provider_model) VALUES (?1, ?2, ?3)",
                params![provider.name, m.claude_model, m.provider_model],
            ).map_err(store::db_err)?;
        }

        conn.execute("DELETE FROM pricing WHERE provider_name = ?1", params![provider.name]).map_err(store::db_err)?;
        for (model, p) in &provider.pricing {
            conn.execute(
                "INSERT INTO pricing (provider_name, model, input_per_m, output_per_m, cache_read_per_m) VALUES (?1, ?2, ?3, ?4, ?5)",
                params![provider.name, model, p.input_per_m, p.output_per_m, p.cache_read_per_m],
            ).map_err(store::db_err)?;
        }

        Ok(())
    }
}

impl ProviderRepository for ProviderRepo {
    fn list(&self) -> Result<Vec<Provider>, RepositoryError> {
        Ok(self.cache.list_values())
    }

    fn get(&self, name: &str) -> Result<Option<Provider>, RepositoryError> {
        Ok(self.cache.get(name))
    }

    fn save(&self, provider: &Provider) -> Result<(), RepositoryError> {
        self.save_to_db(provider)?;
        self.cache.set(provider.name.clone(), provider.clone());
        Ok(())
    }

    fn delete(&self, name: &str) -> Result<(), RepositoryError> {
        let conn = self.store.conn()?;
        let rows = conn.execute("DELETE FROM providers WHERE name = ?1", params![name]).map_err(store::db_err)?;
        if rows == 0 {
            return Err(RepositoryError::NotFound(format!("provider '{name}'")));
        }
        self.cache.remove(&name.to_string());
        Ok(())
    }
}
