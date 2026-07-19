use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::Utc;
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use tokio::sync::Semaphore;
use uuid::Uuid;

use crate::error::{AppError, ProviderFailure};
use crate::git::model::Remote;
use crate::git::repository::RepositoryRepository;
use crate::providers::adapter::{
    AdapterAccountContext, ProviderAdapterRegistry, RepositoryDiscoveryProvider,
};
use crate::providers::cursor::{CursorEntry, CursorStore, OperationRegistry};
use crate::providers::http::ScopedHttpClient;
use crate::providers::model::{
    AccountDeletionImpact, AccountDeletionResolution, BindingSource, NewProviderAccount,
    ProviderAccount, ProviderAccountSummary, ProviderArchivedFilter, ProviderBinding,
    ProviderBindingSuggestion, ProviderBindingSuggestionStatus, ProviderConnectionStatus,
    ProviderInstance, ProviderInstanceSummary, ProviderKind, ProviderRateLimitState,
    ProviderRepositoryPage, ProviderRepositoryQuery, RemoteRepository, RemoteRepositoryIdentity,
};
use crate::providers::store::ProviderStore;
use crate::providers::url::{
    NormalizedInstance, normalize_instance_base, normalize_remote_url, sanitized_remote_url,
};
use crate::secrets::{SecretStore, SensitiveString};

const ACCOUNT_IDLE_TIMEOUT: Duration = Duration::from_secs(5);
const ACCOUNT_DISCOVERY_CONCURRENCY: usize = 4;
const CURSOR_TTL: Duration = Duration::from_secs(10 * 60);
const MAX_ADAPTER_PAGE_ITEMS: usize = 100;
const MAX_UPSTREAM_PAGES_PER_REQUEST: usize = 100;

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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ListRepositoriesInput {
    pub account_id: String,
    pub query: ProviderRepositoryQuery,
    pub cursor: Option<String>,
    pub operation_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BindRemoteInput {
    pub repository_id: String,
    pub remote_name: String,
    pub instance_id: String,
    pub account_id: Option<String>,
    pub provider_repository_id: String,
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
    cursors: CursorStore,
    operations: OperationRegistry,
    account_semaphores: Arc<Mutex<HashMap<String, Arc<Semaphore>>>>,
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
            cursors: CursorStore::default(),
            operations: OperationRegistry::default(),
            account_semaphores: Arc::new(Mutex::new(HashMap::new())),
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
        self.account_semaphores.lock().remove(&input.account_id);
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

    pub async fn list_repositories(
        &self,
        plugin_id: &str,
        input: ListRepositoriesInput,
    ) -> Result<ProviderRepositoryPage, AppError> {
        let operation_id = input.operation_id.clone();
        self.list_repositories_inner(plugin_id, input)
            .await
            .map_err(|error| with_request_context(error, plugin_id, &operation_id))
    }

    async fn list_repositories_inner(
        &self,
        plugin_id: &str,
        input: ListRepositoriesInput,
    ) -> Result<ProviderRepositoryPage, AppError> {
        validate_repository_query(&input.query)?;
        let operation = self
            .operations
            .start(plugin_id, &input.account_id, &input.operation_id)?;
        let cancellation = operation.token().clone();
        let account = self.store.get_account(&input.account_id)?;
        let instance = self.store.get_instance(&account.instance_id)?;
        let adapter = self.adapters.get(instance.provider_kind)?;
        let continuation = input.cursor.is_some();
        let mut adapter_cursor = None;
        let mut buffered = Vec::new();
        if let Some(cursor) = input.cursor.as_deref() {
            let entry = self.cursors.take(
                cursor,
                plugin_id,
                instance.provider_kind,
                &instance.id,
                &account.id,
                &input.query,
            )?;
            adapter_cursor = entry.adapter_cursor;
            buffered = entry.buffered;
        }

        let semaphore = self.account_semaphore(&account.id);
        let permit = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(canceled()),
            permit = semaphore.acquire_owned() => {
                permit.map_err(|_| AppError::Provider(ProviderFailure::busy(Some(250))))?
            },
        };
        let value = self
            .secrets
            .get(&account.secret_ref)?
            .ok_or(AppError::SecretStore)?;
        let secret = SensitiveString::new(value);
        let client = ScopedHttpClient::build(&instance)?;
        let mut items = Vec::with_capacity(input.query.page_size);
        take_buffered(&mut buffered, &mut items, input.query.page_size);
        let mut rate_limit = None;
        let mut pages_read = 0;
        let mut should_fetch =
            !continuation || (items.len() < input.query.page_size && adapter_cursor.is_some());
        let mut seen_cursors = Vec::new();

        while items.len() < input.query.page_size && should_fetch {
            if pages_read >= MAX_UPSTREAM_PAGES_PER_REQUEST {
                return Err(AppError::Provider(
                    ProviderFailure::partial()
                        .with_failed_step("provider.listRepositories.pageLimit"),
                ));
            }
            if let Some(cursor) = adapter_cursor.as_ref() {
                if seen_cursors.contains(cursor) {
                    return Err(AppError::Provider(ProviderFailure::invalid_response()));
                }
                seen_cursors.push(cursor.clone());
            }
            let request = crate::providers::model::AdapterListRequest {
                query: input.query.clone(),
                cursor: adapter_cursor.take(),
            };
            let page = match adapter
                .list_repositories(
                    AdapterAccountContext {
                        client: &client,
                        secret: secret.as_str(),
                        cancellation: &cancellation,
                    },
                    request,
                )
                .await
            {
                Ok(page) => page,
                Err(error) => {
                    self.record_account_error(&account.id, &error);
                    if continuation && !is_canceled(&error) {
                        return Err(AppError::Provider(
                            ProviderFailure::partial()
                                .with_failed_step("provider.listRepositories.page"),
                        ));
                    }
                    return Err(error);
                }
            };
            if cancellation.is_cancelled() {
                return Err(canceled());
            }
            pages_read += 1;
            if page.items.len() > MAX_ADAPTER_PAGE_ITEMS
                || page.items.iter().any(|item| {
                    item.provider_kind != instance.provider_kind || item.instance_id != instance.id
                })
            {
                return Err(AppError::Provider(ProviderFailure::invalid_response()));
            }
            let mut matching = page
                .items
                .into_iter()
                .filter(|item| repository_matches(item, &input.query))
                .collect::<Vec<_>>();
            adapter_cursor = page.next_cursor;
            if let Some(state) = page.rate_limit {
                self.update_rate_health(&account.id, &state);
                rate_limit = Some(state);
            }
            let remaining = input.query.page_size - items.len();
            if matching.len() > remaining {
                buffered.extend(matching.drain(remaining..));
            }
            items.extend(matching);
            should_fetch = adapter_cursor.is_some();
        }
        if cancellation.is_cancelled() {
            return Err(canceled());
        }
        drop(permit);
        let has_more = !buffered.is_empty() || adapter_cursor.is_some();
        let next_cursor = has_more.then(|| {
            self.cursors.insert(CursorEntry {
                plugin_id: plugin_id.to_owned(),
                provider_kind: instance.provider_kind,
                instance_id: instance.id.clone(),
                account_id: account.id.clone(),
                query: input.query,
                adapter_cursor,
                buffered,
                expires_at: Instant::now() + CURSOR_TTL,
            })
        });
        self.health
            .lock()
            .accounts
            .entry(account.id)
            .or_insert(ProviderConnectionStatus::Connected);
        Ok(ProviderRepositoryPage {
            items,
            next_cursor,
            has_more,
            rate_limit,
        })
    }

    pub fn cancel_operation(
        &self,
        plugin_id: &str,
        account_id: &str,
        operation_id: &str,
    ) -> Result<(), AppError> {
        if self.operations.cancel(plugin_id, account_id, operation_id) {
            Ok(())
        } else {
            Err(AppError::Provider(
                ProviderFailure::canceled().with_request_context(plugin_id, operation_id),
            ))
        }
    }

    pub async fn match_local_remotes(
        &self,
        plugin_id: &str,
        instance_id: &str,
        account_id: &str,
        operation_id: &str,
    ) -> Result<Vec<ProviderBindingSuggestion>, AppError> {
        self.match_local_remotes_inner(plugin_id, instance_id, account_id, operation_id)
            .await
            .map_err(|error| with_request_context(error, plugin_id, operation_id))
    }

    async fn match_local_remotes_inner(
        &self,
        plugin_id: &str,
        instance_id: &str,
        account_id: &str,
        operation_id: &str,
    ) -> Result<Vec<ProviderBindingSuggestion>, AppError> {
        let operation = self.operations.start(plugin_id, account_id, operation_id)?;
        let cancellation = operation.token().clone();
        let account = self.store.get_account(account_id)?;
        if account.instance_id != instance_id {
            return Err(AppError::InvalidInput(
                "Provider account must belong to the selected instance".to_owned(),
            ));
        }
        let instance = self.store.get_instance(instance_id)?;
        let adapter = self.adapters.get(instance.provider_kind)?;
        let normalized_instance =
            normalize_instance_base(&instance.base_url, instance.provider_kind)?;
        let semaphore = self.account_semaphore(account_id);
        let _permit = tokio::select! {
            biased;
            _ = cancellation.cancelled() => return Err(canceled()),
            permit = semaphore.acquire_owned() => {
                permit.map_err(|_| AppError::Provider(ProviderFailure::busy(Some(250))))?
            },
        };
        let value = self
            .secrets
            .get(&account.secret_ref)?
            .ok_or(AppError::SecretStore)?;
        let secret = SensitiveString::new(value);
        let client = ScopedHttpClient::build(&instance)?;
        let mut suggestions = Vec::new();

        for remote in self.store.list_local_remotes()? {
            if cancellation.is_cancelled() {
                return Err(canceled());
            }
            let analysis = analyze_remote(adapter.as_ref(), &normalized_instance, &remote);
            if analysis.detected.is_empty() {
                suggestions.push(empty_suggestion(
                    &remote.repository_id,
                    &remote.name,
                    instance_id,
                    ProviderBindingSuggestionStatus::None,
                ));
                continue;
            }
            let mut repositories = Vec::new();
            let mut verification_failed = false;
            for candidate in &analysis.detected {
                let result = tokio::select! {
                    biased;
                    _ = cancellation.cancelled() => return Err(canceled()),
                    result = adapter.get_repository(
                        AdapterAccountContext {
                            client: &client,
                            secret: secret.as_str(),
                            cancellation: &cancellation,
                        },
                        candidate.identity.clone(),
                    ) => result,
                };
                match result {
                    Ok(repository)
                        if repository.provider_kind == instance.provider_kind
                            && repository.instance_id == instance.id =>
                    {
                        if !repositories.iter().any(|current: &RemoteRepository| {
                            current.repository_id == repository.repository_id
                        }) {
                            repositories.push(repository);
                        }
                    }
                    Ok(_) => return Err(AppError::Provider(ProviderFailure::invalid_response())),
                    Err(error) if is_canceled(&error) => return Err(error),
                    Err(error) => {
                        verification_failed = true;
                        self.record_account_error(account_id, &error);
                    }
                }
            }
            let status = if repositories.len() > 1 {
                ProviderBindingSuggestionStatus::Ambiguous
            } else if repositories.len() == 1 && !verification_failed {
                ProviderBindingSuggestionStatus::Suggested
            } else {
                ProviderBindingSuggestionStatus::Unverified
            };
            let selected = (status == ProviderBindingSuggestionStatus::Suggested)
                .then(|| repositories.first())
                .flatten();
            suggestions.push(ProviderBindingSuggestion {
                repository_id: remote.repository_id,
                remote_name: remote.name,
                instance_id: instance.id.clone(),
                status,
                provider_repository_id: selected.map(|repository| repository.repository_id.clone()),
                full_name: selected.map(|repository| repository.full_name.clone()),
                web_url: selected.map(|repository| repository.web_url.clone()),
                matched_url: selected.and_then(|_| {
                    analysis
                        .detected
                        .first()
                        .map(|candidate| candidate.sanitized.clone())
                }),
                candidates: repositories,
            });
        }
        Ok(suggestions)
    }

    pub async fn bind_remote(&self, input: BindRemoteInput) -> Result<ProviderBinding, AppError> {
        if input.provider_repository_id.trim().is_empty()
            || input.provider_repository_id.chars().any(char::is_control)
        {
            return Err(AppError::InvalidInput(
                "Provider repository ID is invalid".to_owned(),
            ));
        }
        let local = RepositoryRepository::new(self.store.database().clone());
        let remote = local.get_remote(&input.repository_id, &input.remote_name)?;
        let instance = self.store.get_instance(&input.instance_id)?;
        let accounts = self.store.list_accounts(&instance.id)?;
        let effective_account = if let Some(account_id) = input.account_id.as_deref() {
            accounts
                .into_iter()
                .find(|account| account.id == account_id)
                .ok_or_else(|| {
                    AppError::InvalidInput(
                        "binding account must belong to the Provider instance".to_owned(),
                    )
                })?
        } else {
            accounts
                .into_iter()
                .find(|account| account.is_default)
                .ok_or_else(|| {
                    AppError::InvalidInput(
                        "Provider instance requires a default account".to_owned(),
                    )
                })?
        };
        let adapter = self.adapters.get(instance.provider_kind)?;
        let value = self
            .secrets
            .get(&effective_account.secret_ref)?
            .ok_or(AppError::SecretStore)?;
        let secret = SensitiveString::new(value);
        let client = ScopedHttpClient::build(&instance)?;
        let cancellation = tokio_util::sync::CancellationToken::new();
        let verified = adapter
            .get_repository(
                AdapterAccountContext {
                    client: &client,
                    secret: secret.as_str(),
                    cancellation: &cancellation,
                },
                RemoteRepositoryIdentity::Id {
                    repository_id: input.provider_repository_id.clone(),
                },
            )
            .await?;
        if verified.provider_kind != instance.provider_kind
            || verified.instance_id != instance.id
            || verified.repository_id != input.provider_repository_id
        {
            return Err(AppError::Provider(ProviderFailure::invalid_response()));
        }
        let normalized_instance =
            normalize_instance_base(&instance.base_url, instance.provider_kind)?;
        let analysis = analyze_remote(adapter.as_ref(), &normalized_instance, &remote);
        let automatic = analysis
            .detected
            .iter()
            .find(|candidate| identity_matches_repository(&candidate.identity, &verified));
        let matched_url = automatic
            .map(|candidate| candidate.sanitized.clone())
            .or_else(|| analysis.sanitized.first().cloned())
            .ok_or_else(|| AppError::InvalidInput("remote has no supported Git URL".to_owned()))?;
        let now = Utc::now();
        self.store.upsert_binding(ProviderBinding {
            repository_id: remote.repository_id,
            remote_name: remote.name,
            provider_instance_id: instance.id,
            provider_account_id: input.account_id,
            provider_repository_id: verified.repository_id,
            full_name: verified.full_name,
            web_url: verified.web_url,
            matched_url,
            binding_source: if automatic.is_some() {
                BindingSource::Auto
            } else {
                BindingSource::Manual
            },
            bound_at: now,
            updated_at: now,
        })
    }

    pub fn list_bindings_for_account(
        &self,
        account_id: &str,
    ) -> Result<Vec<ProviderBinding>, AppError> {
        self.store.get_account(account_id)?;
        self.store.list_bindings_for_account(account_id)
    }

    pub fn unbind_remote(&self, repository_id: &str, remote_name: &str) -> Result<(), AppError> {
        self.store.delete_binding(repository_id, remote_name)
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

    fn account_semaphore(&self, account_id: &str) -> Arc<Semaphore> {
        Arc::clone(
            self.account_semaphores
                .lock()
                .entry(account_id.to_owned())
                .or_insert_with(|| Arc::new(Semaphore::new(ACCOUNT_DISCOVERY_CONCURRENCY))),
        )
    }

    fn update_rate_health(&self, account_id: &str, state: &ProviderRateLimitState) {
        let status = if state.remaining == Some(0) || state.retry_after_ms.is_some() {
            ProviderConnectionStatus::RateLimited
        } else {
            ProviderConnectionStatus::Connected
        };
        self.health
            .lock()
            .accounts
            .insert(account_id.to_owned(), status);
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

struct DetectedRemote {
    identity: RemoteRepositoryIdentity,
    sanitized: String,
}

struct RemoteAnalysis {
    detected: Vec<DetectedRemote>,
    sanitized: Vec<String>,
}

fn analyze_remote(
    adapter: &dyn RepositoryDiscoveryProvider,
    instance: &NormalizedInstance,
    remote: &Remote,
) -> RemoteAnalysis {
    let mut analysis = RemoteAnalysis {
        detected: Vec::new(),
        sanitized: Vec::new(),
    };
    for value in [remote.fetch_url.as_deref(), remote.push_url.as_deref()]
        .into_iter()
        .flatten()
    {
        let Ok(normalized) = normalize_remote_url(value) else {
            continue;
        };
        let sanitized = sanitized_remote_url(&normalized);
        if !analysis.sanitized.contains(&sanitized) {
            analysis.sanitized.push(sanitized.clone());
        }
        let Some(identity) = adapter.detect_remote(instance, &normalized) else {
            continue;
        };
        if !analysis
            .detected
            .iter()
            .any(|candidate| candidate.identity == identity)
        {
            analysis.detected.push(DetectedRemote {
                identity,
                sanitized,
            });
        }
    }
    analysis
}

fn identity_matches_repository(
    identity: &RemoteRepositoryIdentity,
    repository: &RemoteRepository,
) -> bool {
    match identity {
        RemoteRepositoryIdentity::Id { repository_id } => {
            repository_id == &repository.repository_id
        }
        RemoteRepositoryIdentity::Path { path } => path == &repository.full_name,
    }
}

fn empty_suggestion(
    repository_id: &str,
    remote_name: &str,
    instance_id: &str,
    status: ProviderBindingSuggestionStatus,
) -> ProviderBindingSuggestion {
    ProviderBindingSuggestion {
        repository_id: repository_id.to_owned(),
        remote_name: remote_name.to_owned(),
        instance_id: instance_id.to_owned(),
        status,
        provider_repository_id: None,
        full_name: None,
        web_url: None,
        matched_url: None,
        candidates: Vec::new(),
    }
}

fn validate_repository_query(query: &ProviderRepositoryQuery) -> Result<(), AppError> {
    if query.page_size == 0
        || query.page_size > 100
        || query.search.chars().count() > 256
        || query.search.chars().any(char::is_control)
        || query.namespace.as_ref().is_some_and(|namespace| {
            namespace.trim().is_empty()
                || namespace.chars().count() > 1024
                || namespace.chars().any(char::is_control)
        })
    {
        return Err(AppError::InvalidInput(
            "Provider repository query is invalid".to_owned(),
        ));
    }
    Ok(())
}

fn repository_matches(item: &RemoteRepository, query: &ProviderRepositoryQuery) -> bool {
    let search_matches = query.search.trim().is_empty()
        || item
            .full_name
            .to_lowercase()
            .contains(&query.search.trim().to_lowercase());
    let namespace_matches = query
        .namespace
        .as_ref()
        .is_none_or(|namespace| item.namespace.to_lowercase() == namespace.trim().to_lowercase());
    let visibility_matches = query
        .visibility
        .is_none_or(|visibility| item.visibility == visibility);
    let archived_matches = match query.archived {
        ProviderArchivedFilter::All => true,
        ProviderArchivedFilter::Active => !item.archived,
        ProviderArchivedFilter::Archived => item.archived,
    };
    search_matches && namespace_matches && visibility_matches && archived_matches
}

fn take_buffered(
    buffered: &mut Vec<RemoteRepository>,
    output: &mut Vec<RemoteRepository>,
    page_size: usize,
) {
    let count = buffered.len().min(page_size.saturating_sub(output.len()));
    output.extend(buffered.drain(..count));
}

fn with_request_context(error: AppError, plugin_id: &str, operation_id: &str) -> AppError {
    match error {
        AppError::Provider(failure) => AppError::Provider(
            failure.with_request_context(plugin_id.to_owned(), operation_id.to_owned()),
        ),
        error => error,
    }
}

fn is_canceled(error: &AppError) -> bool {
    matches!(error, AppError::Provider(failure) if failure.code() == "provider.request-canceled")
}

fn canceled() -> AppError {
    AppError::Provider(ProviderFailure::canceled())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::time::Duration;

    use chrono::Utc;
    use futures_util::future::BoxFuture;
    use parking_lot::Mutex;
    use tempfile::tempdir;
    use tokio::sync::Notify;
    use uuid::Uuid;

    use super::{
        BindRemoteInput, CreateInstanceInput, DeleteAccountInput, ListRepositoriesInput,
        ProviderService,
    };
    use crate::db::Database;
    use crate::error::{AppError, ErrorEnvelope, ProviderFailure};
    use crate::git::model::{Remote, Repository, RepositoryKind};
    use crate::git::repository::RepositoryRepository;
    use crate::providers::adapter::{
        AdapterAccountContext, ProviderAdapterRegistry, RepositoryDiscoveryProvider,
    };
    use crate::providers::http::ScopedHttpClient;
    use crate::providers::model::{
        AccountDeletionResolution, AccountIdentity, AdapterCursor, AdapterListRequest, AdapterPage,
        InstanceMetadata, ProviderArchivedFilter, ProviderBindingSuggestionStatus,
        ProviderConnectionStatus, ProviderKind, ProviderPermission, ProviderRateLimitState,
        ProviderRepositoryDirection, ProviderRepositoryQuery, ProviderRepositorySort,
        ProviderVisibility, RemoteRepository, RemoteRepositoryIdentity,
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

    struct PagingProvider;

    impl RepositoryDiscoveryProvider for PagingProvider {
        fn kind(&self) -> ProviderKind {
            ProviderKind::Gitlab
        }

        fn validate_instance<'a>(
            &'a self,
            _client: &'a ScopedHttpClient,
        ) -> BoxFuture<'a, Result<InstanceMetadata, AppError>> {
            Box::pin(async {
                Ok(InstanceMetadata {
                    server_version: None,
                })
            })
        }

        fn authenticate_account<'a>(
            &'a self,
            _client: &'a ScopedHttpClient,
            _secret: &'a str,
        ) -> BoxFuture<'a, Result<AccountIdentity, AppError>> {
            Box::pin(async {
                Ok(AccountIdentity {
                    provider_user_id: "paging-user".to_owned(),
                    username: "paging-user".to_owned(),
                    display_name: None,
                    avatar_url: None,
                })
            })
        }

        fn list_repositories<'a>(
            &'a self,
            context: AdapterAccountContext<'a>,
            request: AdapterListRequest,
        ) -> BoxFuture<'a, Result<AdapterPage, AppError>> {
            Box::pin(async move {
                let page = match request.cursor {
                    None | Some(AdapterCursor::Page(1)) => 1,
                    Some(AdapterCursor::Page(page)) => page,
                    Some(AdapterCursor::Keyset(_)) => 99,
                };
                let names: &[&str] = if page == 1 {
                    &["group/unrelated"]
                } else {
                    &["group/skill-a", "group/skill-b", "group/skill-c"]
                };
                Ok(AdapterPage {
                    items: names
                        .iter()
                        .enumerate()
                        .map(|(index, full_name)| {
                            remote_repository(
                                context.client.instance_id(),
                                &format!("{page}-{index}"),
                                full_name,
                            )
                        })
                        .collect(),
                    next_cursor: (page == 1).then_some(AdapterCursor::Page(2)),
                    rate_limit: (page == 2).then_some(ProviderRateLimitState {
                        limit: Some(100),
                        remaining: Some(0),
                        reset_at: None,
                        retry_after_ms: Some(1_000),
                    }),
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

    fn remote_repository(instance_id: &str, id: &str, full_name: &str) -> RemoteRepository {
        let (namespace, name) = full_name.rsplit_once('/').unwrap();
        RemoteRepository {
            provider_kind: ProviderKind::Gitlab,
            instance_id: instance_id.to_owned(),
            repository_id: id.to_owned(),
            namespace: namespace.to_owned(),
            name: name.to_owned(),
            full_name: full_name.to_owned(),
            web_url: format!("https://gitlab.example/{full_name}"),
            https_url: format!("https://gitlab.example/{full_name}.git"),
            ssh_url: format!("git@gitlab.example:{full_name}.git"),
            default_branch: Some("main".to_owned()),
            visibility: ProviderVisibility::Private,
            archived: false,
            fork: false,
            permission: ProviderPermission::Read,
            updated_at: Utc::now(),
        }
    }

    struct MatchingProvider {
        get_calls: AtomicUsize,
    }

    impl MatchingProvider {
        fn new() -> Self {
            Self {
                get_calls: AtomicUsize::new(0),
            }
        }
    }

    impl RepositoryDiscoveryProvider for MatchingProvider {
        fn kind(&self) -> ProviderKind {
            ProviderKind::Gitlab
        }

        fn validate_instance<'a>(
            &'a self,
            _client: &'a ScopedHttpClient,
        ) -> BoxFuture<'a, Result<InstanceMetadata, AppError>> {
            Box::pin(async {
                Ok(InstanceMetadata {
                    server_version: None,
                })
            })
        }

        fn authenticate_account<'a>(
            &'a self,
            _client: &'a ScopedHttpClient,
            _secret: &'a str,
        ) -> BoxFuture<'a, Result<AccountIdentity, AppError>> {
            Box::pin(async {
                Ok(AccountIdentity {
                    provider_user_id: "matching-user".to_owned(),
                    username: "matching-user".to_owned(),
                    display_name: None,
                    avatar_url: None,
                })
            })
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
            context: AdapterAccountContext<'a>,
            identity: RemoteRepositoryIdentity,
        ) -> BoxFuture<'a, Result<RemoteRepository, AppError>> {
            self.get_calls.fetch_add(1, Ordering::SeqCst);
            Box::pin(async move {
                let (id, path) = match identity {
                    RemoteRepositoryIdentity::Id { repository_id } if repository_id == "42" => {
                        ("42".to_owned(), "group/skill".to_owned())
                    }
                    RemoteRepositoryIdentity::Id { repository_id } => {
                        return Err(AppError::NotFound(format!("fake {repository_id}")));
                    }
                    RemoteRepositoryIdentity::Path { path } if path == "group/skill" => {
                        ("42".to_owned(), path)
                    }
                    RemoteRepositoryIdentity::Path { path } => ("99".to_owned(), path),
                };
                Ok(remote_repository(context.client.instance_id(), &id, &path))
            })
        }

        fn detect_remote(
            &self,
            instance: &NormalizedInstance,
            remote: &NormalizedRemoteUrl,
        ) -> Option<RemoteRepositoryIdentity> {
            (instance.host == remote.host).then(|| RemoteRepositoryIdentity::Path {
                path: remote.path.clone(),
            })
        }
    }

    struct BlockingProvider {
        entered: AtomicUsize,
        entered_notify: Notify,
    }

    impl BlockingProvider {
        fn new() -> Self {
            Self {
                entered: AtomicUsize::new(0),
                entered_notify: Notify::new(),
            }
        }

        async fn wait_for_entered(&self, expected: usize) {
            loop {
                let notified = self.entered_notify.notified();
                if self.entered.load(Ordering::SeqCst) >= expected {
                    return;
                }
                notified.await;
            }
        }
    }

    impl RepositoryDiscoveryProvider for BlockingProvider {
        fn kind(&self) -> ProviderKind {
            ProviderKind::Gitlab
        }

        fn validate_instance<'a>(
            &'a self,
            _client: &'a ScopedHttpClient,
        ) -> BoxFuture<'a, Result<InstanceMetadata, AppError>> {
            Box::pin(async {
                Ok(InstanceMetadata {
                    server_version: None,
                })
            })
        }

        fn authenticate_account<'a>(
            &'a self,
            _client: &'a ScopedHttpClient,
            _secret: &'a str,
        ) -> BoxFuture<'a, Result<AccountIdentity, AppError>> {
            Box::pin(async {
                Ok(AccountIdentity {
                    provider_user_id: "blocking-user".to_owned(),
                    username: "blocking-user".to_owned(),
                    display_name: None,
                    avatar_url: None,
                })
            })
        }

        fn list_repositories<'a>(
            &'a self,
            context: AdapterAccountContext<'a>,
            _request: AdapterListRequest,
        ) -> BoxFuture<'a, Result<AdapterPage, AppError>> {
            self.entered.fetch_add(1, Ordering::SeqCst);
            self.entered_notify.notify_waiters();
            Box::pin(async move {
                context.cancellation.cancelled().await;
                Err(AppError::Provider(ProviderFailure::canceled()))
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

    #[tokio::test]
    async fn discovery_fills_a_page_across_upstream_pages_and_uses_one_use_cursors() {
        let database = Database::open_in_memory().unwrap();
        database
            .with_connection(|connection| {
                connection.execute(
                    "INSERT INTO plugin_installations(plugin_id,version,kind,root_path,enabled,installed_at,updated_at) VALUES('git-ramus.provider.gitlab','0.1.0','builtin','/builtin/gitlab',1,?1,?1)",
                    [Utc::now().to_rfc3339()],
                )?;
                Ok(())
            })
            .unwrap();
        let store = ProviderStore::new(database.clone());
        let registry = ProviderAdapterRegistry::for_test(
            database,
            ProviderKind::Gitlab,
            Arc::new(PagingProvider),
        );
        let service =
            ProviderService::new(store, Arc::new(ScriptedSecretStore::default()), registry);
        let instance = service
            .create_instance(CreateInstanceInput {
                provider_kind: ProviderKind::Gitlab,
                display_name: "Paging GitLab".to_owned(),
                base_url: "https://gitlab.example".to_owned(),
                custom_ca_path: None,
            })
            .await
            .unwrap();
        let account = service
            .connect_account(
                &instance.id,
                SensitiveString::new("paging-token".to_owned()),
            )
            .await
            .unwrap();
        let query = ProviderRepositoryQuery {
            search: "skill".to_owned(),
            visibility: None,
            namespace: None,
            archived: ProviderArchivedFilter::All,
            sort: ProviderRepositorySort::Name,
            direction: ProviderRepositoryDirection::Asc,
            page_size: 2,
        };
        let first = service
            .list_repositories(
                "plugin-a",
                ListRepositoriesInput {
                    account_id: account.id.clone(),
                    query: query.clone(),
                    cursor: None,
                    operation_id: Uuid::new_v4().to_string(),
                },
            )
            .await
            .unwrap();
        assert_eq!(
            first
                .items
                .iter()
                .map(|item| item.full_name.as_str())
                .collect::<Vec<_>>(),
            ["group/skill-a", "group/skill-b"]
        );
        assert!(first.has_more);
        assert_eq!(
            service.list_accounts(&instance.id).unwrap()[0].status,
            ProviderConnectionStatus::RateLimited
        );
        let cursor = first.next_cursor.unwrap();
        assert!(
            service
                .list_repositories(
                    "plugin-b",
                    ListRepositoriesInput {
                        account_id: account.id.clone(),
                        query: query.clone(),
                        cursor: Some(cursor.clone()),
                        operation_id: Uuid::new_v4().to_string(),
                    },
                )
                .await
                .is_err()
        );
        let second = service
            .list_repositories(
                "plugin-a",
                ListRepositoriesInput {
                    account_id: account.id.clone(),
                    query,
                    cursor: Some(cursor.clone()),
                    operation_id: Uuid::new_v4().to_string(),
                },
            )
            .await
            .unwrap();
        assert_eq!(second.items[0].full_name, "group/skill-c");
        assert!(!second.has_more);
        assert!(
            service
                .list_repositories(
                    "plugin-a",
                    ListRepositoriesInput {
                        account_id: account.id,
                        query: ProviderRepositoryQuery {
                            search: "skill".to_owned(),
                            visibility: None,
                            namespace: None,
                            archived: ProviderArchivedFilter::All,
                            sort: ProviderRepositorySort::Name,
                            direction: ProviderRepositoryDirection::Asc,
                            page_size: 2,
                        },
                        cursor: Some(cursor),
                        operation_id: Uuid::new_v4().to_string(),
                    },
                )
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn discovery_concurrency_queues_the_fifth_request_and_cancel_never_crosses_plugin_scope()
    {
        let database = Database::open_in_memory().unwrap();
        database
            .with_connection(|connection| {
                connection.execute(
                    "INSERT INTO plugin_installations(plugin_id,version,kind,root_path,enabled,installed_at,updated_at) VALUES('git-ramus.provider.gitlab','0.1.0','builtin','/builtin/gitlab',1,?1,?1)",
                    [Utc::now().to_rfc3339()],
                )?;
                Ok(())
            })
            .unwrap();
        let provider = Arc::new(BlockingProvider::new());
        let adapters = ProviderAdapterRegistry::for_test(
            database.clone(),
            ProviderKind::Gitlab,
            provider.clone(),
        );
        let service = Arc::new(ProviderService::new(
            ProviderStore::new(database),
            Arc::new(ScriptedSecretStore::default()),
            adapters,
        ));
        let instance = service
            .create_instance(CreateInstanceInput {
                provider_kind: ProviderKind::Gitlab,
                display_name: "Blocking GitLab".to_owned(),
                base_url: "https://gitlab.example".to_owned(),
                custom_ca_path: None,
            })
            .await
            .unwrap();
        let account = service
            .connect_account(
                &instance.id,
                SensitiveString::new("blocking-token".to_owned()),
            )
            .await
            .unwrap();
        let query = ProviderRepositoryQuery {
            search: String::new(),
            visibility: None,
            namespace: None,
            archived: ProviderArchivedFilter::All,
            sort: ProviderRepositorySort::Name,
            direction: ProviderRepositoryDirection::Asc,
            page_size: 10,
        };
        let operation_ids = (0..5)
            .map(|_| Uuid::new_v4().to_string())
            .collect::<Vec<_>>();
        let mut running = Vec::new();
        for operation_id in &operation_ids[..4] {
            let service = Arc::clone(&service);
            let account_id = account.id.clone();
            let query = query.clone();
            let operation_id = operation_id.clone();
            running.push(tokio::spawn(async move {
                service
                    .list_repositories(
                        "plugin-a",
                        ListRepositoriesInput {
                            account_id,
                            query,
                            cursor: None,
                            operation_id,
                        },
                    )
                    .await
            }));
        }
        provider.wait_for_entered(4).await;
        let fifth_service = Arc::clone(&service);
        let fifth_account = account.id.clone();
        let fifth_query = query.clone();
        let fifth_operation = operation_ids[4].clone();
        let fifth = tokio::spawn(async move {
            fifth_service
                .list_repositories(
                    "plugin-a",
                    ListRepositoriesInput {
                        account_id: fifth_account,
                        query: fifth_query,
                        cursor: None,
                        operation_id: fifth_operation,
                    },
                )
                .await
        });
        tokio::time::sleep(Duration::from_millis(25)).await;
        assert_eq!(provider.entered.load(Ordering::SeqCst), 4);
        assert!(
            service
                .cancel_operation("plugin-b", &account.id, &operation_ids[4])
                .is_err()
        );
        assert!(
            service
                .cancel_operation("plugin-a", &account.id, &operation_ids[4])
                .is_ok()
        );
        let canceled = ErrorEnvelope::from(fifth.await.unwrap().unwrap_err());
        assert_eq!(canceled.code, "provider.request-canceled");
        assert_eq!(canceled.plugin_id.as_deref(), Some("plugin-a"));
        assert_eq!(
            canceled.operation_id.as_deref(),
            Some(operation_ids[4].as_str())
        );
        assert_eq!(provider.entered.load(Ordering::SeqCst), 4);

        for operation_id in &operation_ids[..4] {
            service
                .cancel_operation("plugin-a", &account.id, operation_id)
                .unwrap();
        }
        for task in running {
            assert!(task.await.unwrap().is_err());
        }
    }

    #[tokio::test]
    async fn matching_deduplicates_https_and_ssh_then_binding_rereads_current_remote() {
        let database = Database::open_in_memory().unwrap();
        database
            .with_connection(|connection| {
                connection.execute(
                    "INSERT INTO plugin_installations(plugin_id,version,kind,root_path,enabled,installed_at,updated_at) VALUES('git-ramus.provider.gitlab','0.1.0','builtin','/builtin/gitlab',1,?1,?1)",
                    [Utc::now().to_rfc3339()],
                )?;
                Ok(())
            })
            .unwrap();
        let local = RepositoryRepository::new(database.clone());
        let repository = Repository::new("/test/repository", "repository", RepositoryKind::Normal);
        local.create(&repository).unwrap();
        local
            .add_remote(&Remote {
                repository_id: repository.id.clone(),
                name: "origin".to_owned(),
                fetch_url: Some("https://gitlab.example/group/skill.git".to_owned()),
                push_url: Some("git@gitlab.example:group/skill.git".to_owned()),
            })
            .unwrap();
        let provider = Arc::new(MatchingProvider::new());
        let adapters = ProviderAdapterRegistry::for_test(
            database.clone(),
            ProviderKind::Gitlab,
            provider.clone(),
        );
        let service = ProviderService::new(
            ProviderStore::new(database),
            Arc::new(ScriptedSecretStore::default()),
            adapters,
        );
        let instance = service
            .create_instance(CreateInstanceInput {
                provider_kind: ProviderKind::Gitlab,
                display_name: "Matching GitLab".to_owned(),
                base_url: "https://gitlab.example".to_owned(),
                custom_ca_path: None,
            })
            .await
            .unwrap();
        let account = service
            .connect_account(
                &instance.id,
                SensitiveString::new("matching-token".to_owned()),
            )
            .await
            .unwrap();

        let suggestions = service
            .match_local_remotes(
                "plugin-a",
                &instance.id,
                &account.id,
                &Uuid::new_v4().to_string(),
            )
            .await
            .unwrap();
        assert_eq!(suggestions.len(), 1);
        assert_eq!(
            suggestions[0].status,
            ProviderBindingSuggestionStatus::Suggested
        );
        assert_eq!(suggestions[0].candidates.len(), 1);
        assert_eq!(provider.get_calls.load(Ordering::SeqCst), 1);

        local
            .add_remote(&Remote {
                repository_id: repository.id.clone(),
                name: "origin".to_owned(),
                fetch_url: Some("https://gitlab.example/group/skill.git".to_owned()),
                push_url: Some("git@gitlab.example:group/other.git".to_owned()),
            })
            .unwrap();
        let ambiguous = service
            .match_local_remotes(
                "plugin-a",
                &instance.id,
                &account.id,
                &Uuid::new_v4().to_string(),
            )
            .await
            .unwrap();
        assert_eq!(
            ambiguous[0].status,
            ProviderBindingSuggestionStatus::Ambiguous
        );
        assert_eq!(ambiguous[0].candidates.len(), 2);
        local
            .add_remote(&Remote {
                repository_id: repository.id.clone(),
                name: "origin".to_owned(),
                fetch_url: None,
                push_url: Some("git@gitlab.example:group/skill.git".to_owned()),
            })
            .unwrap();

        let other_instance = service
            .create_instance(CreateInstanceInput {
                provider_kind: ProviderKind::Gitlab,
                display_name: "Other GitLab".to_owned(),
                base_url: "https://other.gitlab.example".to_owned(),
                custom_ca_path: None,
            })
            .await
            .unwrap();
        let other_account = service
            .connect_account(
                &other_instance.id,
                SensitiveString::new("other-token".to_owned()),
            )
            .await
            .unwrap();
        assert!(
            service
                .bind_remote(BindRemoteInput {
                    repository_id: repository.id.clone(),
                    remote_name: "origin".to_owned(),
                    instance_id: instance.id.clone(),
                    account_id: Some(other_account.id),
                    provider_repository_id: "42".to_owned(),
                })
                .await
                .is_err()
        );

        let binding = service
            .bind_remote(BindRemoteInput {
                repository_id: repository.id.clone(),
                remote_name: "origin".to_owned(),
                instance_id: instance.id,
                account_id: None,
                provider_repository_id: "42".to_owned(),
            })
            .await
            .unwrap();
        assert!(binding.provider_account_id.is_none());
        assert_eq!(
            binding.binding_source,
            crate::providers::model::BindingSource::Auto
        );
        assert!(!binding.matched_url.contains("matching-token"));
        assert_eq!(binding.matched_url, "git@gitlab.example:group/skill.git");
        assert_eq!(
            service
                .list_bindings_for_account(&account.id)
                .unwrap()
                .len(),
            1
        );
        service
            .unbind_remote(&repository.id, "origin")
            .expect("binding removes");
        assert!(
            service
                .list_bindings_for_account(&account.id)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            local
                .get_remote(&repository.id, "origin")
                .unwrap()
                .fetch_url,
            None
        );
        assert_eq!(
            local
                .get_remote(&repository.id, "origin")
                .unwrap()
                .push_url
                .as_deref(),
            Some("git@gitlab.example:group/skill.git")
        );
    }
}
