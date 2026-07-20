use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Weak};

use parking_lot::Mutex;

use crate::error::{AppError, TransportFailure};

#[derive(Default)]
struct RegistryState {
    operations: HashMap<String, OperationEntry>,
    resources: HashMap<String, String>,
}

struct OperationEntry {
    resource_key: Option<String>,
    plugin_id: String,
    domain: TransportAuthorizationDomain,
    cancellation: Arc<AtomicBool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransportAuthorizationDomain {
    CloneIntents,
    Repositories,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportOperationAuthorization {
    pub plugin_id: String,
    pub domain: TransportAuthorizationDomain,
}

#[derive(Clone, Default)]
pub struct TransportOperationRegistry {
    state: Arc<Mutex<RegistryState>>,
}

impl TransportOperationRegistry {
    pub fn reserve(
        &self,
        operation_id: impl Into<String>,
        plugin_id: impl Into<String>,
        domain: TransportAuthorizationDomain,
    ) -> Result<TransportOperationGuard, AppError> {
        let operation_id = operation_id.into();
        let plugin_id = plugin_id.into();
        if operation_id.is_empty() || plugin_id.is_empty() {
            return Err(AppError::InvalidInput(
                "transport operation identity is empty".to_owned(),
            ));
        }

        let mut state = self.state.lock();
        if state.operations.contains_key(&operation_id) {
            return Err(AppError::Transport(TransportFailure::repository_busy()));
        }

        let cancellation = Arc::new(AtomicBool::new(false));
        state.operations.insert(
            operation_id.clone(),
            OperationEntry {
                resource_key: None,
                plugin_id,
                domain,
                cancellation: cancellation.clone(),
            },
        );
        Ok(TransportOperationGuard {
            state: Arc::downgrade(&self.state),
            operation_id,
            resource_key: None,
            cancellation,
            active: true,
        })
    }

    pub fn register(
        &self,
        operation_id: impl Into<String>,
        resource_key: impl Into<String>,
    ) -> Result<TransportOperationGuard, AppError> {
        let operation_id = operation_id.into();
        let resource_key = resource_key.into();
        let mut guard = self.reserve(
            operation_id,
            "git-transport.internal",
            TransportAuthorizationDomain::Repositories,
        )?;
        guard.bind_resource(resource_key)?;
        Ok(guard)
    }

    pub fn authorization(&self, operation_id: &str) -> Option<TransportOperationAuthorization> {
        let state = self.state.lock();
        state
            .operations
            .get(operation_id)
            .map(|entry| TransportOperationAuthorization {
                plugin_id: entry.plugin_id.clone(),
                domain: entry.domain,
            })
    }

    pub fn cancel_owned(
        &self,
        operation_id: &str,
        plugin_id: &str,
        domain: TransportAuthorizationDomain,
    ) -> Result<bool, AppError> {
        let state = self.state.lock();
        let Some(entry) = state.operations.get(operation_id) else {
            return Ok(false);
        };
        if entry.plugin_id != plugin_id || entry.domain != domain {
            return Err(AppError::PermissionDenied);
        }
        entry.cancellation.store(true, Ordering::Release);
        Ok(true)
    }

    pub fn cancel(&self, operation_id: &str) -> bool {
        let state = self.state.lock();
        let Some(entry) = state.operations.get(operation_id) else {
            return false;
        };
        entry.cancellation.store(true, Ordering::Release);
        true
    }
}

pub struct TransportOperationGuard {
    state: Weak<Mutex<RegistryState>>,
    operation_id: String,
    resource_key: Option<String>,
    cancellation: Arc<AtomicBool>,
    active: bool,
}

impl TransportOperationGuard {
    pub fn operation_id(&self) -> &str {
        &self.operation_id
    }

    pub fn cancellation(&self) -> Arc<AtomicBool> {
        self.cancellation.clone()
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancellation.load(Ordering::Acquire)
    }

    pub fn bind_resource(&mut self, resource_key: impl Into<String>) -> Result<(), AppError> {
        let resource_key = resource_key.into();
        if resource_key.is_empty() {
            return Err(AppError::InvalidInput(
                "transport operation resource is empty".to_owned(),
            ));
        }
        if !self.active {
            return Err(AppError::Canceled);
        }

        let Some(state) = self.state.upgrade() else {
            self.active = false;
            return Err(AppError::Canceled);
        };
        let mut state = state.lock();
        let Some(entry) = state.operations.get(&self.operation_id) else {
            self.active = false;
            return Err(AppError::Canceled);
        };
        if !Arc::ptr_eq(&entry.cancellation, &self.cancellation) {
            self.active = false;
            return Err(AppError::Canceled);
        }
        if self.cancellation.load(Ordering::Acquire) {
            return Err(AppError::Canceled);
        }
        if let Some(bound_resource) = &entry.resource_key {
            return if bound_resource == &resource_key {
                Ok(())
            } else {
                Err(AppError::InvalidInput(
                    "transport operation is already bound to another resource".to_owned(),
                ))
            };
        }
        if state.resources.contains_key(&resource_key) {
            return Err(AppError::Transport(TransportFailure::repository_busy()));
        }

        state
            .resources
            .insert(resource_key.clone(), self.operation_id.clone());
        state
            .operations
            .get_mut(&self.operation_id)
            .expect("operation exists while registry lock is held")
            .resource_key = Some(resource_key.clone());
        self.resource_key = Some(resource_key);
        Ok(())
    }

    pub fn finish(mut self) {
        self.release();
    }

    pub fn finish_if_not_cancelled(mut self) -> bool {
        if !self.active {
            return false;
        }
        let Some(state) = self.state.upgrade() else {
            self.active = false;
            return !self.cancellation.load(Ordering::Acquire);
        };
        let mut state = state.lock();
        let completed = !self.cancellation.load(Ordering::Acquire);
        remove_registration(&mut state, &self.operation_id, self.resource_key.as_deref());
        self.active = false;
        completed
    }

    fn release(&mut self) {
        if !self.active {
            return;
        }
        self.active = false;
        let Some(state) = self.state.upgrade() else {
            return;
        };
        let mut state = state.lock();
        remove_registration(&mut state, &self.operation_id, self.resource_key.as_deref());
    }
}

fn remove_registration(state: &mut RegistryState, operation_id: &str, resource_key: Option<&str>) {
    let owns_operation = state
        .operations
        .get(operation_id)
        .is_some_and(|entry| entry.resource_key.as_deref() == resource_key);
    if owns_operation {
        state.operations.remove(operation_id);
    }
    if let Some(resource_key) = resource_key {
        let owns_resource = state
            .resources
            .get(resource_key)
            .is_some_and(|owner| owner == operation_id);
        if owns_resource {
            state.resources.remove(resource_key);
        }
    }
}

impl Drop for TransportOperationGuard {
    fn drop(&mut self) {
        self.release();
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::Ordering;

    use super::{TransportAuthorizationDomain, TransportOperationRegistry};
    use crate::error::AppError;

    #[test]
    fn registry_rejects_duplicate_ids_and_resources_and_cancels_only_the_owner() {
        let registry = TransportOperationRegistry::default();
        let first = registry
            .register("operation-one", "repository-one")
            .unwrap();
        assert!(
            registry
                .register("operation-one", "repository-two")
                .is_err()
        );
        assert!(
            registry
                .register("operation-two", "repository-one")
                .is_err()
        );
        let second = registry
            .register("operation-two", "repository-two")
            .unwrap();

        assert!(registry.cancel("operation-one"));
        assert!(first.cancellation().load(Ordering::Acquire));
        assert!(!second.cancellation().load(Ordering::Acquire));
        assert!(!registry.cancel("missing-operation"));
    }

    #[test]
    fn finishing_or_dropping_a_registration_releases_both_indexes() {
        let registry = TransportOperationRegistry::default();
        let first = registry.register("operation", "repository").unwrap();
        first.finish();
        let replacement = registry.register("operation", "repository").unwrap();
        drop(replacement);
        assert!(registry.register("operation", "repository").is_ok());
    }

    #[test]
    fn completion_and_cancellation_have_one_atomic_winner() {
        let registry = TransportOperationRegistry::default();
        let completed = registry.register("completed", "repository-one").unwrap();
        assert!(completed.finish_if_not_cancelled());
        assert!(!registry.cancel("completed"));

        let canceled = registry.register("canceled", "repository-two").unwrap();
        assert!(registry.cancel("canceled"));
        assert!(!canceled.finish_if_not_cancelled());
        assert!(registry.register("canceled", "repository-two").is_ok());
    }

    #[test]
    fn reserved_operations_bind_owner_domain_and_honor_early_cancellation() {
        let registry = TransportOperationRegistry::default();
        let mut guard = registry
            .reserve(
                "operation",
                "plugin.owner",
                TransportAuthorizationDomain::CloneIntents,
            )
            .unwrap();
        let authorization = registry.authorization("operation").unwrap();
        assert_eq!(authorization.plugin_id, "plugin.owner");
        assert_eq!(
            authorization.domain,
            TransportAuthorizationDomain::CloneIntents
        );
        assert!(matches!(
            registry.cancel_owned(
                "operation",
                "plugin.other",
                TransportAuthorizationDomain::CloneIntents,
            ),
            Err(AppError::PermissionDenied)
        ));
        assert!(
            registry
                .cancel_owned(
                    "operation",
                    "plugin.owner",
                    TransportAuthorizationDomain::CloneIntents,
                )
                .unwrap()
        );
        assert!(matches!(
            guard.bind_resource("clone:/destination"),
            Err(AppError::Canceled)
        ));
        drop(guard);
        assert!(registry.authorization("operation").is_none());
    }

    #[test]
    fn separately_reserved_operations_cannot_bind_the_same_resource() {
        let registry = TransportOperationRegistry::default();
        let mut first = registry
            .reserve(
                "operation-one",
                "plugin.one",
                TransportAuthorizationDomain::Repositories,
            )
            .unwrap();
        let mut second = registry
            .reserve(
                "operation-two",
                "plugin.two",
                TransportAuthorizationDomain::Repositories,
            )
            .unwrap();
        first.bind_resource("repository:one").unwrap();
        assert!(second.bind_resource("repository:one").is_err());
    }
}
