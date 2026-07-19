use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use parking_lot::Mutex;
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

use crate::error::{AppError, ProviderFailure};
use crate::providers::model::{
    AdapterCursor, ProviderKind, ProviderRepositoryQuery, RemoteRepository,
};

const DEFAULT_CURSOR_CAPACITY: usize = 512;
const DEFAULT_GLOBAL_OPERATION_LIMIT: usize = 1024;
const DEFAULT_ACCOUNT_OPERATION_LIMIT: usize = 64;

pub struct CursorEntry {
    pub plugin_id: String,
    pub provider_kind: ProviderKind,
    pub instance_id: String,
    pub account_id: String,
    pub query: ProviderRepositoryQuery,
    pub adapter_cursor: Option<AdapterCursor>,
    pub buffered: Vec<RemoteRepository>,
    pub expires_at: Instant,
}

#[derive(Clone)]
pub struct CursorStore {
    entries: Arc<Mutex<HashMap<String, CursorEntry>>>,
    capacity: usize,
}

impl Default for CursorStore {
    fn default() -> Self {
        Self::with_capacity(DEFAULT_CURSOR_CAPACITY)
    }
}

impl CursorStore {
    fn with_capacity(capacity: usize) -> Self {
        Self {
            entries: Arc::new(Mutex::new(HashMap::new())),
            capacity: capacity.max(1),
        }
    }

    pub fn insert(&self, entry: CursorEntry) -> String {
        let now = Instant::now();
        let mut entries = self.entries.lock();
        entries.retain(|_, entry| entry.expires_at > now);
        if entries.len() >= self.capacity {
            let nearest = entries
                .iter()
                .min_by_key(|(_, entry)| entry.expires_at)
                .map(|(id, _)| id.clone());
            if let Some(nearest) = nearest {
                entries.remove(&nearest);
            }
        }
        let id = Uuid::new_v4().to_string();
        entries.insert(id.clone(), entry);
        id
    }

    pub fn take(
        &self,
        cursor: &str,
        plugin_id: &str,
        provider_kind: ProviderKind,
        instance_id: &str,
        account_id: &str,
        query: &ProviderRepositoryQuery,
    ) -> Result<CursorEntry, AppError> {
        let now = Instant::now();
        let mut entries = self.entries.lock();
        entries.retain(|_, entry| entry.expires_at > now);
        let valid = entries.get(cursor).is_some_and(|entry| {
            entry.plugin_id == plugin_id
                && entry.provider_kind == provider_kind
                && entry.instance_id == instance_id
                && entry.account_id == account_id
                && &entry.query == query
        });
        if !valid {
            return Err(AppError::Provider(ProviderFailure::invalid_cursor()));
        }
        entries
            .remove(cursor)
            .ok_or_else(|| AppError::Provider(ProviderFailure::invalid_cursor()))
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.entries.lock().len()
    }
}

type OperationKey = (String, String, String);

struct OperationState {
    tokens: HashMap<OperationKey, CancellationToken>,
}

struct OperationInner {
    state: Mutex<OperationState>,
    idle: Notify,
    global_limit: usize,
    account_limit: usize,
}

#[derive(Clone)]
pub struct OperationRegistry {
    inner: Arc<OperationInner>,
}

impl Default for OperationRegistry {
    fn default() -> Self {
        Self::new(
            DEFAULT_GLOBAL_OPERATION_LIMIT,
            DEFAULT_ACCOUNT_OPERATION_LIMIT,
        )
    }
}

impl OperationRegistry {
    fn new(global_limit: usize, account_limit: usize) -> Self {
        Self {
            inner: Arc::new(OperationInner {
                state: Mutex::new(OperationState {
                    tokens: HashMap::new(),
                }),
                idle: Notify::new(),
                global_limit: global_limit.max(1),
                account_limit: account_limit.max(1),
            }),
        }
    }

    #[cfg(test)]
    fn with_limits(global_limit: usize, account_limit: usize) -> Self {
        Self::new(global_limit, account_limit)
    }

    pub fn start(
        &self,
        plugin_id: &str,
        account_id: &str,
        operation_id: &str,
    ) -> Result<OperationGuard, AppError> {
        let key = (
            plugin_id.to_owned(),
            account_id.to_owned(),
            operation_id.to_owned(),
        );
        let mut state = self.inner.state.lock();
        let account_operations = state
            .tokens
            .keys()
            .filter(|(_, current_account, _)| current_account == account_id)
            .count();
        if state.tokens.contains_key(&key)
            || state.tokens.len() >= self.inner.global_limit
            || account_operations >= self.inner.account_limit
        {
            return Err(AppError::Provider(ProviderFailure::busy(Some(250))));
        }
        let token = CancellationToken::new();
        state.tokens.insert(key.clone(), token.clone());
        drop(state);
        Ok(OperationGuard {
            key,
            token,
            registry: self.clone(),
        })
    }

    pub fn cancel(&self, plugin_id: &str, account_id: &str, operation_id: &str) -> bool {
        let key = (
            plugin_id.to_owned(),
            account_id.to_owned(),
            operation_id.to_owned(),
        );
        let token = self.inner.state.lock().tokens.get(&key).cloned();
        if let Some(token) = token {
            token.cancel();
            true
        } else {
            false
        }
    }

    pub fn cancel_for_plugin_account(&self, plugin_id: &str, account_id: &str) -> usize {
        self.cancel_matching(|(current_plugin, current_account, _)| {
            current_plugin == plugin_id && current_account == account_id
        })
    }

    pub fn cancel_for_account(&self, account_id: &str) -> usize {
        self.cancel_matching(|(_, current_account, _)| current_account == account_id)
    }

    pub async fn wait_for_plugin_account_idle(
        &self,
        plugin_id: &str,
        account_id: &str,
        timeout: Duration,
    ) -> Result<(), AppError> {
        self.wait_until_idle(
            |(current_plugin, current_account, _)| {
                current_plugin == plugin_id && current_account == account_id
            },
            timeout,
        )
        .await
    }

    pub async fn wait_for_account_idle(
        &self,
        account_id: &str,
        timeout: Duration,
    ) -> Result<(), AppError> {
        self.wait_until_idle(
            |(_, current_account, _)| current_account == account_id,
            timeout,
        )
        .await
    }

    fn cancel_matching(&self, predicate: impl Fn(&OperationKey) -> bool) -> usize {
        let tokens = self
            .inner
            .state
            .lock()
            .tokens
            .iter()
            .filter(|(key, _)| predicate(key))
            .map(|(_, token)| token.clone())
            .collect::<Vec<_>>();
        let count = tokens.len();
        for token in tokens {
            token.cancel();
        }
        count
    }

    async fn wait_until_idle(
        &self,
        predicate: impl Fn(&OperationKey) -> bool,
        timeout: Duration,
    ) -> Result<(), AppError> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let notified = self.inner.idle.notified();
            if !self.inner.state.lock().tokens.keys().any(&predicate) {
                return Ok(());
            }
            if tokio::time::timeout_at(deadline, notified).await.is_err() {
                return Err(AppError::Provider(ProviderFailure::busy(Some(250))));
            }
        }
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.inner.state.lock().tokens.len()
    }
}

pub struct OperationGuard {
    key: OperationKey,
    token: CancellationToken,
    registry: OperationRegistry,
}

impl OperationGuard {
    pub fn token(&self) -> &CancellationToken {
        &self.token
    }
}

impl Drop for OperationGuard {
    fn drop(&mut self) {
        self.registry.inner.state.lock().tokens.remove(&self.key);
        self.registry.inner.idle.notify_waiters();
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use crate::providers::model::{
        AdapterCursor, ProviderArchivedFilter, ProviderKind, ProviderRepositoryDirection,
        ProviderRepositoryQuery, ProviderRepositorySort,
    };

    use super::{CursorEntry, CursorStore, OperationRegistry};

    const INSTANCE_ID: &str = "6da75ccf-f7df-4bf2-92b7-2c158765726f";
    const OTHER_INSTANCE_ID: &str = "58b42edb-cd6b-4f15-b73e-a034fe731a22";
    const ACCOUNT_ID: &str = "7f3c0214-373c-4d43-b0c7-cdaed1cbcc50";
    const OTHER_ACCOUNT_ID: &str = "04531f52-d05f-42e5-87f6-d023293efa9a";
    const OPERATION_ID: &str = "f84223af-c753-4209-be36-12d381375fcb";

    fn query(search: &str) -> ProviderRepositoryQuery {
        ProviderRepositoryQuery {
            search: search.to_owned(),
            visibility: None,
            namespace: None,
            archived: ProviderArchivedFilter::All,
            sort: ProviderRepositorySort::Name,
            direction: ProviderRepositoryDirection::Asc,
            page_size: 30,
        }
    }

    fn cursor_entry(
        plugin_id: &str,
        provider_kind: ProviderKind,
        instance_id: &str,
        account_id: &str,
        query: ProviderRepositoryQuery,
    ) -> CursorEntry {
        CursorEntry {
            plugin_id: plugin_id.to_owned(),
            provider_kind,
            instance_id: instance_id.to_owned(),
            account_id: account_id.to_owned(),
            query,
            adapter_cursor: Some(AdapterCursor::Page(2)),
            buffered: Vec::new(),
            expires_at: Instant::now() + Duration::from_secs(600),
        }
    }

    #[test]
    fn cursor_is_one_use_and_bound_to_plugin_provider_instance_account_and_query() {
        let store = CursorStore::default();
        let cursor = store.insert(cursor_entry(
            "plugin-a",
            ProviderKind::Gitlab,
            INSTANCE_ID,
            ACCOUNT_ID,
            query("skill"),
        ));
        assert!(
            store
                .take(
                    &cursor,
                    "plugin-b",
                    ProviderKind::Gitlab,
                    INSTANCE_ID,
                    ACCOUNT_ID,
                    &query("skill")
                )
                .is_err()
        );
        assert!(
            store
                .take(
                    &cursor,
                    "plugin-a",
                    ProviderKind::Github,
                    INSTANCE_ID,
                    ACCOUNT_ID,
                    &query("skill")
                )
                .is_err()
        );
        assert!(
            store
                .take(
                    &cursor,
                    "plugin-a",
                    ProviderKind::Gitlab,
                    OTHER_INSTANCE_ID,
                    ACCOUNT_ID,
                    &query("skill")
                )
                .is_err()
        );
        assert!(
            store
                .take(
                    &cursor,
                    "plugin-a",
                    ProviderKind::Gitlab,
                    INSTANCE_ID,
                    OTHER_ACCOUNT_ID,
                    &query("skill")
                )
                .is_err()
        );
        assert!(
            store
                .take(
                    &cursor,
                    "plugin-a",
                    ProviderKind::Gitlab,
                    INSTANCE_ID,
                    ACCOUNT_ID,
                    &query("other")
                )
                .is_err()
        );
        assert!(
            store
                .take(
                    &cursor,
                    "plugin-a",
                    ProviderKind::Gitlab,
                    INSTANCE_ID,
                    ACCOUNT_ID,
                    &query("skill")
                )
                .is_ok()
        );
        assert!(
            store
                .take(
                    &cursor,
                    "plugin-a",
                    ProviderKind::Gitlab,
                    INSTANCE_ID,
                    ACCOUNT_ID,
                    &query("skill")
                )
                .is_err()
        );
    }

    #[test]
    fn cursor_store_purges_expired_and_evicts_the_nearest_expiration_at_capacity() {
        let store = CursorStore::with_capacity(2);
        let expired = store.insert(CursorEntry {
            expires_at: Instant::now() - Duration::from_secs(1),
            ..cursor_entry(
                "plugin-a",
                ProviderKind::Gitlab,
                INSTANCE_ID,
                ACCOUNT_ID,
                query("expired"),
            )
        });
        let first = store.insert(cursor_entry(
            "plugin-a",
            ProviderKind::Gitlab,
            INSTANCE_ID,
            ACCOUNT_ID,
            query("first"),
        ));
        let second = store.insert(CursorEntry {
            expires_at: Instant::now() + Duration::from_secs(1200),
            ..cursor_entry(
                "plugin-a",
                ProviderKind::Gitlab,
                INSTANCE_ID,
                ACCOUNT_ID,
                query("second"),
            )
        });
        let third = store.insert(CursorEntry {
            expires_at: Instant::now() + Duration::from_secs(1800),
            ..cursor_entry(
                "plugin-a",
                ProviderKind::Gitlab,
                INSTANCE_ID,
                ACCOUNT_ID,
                query("third"),
            )
        });
        assert_eq!(store.len(), 2);
        for (id, term, expected) in [
            (expired, "expired", false),
            (first, "first", false),
            (second, "second", true),
            (third, "third", true),
        ] {
            assert_eq!(
                store
                    .take(
                        &id,
                        "plugin-a",
                        ProviderKind::Gitlab,
                        INSTANCE_ID,
                        ACCOUNT_ID,
                        &query(term)
                    )
                    .is_ok(),
                expected
            );
        }
    }

    #[tokio::test]
    async fn operation_cancel_only_cancels_the_owning_plugin_request() {
        let registry = OperationRegistry::default();
        let guard = registry
            .start("plugin-a", ACCOUNT_ID, OPERATION_ID)
            .unwrap();
        assert!(!registry.cancel("plugin-b", ACCOUNT_ID, OPERATION_ID));
        assert!(!registry.cancel("plugin-a", OTHER_ACCOUNT_ID, OPERATION_ID));
        assert!(registry.cancel("plugin-a", ACCOUNT_ID, OPERATION_ID));
        guard.token().cancelled().await;
    }

    #[tokio::test]
    async fn operation_limits_reject_without_retaining_tokens_and_idle_waits_for_drop() {
        let registry = OperationRegistry::with_limits(2, 1);
        let first = registry
            .start("plugin-a", ACCOUNT_ID, "operation-a")
            .unwrap();
        assert!(
            registry
                .start("plugin-b", ACCOUNT_ID, "operation-b")
                .is_err()
        );
        let second = registry
            .start("plugin-a", OTHER_ACCOUNT_ID, "operation-b")
            .unwrap();
        assert!(
            registry
                .start("plugin-a", "third-account", "operation-c")
                .is_err()
        );
        assert!(
            registry
                .start("plugin-a", ACCOUNT_ID, "operation-a")
                .is_err()
        );
        assert_eq!(registry.len(), 2);

        assert_eq!(
            registry.cancel_for_plugin_account("plugin-a", ACCOUNT_ID),
            1
        );
        first.token().cancelled().await;
        assert!(
            registry
                .wait_for_plugin_account_idle("plugin-a", ACCOUNT_ID, Duration::from_millis(5))
                .await
                .is_err()
        );
        drop(first);
        registry
            .wait_for_plugin_account_idle("plugin-a", ACCOUNT_ID, Duration::from_secs(1))
            .await
            .unwrap();
        drop(second);
        registry
            .wait_for_account_idle(OTHER_ACCOUNT_ID, Duration::from_secs(1))
            .await
            .unwrap();
    }
}
