use std::sync::Arc;

use futures_util::future::BoxFuture;
use rusqlite::OptionalExtension;
use tokio_util::sync::CancellationToken;

use crate::db::Database;
use crate::error::AppError;
use crate::plugins::PluginRegistry;
use crate::plugins::manifest::{PluginKind, ProviderContributionId};
use crate::providers::github::GithubProvider;
use crate::providers::gitlab::GitlabProvider;
use crate::providers::http::ScopedHttpClient;
use crate::providers::model::{
    AccountIdentity, AdapterListRequest, AdapterPage, InstanceMetadata, ProviderKind,
    RemoteRepository, RemoteRepositoryIdentity,
};
use crate::providers::url::{NormalizedInstance, NormalizedRemoteUrl};

pub struct AdapterAccountContext<'a> {
    pub client: &'a ScopedHttpClient,
    pub secret: &'a str,
    pub cancellation: &'a CancellationToken,
}

pub trait RepositoryDiscoveryProvider: Send + Sync {
    fn kind(&self) -> ProviderKind;

    fn validate_instance<'a>(
        &'a self,
        client: &'a ScopedHttpClient,
    ) -> BoxFuture<'a, Result<InstanceMetadata, AppError>>;

    fn authenticate_account<'a>(
        &'a self,
        client: &'a ScopedHttpClient,
        secret: &'a str,
    ) -> BoxFuture<'a, Result<AccountIdentity, AppError>>;

    fn list_repositories<'a>(
        &'a self,
        context: AdapterAccountContext<'a>,
        request: AdapterListRequest,
    ) -> BoxFuture<'a, Result<AdapterPage, AppError>>;

    fn get_repository<'a>(
        &'a self,
        context: AdapterAccountContext<'a>,
        identity: RemoteRepositoryIdentity,
    ) -> BoxFuture<'a, Result<RemoteRepository, AppError>>;

    fn detect_remote(
        &self,
        instance: &NormalizedInstance,
        remote: &NormalizedRemoteUrl,
    ) -> Option<RemoteRepositoryIdentity>;
}

#[derive(Clone)]
struct AdapterRegistration {
    plugin_id: String,
    adapter: Arc<dyn RepositoryDiscoveryProvider>,
}

#[derive(Clone)]
pub struct ProviderAdapterRegistry {
    database: Database,
    github: Option<AdapterRegistration>,
    gitlab: Option<AdapterRegistration>,
}

impl ProviderAdapterRegistry {
    pub fn from_plugins(database: Database, plugins: &PluginRegistry) -> Result<Self, AppError> {
        let mut registry = Self {
            database,
            github: None,
            gitlab: None,
        };
        for descriptor in plugins.descriptors() {
            if descriptor.manifest.contributions.providers.is_empty() {
                continue;
            }
            if descriptor.manifest.kind != PluginKind::Builtin {
                return Err(AppError::InvalidInput(
                    "Provider adapters must be built-in plugins".to_owned(),
                ));
            }
            for contribution in &descriptor.manifest.contributions.providers {
                if contribution.adapter_id != descriptor.manifest.id {
                    return Err(AppError::InvalidInput(
                        "Provider adapter ID must match its plugin".to_owned(),
                    ));
                }
                let (kind, expected_id, adapter): (
                    ProviderKind,
                    &str,
                    Arc<dyn RepositoryDiscoveryProvider>,
                ) = match contribution.provider_id {
                    ProviderContributionId::Github => (
                        ProviderKind::Github,
                        "git-ramus.provider.github",
                        Arc::new(GithubProvider),
                    ),
                    ProviderContributionId::Gitlab => (
                        ProviderKind::Gitlab,
                        "git-ramus.provider.gitlab",
                        Arc::new(GitlabProvider),
                    ),
                };
                if descriptor.manifest.id != expected_id {
                    return Err(AppError::InvalidInput(
                        "Provider contribution does not match a compiled adapter".to_owned(),
                    ));
                }
                registry.register(kind, expected_id.to_owned(), adapter)?;
            }
        }
        Ok(registry)
    }

    pub fn get(
        &self,
        kind: ProviderKind,
    ) -> Result<Arc<dyn RepositoryDiscoveryProvider>, AppError> {
        let registration = self.registration(kind).ok_or_else(provider_disabled)?;
        if !self.installation_enabled(&registration.plugin_id)? {
            return Err(provider_disabled());
        }
        Ok(Arc::clone(&registration.adapter))
    }

    pub fn is_enabled(&self, kind: ProviderKind) -> Result<bool, AppError> {
        let Some(registration) = self.registration(kind) else {
            return Ok(false);
        };
        self.installation_enabled(&registration.plugin_id)
    }

    #[cfg(all(feature = "e2e", debug_assertions))]
    pub(crate) fn replace_gitlab_for_e2e(&mut self, adapter: Arc<dyn RepositoryDiscoveryProvider>) {
        self.gitlab = Some(AdapterRegistration {
            plugin_id: "git-ramus.provider.gitlab".to_owned(),
            adapter,
        });
    }

    fn register(
        &mut self,
        kind: ProviderKind,
        plugin_id: String,
        adapter: Arc<dyn RepositoryDiscoveryProvider>,
    ) -> Result<(), AppError> {
        let slot = match kind {
            ProviderKind::Github => &mut self.github,
            ProviderKind::Gitlab => &mut self.gitlab,
        };
        if slot.is_some() {
            return Err(AppError::InvalidInput(
                "duplicate Provider adapter contribution".to_owned(),
            ));
        }
        *slot = Some(AdapterRegistration { plugin_id, adapter });
        Ok(())
    }

    fn registration(&self, kind: ProviderKind) -> Option<&AdapterRegistration> {
        match kind {
            ProviderKind::Github => self.github.as_ref(),
            ProviderKind::Gitlab => self.gitlab.as_ref(),
        }
    }

    fn installation_enabled(&self, plugin_id: &str) -> Result<bool, AppError> {
        self.database.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT enabled FROM plugin_installations WHERE plugin_id=?1 AND kind='builtin'",
                    [plugin_id],
                    |row| row.get(0),
                )
                .optional()
                .map(|enabled: Option<bool>| enabled.unwrap_or(false))
        })
    }

    #[cfg(any(test, debug_assertions))]
    #[doc(hidden)]
    pub fn for_test(
        database: Database,
        kind: ProviderKind,
        adapter: Arc<dyn RepositoryDiscoveryProvider>,
    ) -> Self {
        let mut registry = Self {
            database,
            github: None,
            gitlab: None,
        };
        let plugin_id = match kind {
            ProviderKind::Github => "git-ramus.provider.github",
            ProviderKind::Gitlab => "git-ramus.provider.gitlab",
        };
        registry
            .register(kind, plugin_id.to_owned(), adapter)
            .expect("test registry has one adapter");
        registry
    }
}

fn provider_disabled() -> AppError {
    AppError::UserActionRequired("Provider is disabled or unavailable".to_owned())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use chrono::Utc;
    use tempfile::tempdir;

    use super::ProviderAdapterRegistry;
    use crate::db::Database;
    use crate::plugins::PluginRegistry;
    use crate::providers::model::ProviderKind;

    fn seed_installation(database: &Database, plugin_id: &str) {
        database
            .with_connection(|connection| {
                connection.execute(
                    "INSERT INTO plugin_installations(plugin_id,version,kind,root_path,enabled,installed_at,updated_at) VALUES(?1,'0.1.0','builtin','/builtin',1,?2,?2)",
                    rusqlite::params![plugin_id, Utc::now().to_rfc3339()],
                )?;
                Ok(())
            })
            .expect("installation seeds");
    }

    #[test]
    fn registry_gates_compiled_adapters_through_the_current_enabled_bit() {
        let database = Database::open_in_memory().expect("database opens");
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("resources/plugins");
        let plugins = PluginRegistry::discover(&root).expect("built-ins discover");
        seed_installation(&database, "git-ramus.provider.github");
        seed_installation(&database, "git-ramus.provider.gitlab");
        let registry =
            ProviderAdapterRegistry::from_plugins(database.clone(), &plugins).expect("registry");

        assert_eq!(
            registry.get(ProviderKind::Github).unwrap().kind(),
            ProviderKind::Github
        );
        assert_eq!(
            registry.get(ProviderKind::Gitlab).unwrap().kind(),
            ProviderKind::Gitlab
        );
        database
            .with_connection(|connection| {
                connection.execute(
                    "UPDATE plugin_installations SET enabled=0 WHERE plugin_id='git-ramus.provider.gitlab'",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        assert!(!registry.is_enabled(ProviderKind::Gitlab).unwrap());
        assert!(registry.get(ProviderKind::Gitlab).is_err());
        assert!(registry.get(ProviderKind::Github).is_ok());
    }

    #[test]
    fn registry_rejects_a_builtin_manifest_that_impersonates_a_compiled_adapter() {
        let directory = tempdir().expect("temporary directory creates");
        let plugin = directory.path().join("git-ramus.provider.impostor");
        fs::create_dir(&plugin).expect("plugin directory creates");
        fs::write(
            plugin.join("plugin.json"),
            r#"{"schemaVersion":1,"id":"git-ramus.provider.impostor","name":"Impostor","version":"0.1.0","publisher":"test","description":"Impostor Provider","kind":"builtin","sdkVersion":"^0.1.0","entrypoints":{},"contributions":{"navigation":[],"providers":[{"providerId":"gitlab","adapterId":"git-ramus.provider.impostor","displayName":"GitLab","icon":"gitlab","instanceModes":["cloud"],"capabilities":["repositoryDiscovery"]}]},"permissions":[]}"#,
        )
        .expect("manifest writes");
        let plugins = PluginRegistry::discover(directory.path()).expect("manifest discovers");
        assert!(
            ProviderAdapterRegistry::from_plugins(Database::open_in_memory().unwrap(), &plugins)
                .is_err()
        );
    }
}
