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
    resource_key: String,
    cancellation: Arc<AtomicBool>,
}

#[derive(Clone, Default)]
pub struct TransportOperationRegistry {
    state: Arc<Mutex<RegistryState>>,
}

impl TransportOperationRegistry {
    pub fn register(
        &self,
        operation_id: impl Into<String>,
        resource_key: impl Into<String>,
    ) -> Result<TransportOperationGuard, AppError> {
        let operation_id = operation_id.into();
        let resource_key = resource_key.into();
        if operation_id.is_empty() || resource_key.is_empty() {
            return Err(AppError::InvalidInput(
                "transport operation identity is empty".to_owned(),
            ));
        }
        let mut state = self.state.lock();
        if state.operations.contains_key(&operation_id)
            || state.resources.contains_key(&resource_key)
        {
            return Err(AppError::Transport(TransportFailure::repository_busy()));
        }
        let cancellation = Arc::new(AtomicBool::new(false));
        state
            .resources
            .insert(resource_key.clone(), operation_id.clone());
        state.operations.insert(
            operation_id.clone(),
            OperationEntry {
                resource_key: resource_key.clone(),
                cancellation: cancellation.clone(),
            },
        );
        Ok(TransportOperationGuard {
            state: Arc::downgrade(&self.state),
            operation_id,
            resource_key,
            cancellation,
            active: true,
        })
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
    resource_key: String,
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
        remove_registration(&mut state, &self.operation_id, &self.resource_key);
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
        remove_registration(&mut state, &self.operation_id, &self.resource_key);
    }
}

fn remove_registration(state: &mut RegistryState, operation_id: &str, resource_key: &str) {
    let owns_operation = state
        .operations
        .get(operation_id)
        .is_some_and(|entry| entry.resource_key == resource_key);
    if owns_operation {
        state.operations.remove(operation_id);
    }
    let owns_resource = state
        .resources
        .get(resource_key)
        .is_some_and(|owner| owner == operation_id);
    if owns_resource {
        state.resources.remove(resource_key);
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

    use super::TransportOperationRegistry;

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
}
