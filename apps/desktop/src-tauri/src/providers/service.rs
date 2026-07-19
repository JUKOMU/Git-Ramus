use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{AppError, ProviderFailure};
use crate::providers::adapter::ProviderAdapterRegistry;
use crate::providers::cursor::{CursorStore, OperationRegistry};
use crate::providers::http::ScopedHttpClient;
use crate::providers::model::{
    AccountDeletionImpact, AccountDeletionResolution, NewProviderAccount, ProviderAccount,
    ProviderAccountSummary, ProviderConnectionStatus, ProviderInstance, ProviderInstanceSummary,
    ProviderKind,
};
use crate::providers::store::ProviderStore;
use crate::providers::url::normalize_instance_base;
use crate::secrets::{SecretStore, SensitiveString};

const ACCOUNT_IDLE_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateInstanceInput {
    pub provider_kind: ProviderKind,
    pub display_name: String,
    pub base_url: String,
    pub custom_ca_path: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "path", rename_all = "camelCase")]
pub enum CustomCaUpdate {
    Keep,
    Remove,
    Replace(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInstanceInput {
    pub instance_id: String,
    pub display_name: String,
    pub base_url: String,
    pub custom_ca: CustomCaUpdate,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DeleteAccountInput {
    pub account_id: String,
    pub resolution: AccountDeletionResolution,
    pub new_default_account_id: Option<String>,
}

#[derive(Default)]
struct ProviderHealth {
    instances: HashMap<String, ProviderConnectionStatus>,
    accounts: HashMap<String, ProviderConnectionStatus>,
}

pub struct ProviderService {
    store: ProviderStore,
    secrets: Arc<dyn SecretStore>,
    adapters: ProviderAdapterRegistry,
    _cursors: CursorStore,
    operations: OperationRegistry,
    health: Arc<Mutex<ProviderHealth>>,
}

impl ProviderService {
    pub fn new(
        store: ProviderStore,
        secrets: Arc<dyn SecretStore>,
        adapters: ProviderAdapterRegistry,
    ) -> Self {
        Self {
            store,
            secrets,
            adapters,
            _cursors: CursorStore::default(),
            operations: OperationRegistry::default(),
            health: Arc::new(Mutex::new(ProviderHealth::default())),
        }
    }

    pub async fn create_instance(
        &self,
        input: CreateInstanceInput,
    ) -> Result<ProviderInstanceSummary, AppError> {
        let display_name = validate_display_name(&input.display_name)?;
        let normalized = normalize_instance_base(&input.base_url, input.provider_kind)?;
        let custom_ca_path =
            canonical_custom_ca(input.provider_kind, input.custom_ca_path.as_deref())?;
        let now = Utc::now();
        let mut instance = ProviderInstance {
            id: Uuid::new_v4().to_string(),
            provider_kind: input.provider_kind,
            display_name,
            base_url: normalized.base_url,
            api_base_url: normalized.api_base_url,
            custom_ca_path,
            last_validated_at: None,
            server_version: None,
            created_at: now,
            updated_at: now,
        };
        let adapter = self.adapters.get(instance.provider_kind)?;
        let client = ScopedHttpClient::build(&instance)?;
        let metadata = adapter.validate_instance(&client).await?;
        instance.last_validated_at = Some(now);
        instance.server_version = metadata.server_version;
        let instance = self.store.insert_instance(instance)?;
        self.health
            .lock()
            .instances
            .insert(instance.id.clone(), ProviderConnectionStatus::Connected);
        self.instance_summary(instance)
    }

    pub async fn update_instance(
        &self,
        input: UpdateInstanceInput,
    ) -> Result<ProviderInstanceSummary, AppError> {
        let current = self.store.get_instance(&input.instance_id)?;
        let display_name = validate_display_name(&input.display_name)?;
        let normalized = normalize_instance_base(&input.base_url, current.provider_kind)?;
        let custom_ca_path = match input.custom_ca {
            CustomCaUpdate::Keep => current.custom_ca_path.clone(),
            CustomCaUpdate::Remove => None,
            CustomCaUpdate::Replace(path) => {
                canonical_custom_ca(current.provider_kind, Some(&path))?
            }
        };
        let now = Utc::now();
        let mut replacement = ProviderInstance {
            id: current.id,
            provider_kind: current.provider_kind,
            display_name,
            base_url: normalized.base_url,
            api_base_url: normalized.api_base_url,
            custom_ca_path,
            last_validated_at: current.last_validated_at,
            server_version: current.server_version,
            created_at: current.created_at,
            updated_at: now,
        };
        let adapter = self.adapters.get(replacement.provider_kind)?;
        let client = ScopedHttpClient::build(&replacement)?;
        let metadata = adapter
            .validate_instance(&client)
            .await
            .inspect_err(|error| self.record_instance_error(&replacement.id, error))?;
        replacement.last_validated_at = Some(now);
        replacement.server_version = metadata.server_version;
        let replacement = self.store.update_instance(replacement)?;
        self.health
            .lock()
            .instances
            .insert(replacement.id.clone(), ProviderConnectionStatus::Connected);
        self.instance_summary(replacement)
    }

    pub async fn validate_instance(
        &self,
        instance_id: &str,
    ) -> Result<ProviderInstanceSummary, AppError> {
        let mut instance = self.store.get_instance(instance_id)?;
        let adapter = self.adapters.get(instance.provider_kind)?;
        let client = ScopedHttpClient::build(&instance)?;
        let metadata = adapter
            .validate_instance(&client)
            .await
            .inspect_err(|error| self.record_instance_error(instance_id, error))?;
        let now = Utc::now();
        instance.last_validated_at = Some(now);
        instance.updated_at = now;
        instance.server_version = metadata.server_version;
        let instance = self.store.update_instance(instance)?;
        self.health
            .lock()
            .instances
            .insert(instance.id.clone(), ProviderConnectionStatus::Connected);
        self.instance_summary(instance)
    }

    pub fn list_instances(&self) -> Result<Vec<ProviderInstanceSummary>, AppError> {
        self.store
            .list_instances()?
            .into_iter()
            .map(|instance| self.instance_summary(instance))
            .collect()
    }

    pub fn delete_instance(&self, instance_id: &str) -> Result<(), AppError> {
        self.store.delete_instance(instance_id)?;
        self.health.lock().instances.remove(instance_id);
        Ok(())
    }

    pub fn list_accounts(
        &self,
        instance_id: &str,
    ) -> Result<Vec<ProviderAccountSummary>, AppError> {
        let instance = self.store.get_instance(instance_id)?;
        let enabled = self.adapters.is_enabled(instance.provider_kind)?;
        Ok(self
            .store
            .list_accounts(instance_id)?
            .into_iter()
            .map(|account| self.account_summary(account, enabled))
            .collect())
    }

    pub async fn connect_account(
        &self,
        instance_id: &str,
        pat: SensitiveString,
    ) -> Result<ProviderAccountSummary, AppError> {
        let instance = self.store.get_instance(instance_id)?;
        let adapter = self.adapters.get(instance.provider_kind)?;
        let client = ScopedHttpClient::build(&instance)?;
        let account_id = Uuid::new_v4().to_string();
        let secret_ref = new_secret_ref(&account_id);
        self.secrets.set(&secret_ref, pat.as_str())?;
        let identity = match adapter.authenticate_account(&client, pat.as_str()).await {
            Ok(identity) => identity,
            Err(error) => {
                self.compensate_new_secret(&secret_ref);
                return Err(error);
            }
        };
        let now = Utc::now();
        let account = NewProviderAccount {
            id: account_id,
            instance_id: instance.id,
            provider_user_id: identity.provider_user_id,
            username: identity.username,
            display_name: identity.display_name,
            avatar_url: identity.avatar_url,
            secret_ref: secret_ref.clone(),
            last_validated_at: now,
            created_at: now,
            updated_at: now,
        };
        let account = match self.store.insert_account(account) {
            Ok(account) => account,
            Err(error) => {
                self.compensate_new_secret(&secret_ref);
                return Err(error);
            }
        };
        self.health
            .lock()
            .accounts
            .insert(account.id.clone(), ProviderConnectionStatus::Connected);
        Ok(account.summary(ProviderConnectionStatus::Connected))
    }

    pub async fn rotate_account(
        &self,
        account_id: &str,
        pat: SensitiveString,
    ) -> Result<ProviderAccountSummary, AppError> {
        let account = self.store.get_account(account_id)?;
        let instance = self.store.get_instance(&account.instance_id)?;
        let adapter = self.adapters.get(instance.provider_kind)?;
        let client = ScopedHttpClient::build(&instance)?;
        let new_ref = new_secret_ref(account_id);
        self.secrets.set(&new_ref, pat.as_str())?;
        let identity = match adapter.authenticate_account(&client, pat.as_str()).await {
            Ok(identity) => identity,
            Err(error) => {
                self.compensate_new_secret(&new_ref);
                self.record_account_error(account_id, &error);
                return Err(error);
            }
        };
        if identity.provider_user_id != account.provider_user_id {
            self.compensate_new_secret(&new_ref);
            return Err(AppError::InvalidInput(
                "replacement token belongs to another Provider user".to_owned(),
            ));
        }
        let now = Utc::now().to_rfc3339();
        if let Err(error) = self.store.update_account_secret(account_id, &new_ref, &now) {
            self.compensate_new_secret(&new_ref);
            return Err(error);
        }
        if self.secrets.delete(&account.secret_ref).is_err() {
            self.store.enqueue_secret_cleanup(&account.secret_ref)?;
        }
        self.health
            .lock()
            .accounts
            .insert(account_id.to_owned(), ProviderConnectionStatus::Connected);
        Ok(self
            .store
            .get_account(account_id)?
            .summary(ProviderConnectionStatus::Connected))
    }

    pub async fn validate_account(
        &self,
        account_id: &str,
    ) -> Result<ProviderAccountSummary, AppError> {
        let account = self.store.get_account(account_id)?;
        let instance = self.store.get_instance(&account.instance_id)?;
        let adapter = self.adapters.get(instance.provider_kind)?;
        let client = ScopedHttpClient::build(&instance)?;
        let value = self
            .secrets
            .get(&account.secret_ref)?
            .ok_or(AppError::SecretStore)?;
        let secret = SensitiveString::new(value);
        let identity = adapter
            .authenticate_account(&client, secret.as_str())
            .await
            .inspect_err(|error| self.record_account_error(account_id, error))?;
        if identity.provider_user_id != account.provider_user_id {
            let error = AppError::Provider(ProviderFailure::authentication());
            self.record_account_error(account_id, &error);
            return Err(error);
        }
        self.store.update_account_secret(
            account_id,
            &account.secret_ref,
            &Utc::now().to_rfc3339(),
        )?;
        self.health
            .lock()
            .accounts
            .insert(account_id.to_owned(), ProviderConnectionStatus::Connected);
        Ok(self
            .store
            .get_account(account_id)?
            .summary(ProviderConnectionStatus::Connected))
    }

    pub fn set_default_account(
        &self,
        instance_id: &str,
        account_id: &str,
    ) -> Result<ProviderAccountSummary, AppError> {
        self.store.set_default_account(instance_id, account_id)?;
        let account = self.store.get_account(account_id)?;
        let instance = self.store.get_instance(instance_id)?;
        let enabled = self.adapters.is_enabled(instance.provider_kind)?;
        Ok(self.account_summary(account, enabled))
    }

    pub fn account_deletion_impact(
        &self,
        account_id: &str,
    ) -> Result<AccountDeletionImpact, AppError> {
        self.store.account_deletion_impact(account_id)
    }

    pub async fn delete_account(&self, input: DeleteAccountInput) -> Result<(), AppError> {
        self.operations.cancel_for_account(&input.account_id);
        self.operations
            .wait_for_account_idle(&input.account_id, ACCOUNT_IDLE_TIMEOUT)
            .await?;
        let account = self.store.get_account(&input.account_id)?;
        let secret = SensitiveString::new(
            self.secrets
                .get(&account.secret_ref)?
                .ok_or(AppError::SecretStore)?,
        );
        self.secrets.delete(&account.secret_ref)?;
        if let Err(error) = self.store.delete_account_with_resolution(
            &input.account_id,
            &input.resolution,
            input.new_default_account_id.as_deref(),
        ) {
            self.secrets.set(&account.secret_ref, secret.as_str())?;
            return Err(error);
        }
        self.health.lock().accounts.remove(&input.account_id);
        Ok(())
    }

    pub async fn cancel_plugin_account_operations(
        &self,
        plugin_id: &str,
        account_id: &str,
    ) -> Result<(), AppError> {
        self.operations
            .cancel_for_plugin_account(plugin_id, account_id);
        self.operations
            .wait_for_plugin_account_idle(plugin_id, account_id, ACCOUNT_IDLE_TIMEOUT)
            .await
    }

    pub fn retry_secret_cleanup(&self) -> Result<(), AppError> {
        for record in self.store.list_secret_cleanup()? {
            if self.store.secret_ref_is_referenced(&record.secret_ref)? {
                continue;
            }
            match self.secrets.delete(&record.secret_ref) {
                Ok(()) => self
                    .store
                    .record_cleanup_attempt(&record.secret_ref, true, None)?,
                Err(_) => self.store.record_cleanup_attempt(
                    &record.secret_ref,
                    false,
                    Some("secrets.unavailable"),
                )?,
            }
        }
        Ok(())
    }

    fn compensate_new_secret(&self, secret_ref: &str) {
        if self.secrets.delete(secret_ref).is_err() {
            let _ = self.store.enqueue_secret_cleanup(secret_ref);
        }
    }

    fn instance_summary(
        &self,
        instance: ProviderInstance,
    ) -> Result<ProviderInstanceSummary, AppError> {
        let enabled = self.adapters.is_enabled(instance.provider_kind)?;
        let status = if enabled {
            self.health
                .lock()
                .instances
                .get(&instance.id)
                .copied()
                .unwrap_or(if instance.last_validated_at.is_some() {
                    ProviderConnectionStatus::Connected
                } else {
                    ProviderConnectionStatus::Unavailable
                })
        } else {
            ProviderConnectionStatus::Unavailable
        };
        Ok(instance.summary(enabled, status))
    }

    fn account_summary(
        &self,
        account: ProviderAccount,
        provider_enabled: bool,
    ) -> ProviderAccountSummary {
        let status = if provider_enabled {
            self.health
                .lock()
                .accounts
                .get(&account.id)
                .copied()
                .unwrap_or(ProviderConnectionStatus::Connected)
        } else {
            ProviderConnectionStatus::Unavailable
        };
        account.summary(status)
    }

    fn record_instance_error(&self, instance_id: &str, error: &AppError) {
        self.health
            .lock()
            .instances
            .insert(instance_id.to_owned(), status_for_error(error));
    }

    fn record_account_error(&self, account_id: &str, error: &AppError) {
        self.health
            .lock()
            .accounts
            .insert(account_id.to_owned(), status_for_error(error));
    }
}

fn validate_display_name(value: &str) -> Result<String, AppError> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 128 || value.chars().any(char::is_control) {
        return Err(AppError::InvalidInput(
            "Provider display name is invalid".to_owned(),
        ));
    }
    Ok(value.to_owned())
}

fn canonical_custom_ca(
    kind: ProviderKind,
    value: Option<&str>,
) -> Result<Option<String>, AppError> {
    let Some(value) = value else {
        return Ok(None);
    };
    if kind == ProviderKind::Github {
        return Err(AppError::InvalidInput(
            "GitHub.com does not support a custom CA".to_owned(),
        ));
    }
    let canonical = std::fs::canonicalize(Path::new(value))?;
    let canonical = canonical.to_str().ok_or(AppError::NonUtf8Path)?.to_owned();
    Ok(Some(canonical))
}

fn new_secret_ref(account_id: &str) -> String {
    format!("provider/account/{account_id}/{}", Uuid::new_v4())
}

fn status_for_error(error: &AppError) -> ProviderConnectionStatus {
    match error {
        AppError::Provider(failure) => match failure.code() {
            "provider.authentication-required" | "provider.permission-insufficient" => {
                ProviderConnectionStatus::ActionRequired
            }
            "provider.rate-limited" => ProviderConnectionStatus::RateLimited,
            _ => ProviderConnectionStatus::Unavailable,
        },
        AppError::UserActionRequired(_) | AppError::SecretStore => {
            ProviderConnectionStatus::ActionRequired
        }
        _ => ProviderConnectionStatus::Unavailable,
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use chrono::Utc;
    use futures_util::future::BoxFuture;
    use parking_lot::Mutex;
    use tempfile::tempdir;

    use super::{CreateInstanceInput, DeleteAccountInput, ProviderService};
    use crate::db::Database;
    use crate::error::AppError;
    use crate::providers::adapter::{
        AdapterAccountContext, ProviderAdapterRegistry, RepositoryDiscoveryProvider,
    };
    use crate::providers::http::ScopedHttpClient;
    use crate::providers::model::{
        AccountDeletionResolution, AccountIdentity, AdapterListRequest, AdapterPage,
        InstanceMetadata, ProviderKind, RemoteRepository, RemoteRepositoryIdentity,
    };
    use crate::providers::store::ProviderStore;
    use crate::providers::url::{NormalizedInstance, NormalizedRemoteUrl};
    use crate::secrets::{SecretStore, SensitiveString};

    struct FakeProvider {
        kind: ProviderKind,
        identity: Mutex<AccountIdentity>,
    }

    impl FakeProvider {
        fn new(kind: ProviderKind) -> Self {
            Self {
                kind,
                identity: Mutex::new(AccountIdentity {
                    provider_user_id: "provider-user-1".to_owned(),
                    username: "tempest".to_owned(),
                    display_name: Some("Yozora Tempest".to_owned()),
                    avatar_url: None,
                }),
            }
        }

        fn set_provider_user(&self, provider_user_id: &str) {
            self.identity.lock().provider_user_id = provider_user_id.to_owned();
        }
    }

    impl RepositoryDiscoveryProvider for FakeProvider {
        fn kind(&self) -> ProviderKind {
            self.kind
        }

        fn validate_instance<'a>(
            &'a self,
            _client: &'a ScopedHttpClient,
        ) -> BoxFuture<'a, Result<InstanceMetadata, AppError>> {
            Box::pin(async {
                Ok(InstanceMetadata {
                    server_version: Some("18.2.0-test".to_owned()),
                })
            })
        }

        fn authenticate_account<'a>(
            &'a self,
            _client: &'a ScopedHttpClient,
            _secret: &'a str,
        ) -> BoxFuture<'a, Result<AccountIdentity, AppError>> {
            Box::pin(async move { Ok(self.identity.lock().clone()) })
        }

        fn list_repositories<'a>(
            &'a self,
            _context: AdapterAccountContext<'a>,
            _request: AdapterListRequest,
        ) -> BoxFuture<'a, Result<AdapterPage, AppError>> {
            Box::pin(async {
                Ok(AdapterPage {
                    items: Vec::new(),
                    next_cursor: None,
                    rate_limit: None,
                })
            })
        }

        fn get_repository<'a>(
            &'a self,
            _context: AdapterAccountContext<'a>,
            _identity: RemoteRepositoryIdentity,
        ) -> BoxFuture<'a, Result<RemoteRepository, AppError>> {
            Box::pin(async { Err(AppError::NotFound("fake repository".to_owned())) })
        }

        fn detect_remote(
            &self,
            _instance: &NormalizedInstance,
            _remote: &NormalizedRemoteUrl,
        ) -> Option<RemoteRepositoryIdentity> {
            None
        }
    }

    #[derive(Default)]
    struct ScriptedSecretStore {
        values: Mutex<HashMap<String, String>>,
        gets: Mutex<Vec<String>>,
        fail_delete: AtomicBool,
    }

    impl ScriptedSecretStore {
        fn values(&self) -> HashMap<String, String> {
            self.values.lock().clone()
        }

        fn fail_deletes(&self, value: bool) {
            self.fail_delete.store(value, Ordering::SeqCst);
        }

        fn get_keys(&self) -> Vec<String> {
            self.gets.lock().clone()
        }
    }

    impl SecretStore for ScriptedSecretStore {
        fn set(&self, key: &str, secret: &str) -> Result<(), AppError> {
            self.values.lock().insert(key.to_owned(), secret.to_owned());
            Ok(())
        }

        fn get(&self, key: &str) -> Result<Option<String>, AppError> {
            self.gets.lock().push(key.to_owned());
            Ok(self.values.lock().get(key).cloned())
        }

        fn delete(&self, key: &str) -> Result<(), AppError> {
            if self.fail_delete.load(Ordering::SeqCst) {
                return Err(AppError::SecretStore);
            }
            self.values.lock().remove(key);
            Ok(())
        }
    }

    struct Fixture {
        database: Database,
        store: ProviderStore,
        service: ProviderService,
        secrets: Arc<ScriptedSecretStore>,
        adapter: Arc<FakeProvider>,
    }

    impl Fixture {
        fn new() -> Self {
            let database = Database::open_in_memory().expect("database opens");
            database
                .with_connection(|connection| {
                    connection.execute(
                        "INSERT INTO plugin_installations(plugin_id,version,kind,root_path,enabled,installed_at,updated_at) VALUES('git-ramus.provider.gitlab','0.1.0','builtin','/builtin/gitlab',1,?1,?1)",
                        [Utc::now().to_rfc3339()],
                    )?;
                    Ok(())
                })
                .expect("Provider installation seeds");
            let store = ProviderStore::new(database.clone());
            let secrets = Arc::new(ScriptedSecretStore::default());
            let adapter = Arc::new(FakeProvider::new(ProviderKind::Gitlab));
            let adapters = ProviderAdapterRegistry::for_test(
                database.clone(),
                ProviderKind::Gitlab,
                adapter.clone(),
            );
            let service = ProviderService::new(store.clone(), secrets.clone(), adapters);
            Self {
                database,
                store,
                service,
                secrets,
                adapter,
            }
        }

        async fn create_instance(&self) -> crate::providers::model::ProviderInstanceSummary {
            self.service
                .create_instance(CreateInstanceInput {
                    provider_kind: ProviderKind::Gitlab,
                    display_name: "Private GitLab".to_owned(),
                    base_url: "https://gitlab.example".to_owned(),
                    custom_ca_path: None,
                })
                .await
                .expect("instance creates")
        }
    }

    #[tokio::test]
    async fn account_connect_uses_a_random_secret_ref_and_sets_the_first_default() {
        let fixture = Fixture::new();
        let instance = fixture.create_instance().await;
        let account = fixture
            .service
            .connect_account(&instance.id, SensitiveString::new("token-a".to_owned()))
            .await
            .expect("account connects");

        assert!(account.is_default);
        let secrets = fixture.secrets.values();
        assert_eq!(secrets.len(), 1);
        let (secret_ref, value) = secrets.iter().next().expect("secret exists");
        assert_eq!(value, "token-a");
        assert!(secret_ref.starts_with(&format!("provider/account/{}/", account.id)));
        assert!(!secret_ref.contains(&account.username));
        assert!(
            !fixture
                .store
                .get_account(&account.id)
                .unwrap()
                .secret_ref
                .contains("token-a")
        );
    }

    #[tokio::test]
    async fn instance_github_uses_fixed_urls_and_rejects_custom_ca_configuration() {
        let database = Database::open_in_memory().unwrap();
        database
            .with_connection(|connection| {
                connection.execute(
                    "INSERT INTO plugin_installations(plugin_id,version,kind,root_path,enabled,installed_at,updated_at) VALUES('git-ramus.provider.github','0.1.0','builtin','/builtin/github',1,?1,?1)",
                    [Utc::now().to_rfc3339()],
                )?;
                Ok(())
            })
            .unwrap();
        let adapter = Arc::new(FakeProvider::new(ProviderKind::Github));
        let registry =
            ProviderAdapterRegistry::for_test(database.clone(), ProviderKind::Github, adapter);
        let service = ProviderService::new(
            ProviderStore::new(database),
            Arc::new(ScriptedSecretStore::default()),
            registry,
        );
        let instance = service
            .create_instance(CreateInstanceInput {
                provider_kind: ProviderKind::Github,
                display_name: "GitHub".to_owned(),
                base_url: "https://github.com/".to_owned(),
                custom_ca_path: None,
            })
            .await
            .unwrap();
        assert_eq!(instance.base_url, "https://github.com");
        assert!(
            service
                .create_instance(CreateInstanceInput {
                    provider_kind: ProviderKind::Github,
                    display_name: "GitHub with CA".to_owned(),
                    base_url: "https://github.com".to_owned(),
                    custom_ca_path: Some("unused.pem".to_owned()),
                })
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn instance_custom_ca_summary_exposes_only_the_canonical_file_name() {
        let fixture = Fixture::new();
        let directory = tempdir().unwrap();
        let ca_path = directory.path().join("company-root.pem");
        let rcgen::CertifiedKey { cert, .. } =
            rcgen::generate_simple_self_signed(vec!["gitlab.example".to_owned()]).unwrap();
        std::fs::write(&ca_path, cert.pem()).unwrap();

        let instance = fixture
            .service
            .create_instance(CreateInstanceInput {
                provider_kind: ProviderKind::Gitlab,
                display_name: "Private GitLab".to_owned(),
                base_url: "https://gitlab.example".to_owned(),
                custom_ca_path: Some(ca_path.to_string_lossy().into_owned()),
            })
            .await
            .unwrap();

        assert!(instance.custom_ca_configured);
        assert_eq!(
            instance.custom_ca_label.as_deref(),
            Some("company-root.pem")
        );
        let serialized = serde_json::to_string(&instance).unwrap();
        assert!(!serialized.contains(&directory.path().to_string_lossy().into_owned()));
    }

    #[tokio::test]
    async fn account_second_profile_reuses_the_instance_without_replacing_the_default() {
        let fixture = Fixture::new();
        let instance = fixture.create_instance().await;
        let first = fixture
            .service
            .connect_account(&instance.id, SensitiveString::new("token-a".to_owned()))
            .await
            .expect("first account connects");
        fixture.adapter.set_provider_user("provider-user-2");
        let second = fixture
            .service
            .connect_account(&instance.id, SensitiveString::new("token-b".to_owned()))
            .await
            .expect("second account connects");

        assert!(first.is_default);
        assert!(!second.is_default);
        let selected = fixture
            .service
            .set_default_account(&instance.id, &second.id)
            .expect("default switches");
        assert!(selected.is_default);
        assert!(!fixture.store.get_account(&first.id).unwrap().is_default);
    }

    #[tokio::test]
    async fn account_failed_insert_deletes_or_queues_the_new_secret() {
        let fixture = Fixture::new();
        let instance = fixture.create_instance().await;
        fixture
            .service
            .connect_account(&instance.id, SensitiveString::new("token-a".to_owned()))
            .await
            .expect("first account connects");
        fixture.secrets.fail_deletes(true);

        let error = fixture
            .service
            .connect_account(&instance.id, SensitiveString::new("token-b".to_owned()))
            .await
            .expect_err("duplicate Provider identity fails");

        assert!(matches!(error, AppError::InvalidInput(_)));
        assert_eq!(fixture.store.list_secret_cleanup().unwrap().len(), 1);
        assert_eq!(fixture.store.list_accounts(&instance.id).unwrap().len(), 1);
    }

    #[tokio::test]
    async fn account_rotation_rejects_a_token_for_another_provider_user() {
        let fixture = Fixture::new();
        let instance = fixture.create_instance().await;
        let account = fixture
            .service
            .connect_account(
                &instance.id,
                SensitiveString::new("original-token".to_owned()),
            )
            .await
            .expect("account connects");
        fixture.adapter.set_provider_user("different-provider-user");

        assert!(
            fixture
                .service
                .rotate_account(&account.id, SensitiveString::new("other-token".to_owned()))
                .await
                .is_err()
        );
        let stored = fixture.store.get_account(&account.id).unwrap();
        assert_eq!(
            fixture.secrets.get(&stored.secret_ref).unwrap().as_deref(),
            Some("original-token")
        );
    }

    #[tokio::test]
    async fn account_disabled_provider_preserves_rows_and_resumes_after_reenable() {
        let fixture = Fixture::new();
        let instance = fixture.create_instance().await;
        let account = fixture
            .service
            .connect_account(&instance.id, SensitiveString::new("token-a".to_owned()))
            .await
            .expect("account connects");
        fixture
            .database
            .with_connection(|connection| {
                connection.execute(
                    "UPDATE plugin_installations SET enabled=0 WHERE plugin_id='git-ramus.provider.gitlab'",
                    [],
                )?;
                Ok(())
            })
            .unwrap();

        assert!(fixture.service.validate_account(&account.id).await.is_err());
        assert_eq!(fixture.store.list_accounts(&instance.id).unwrap().len(), 1);
        assert!(!fixture.service.list_instances().unwrap()[0].provider_enabled);

        fixture
            .database
            .with_connection(|connection| {
                connection.execute(
                    "UPDATE plugin_installations SET enabled=1 WHERE plugin_id='git-ramus.provider.gitlab'",
                    [],
                )?;
                Ok(())
            })
            .unwrap();
        assert!(fixture.service.validate_account(&account.id).await.is_ok());
        assert_eq!(fixture.store.list_accounts(&instance.id).unwrap().len(), 1);
    }

    #[tokio::test]
    async fn account_delete_failure_keeps_or_restores_the_secret_and_database_row() {
        let fixture = Fixture::new();
        let instance = fixture.create_instance().await;
        let account = fixture
            .service
            .connect_account(&instance.id, SensitiveString::new("token-a".to_owned()))
            .await
            .expect("account connects");
        fixture.secrets.fail_deletes(true);
        let input = DeleteAccountInput {
            account_id: account.id.clone(),
            resolution: AccountDeletionResolution::Unbind,
            new_default_account_id: None,
        };
        assert!(fixture.service.delete_account(input).await.is_err());
        assert!(fixture.store.get_account(&account.id).is_ok());

        fixture.secrets.fail_deletes(false);
        let invalid_resolution = DeleteAccountInput {
            account_id: account.id.clone(),
            resolution: AccountDeletionResolution::Reassign {
                account_id: "not-a-sibling".to_owned(),
            },
            new_default_account_id: None,
        };
        assert!(
            fixture
                .service
                .delete_account(invalid_resolution)
                .await
                .is_err()
        );
        let stored = fixture.store.get_account(&account.id).unwrap();
        assert_eq!(
            fixture.secrets.get(&stored.secret_ref).unwrap().as_deref(),
            Some("token-a")
        );
    }

    #[tokio::test]
    async fn account_startup_cleanup_skips_referenced_secrets_and_removes_orphans() {
        let fixture = Fixture::new();
        let instance = fixture.create_instance().await;
        let account = fixture
            .service
            .connect_account(&instance.id, SensitiveString::new("token-a".to_owned()))
            .await
            .expect("account connects");
        let referenced = fixture.store.get_account(&account.id).unwrap().secret_ref;
        fixture.store.enqueue_secret_cleanup(&referenced).unwrap();
        fixture
            .secrets
            .set("provider/account/orphan", "orphan")
            .unwrap();
        fixture
            .store
            .enqueue_secret_cleanup("provider/account/orphan")
            .unwrap();

        fixture.service.retry_secret_cleanup().unwrap();

        assert!(fixture.secrets.get(&referenced).unwrap().is_some());
        assert!(
            fixture
                .secrets
                .get("provider/account/orphan")
                .unwrap()
                .is_none()
        );
        assert_eq!(fixture.store.list_secret_cleanup().unwrap().len(), 1);
    }

    #[tokio::test]
    async fn account_file_restart_reloads_profiles_and_validates_through_secret_refs() {
        let directory = tempdir().expect("temporary directory creates");
        let database_path = directory.path().join("provider.db");
        let secrets = Arc::new(ScriptedSecretStore::default());
        let adapter = Arc::new(FakeProvider::new(ProviderKind::Gitlab));
        let (instance_id, first_account_id);
        {
            let database = Database::open(&database_path).expect("file database opens");
            database
                .with_connection(|connection| {
                    connection.execute(
                        "INSERT INTO plugin_installations(plugin_id,version,kind,root_path,enabled,installed_at,updated_at) VALUES('git-ramus.provider.gitlab','0.1.0','builtin','/builtin/gitlab',1,?1,?1)",
                        [Utc::now().to_rfc3339()],
                    )?;
                    Ok(())
                })
                .unwrap();
            let registry = ProviderAdapterRegistry::for_test(
                database.clone(),
                ProviderKind::Gitlab,
                adapter.clone(),
            );
            let service =
                ProviderService::new(ProviderStore::new(database), secrets.clone(), registry);
            let instance = service
                .create_instance(CreateInstanceInput {
                    provider_kind: ProviderKind::Gitlab,
                    display_name: "Restart GitLab".to_owned(),
                    base_url: "https://gitlab.example".to_owned(),
                    custom_ca_path: None,
                })
                .await
                .unwrap();
            let first = service
                .connect_account(
                    &instance.id,
                    SensitiveString::new("persisted-token-a".to_owned()),
                )
                .await
                .unwrap();
            adapter.set_provider_user("provider-user-2");
            service
                .connect_account(
                    &instance.id,
                    SensitiveString::new("persisted-token-b".to_owned()),
                )
                .await
                .unwrap();
            instance_id = instance.id;
            first_account_id = first.id;
        }

        adapter.set_provider_user("provider-user-1");
        let database = Database::open(&database_path).expect("file database reopens");
        let registry =
            ProviderAdapterRegistry::for_test(database.clone(), ProviderKind::Gitlab, adapter);
        let store = ProviderStore::new(database);
        let expected_secret_ref = store.get_account(&first_account_id).unwrap().secret_ref;
        let service = ProviderService::new(store, secrets.clone(), registry);
        assert_eq!(service.list_instances().unwrap().len(), 1);
        assert_eq!(service.list_accounts(&instance_id).unwrap().len(), 2);
        assert!(service.validate_account(&first_account_id).await.is_ok());
        assert_eq!(secrets.get_keys().last(), Some(&expected_secret_ref));
        assert!(
            secrets
                .values()
                .values()
                .any(|value| value == "persisted-token-a")
        );
    }
}
