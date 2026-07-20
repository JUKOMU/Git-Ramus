use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::config::{
    config_plan, ensure_restorable_snapshot, plan_snapshot, read_managed_snapshot,
    write_managed_snapshot,
};
use super::model::{
    EffectiveTransport, ProfileDeletionImpact, RepositoryTransportBinding,
    RepositoryTransportBindingSummary, TransportConfigRepair, TransportConfigSnapshot,
    TransportDriftStatus, TransportKind, TransportProfile, TransportProfileSummary,
};
use super::store::TransportStore;
use super::url::{ValidatedRemoteUrl, validate_clone_url};
use crate::db::Database;
use crate::error::{AppError, TransportFailure};
use crate::git::GitRunner;
use crate::git::model::Repository;
use crate::git::repository::{RepositoryRepository, RepositoryWriteLocks, TrustRepository};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DriftResolution {
    Reject,
    KeepExternal,
    Reapply,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "action",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum ProfileDeletionResolution {
    Replace {
        repository_id: String,
        replacement_profile_id: String,
    },
    Unbind {
        repository_id: String,
        drift_resolution: DriftResolution,
    },
}

impl ProfileDeletionResolution {
    fn repository_id(&self) -> &str {
        match self {
            Self::Replace { repository_id, .. } | Self::Unbind { repository_id, .. } => {
                repository_id
            }
        }
    }
}

#[derive(Clone)]
pub struct TransportProfileService {
    store: TransportStore,
    repositories: RepositoryRepository,
    trusts: TrustRepository,
    write_locks: RepositoryWriteLocks,
    lifecycle_lock: Arc<std::sync::Mutex<()>>,
    runner: Arc<dyn GitRunner>,
}

impl TransportProfileService {
    pub fn new(
        database: Database,
        write_locks: RepositoryWriteLocks,
        runner: Arc<dyn GitRunner>,
    ) -> Self {
        Self {
            store: TransportStore::new(database.clone()),
            repositories: RepositoryRepository::new(database.clone()),
            trusts: TrustRepository::new(database),
            write_locks,
            lifecycle_lock: Arc::new(std::sync::Mutex::new(())),
            runner,
        }
    }

    pub fn list_profiles(&self) -> Result<Vec<TransportProfileSummary>, AppError> {
        let _lifecycle_guard = self.lock_lifecycle()?;
        self.store.list_profile_summaries()
    }

    pub fn create_ssh_profile(
        &self,
        display_name: &str,
        key_path: &Path,
        identities_only: bool,
    ) -> Result<TransportProfileSummary, AppError> {
        let _lifecycle_guard = self.lock_lifecycle()?;
        let display_name = validated_display_name(display_name)?;
        let key_path = canonical_key_path(key_path)?;
        let profile = TransportProfile::new_ssh(display_name, key_path, identities_only);
        self.store.insert_profile(&profile)?;
        Ok(profile.summary(true, 0))
    }

    pub fn create_https_profile(
        &self,
        display_name: &str,
        username: &str,
    ) -> Result<TransportProfileSummary, AppError> {
        let _lifecycle_guard = self.lock_lifecycle()?;
        let profile = TransportProfile::new_https(
            validated_display_name(display_name)?,
            validated_username(username)?,
        );
        self.store.insert_profile(&profile)?;
        Ok(profile.summary(true, 0))
    }

    pub fn update_ssh_profile(
        &self,
        profile_id: &str,
        display_name: &str,
        selected_key_path: Option<&Path>,
        identities_only: bool,
    ) -> Result<TransportProfileSummary, AppError> {
        let _lifecycle_guard = self.lock_lifecycle()?;
        let mut profile = self.required_profile(profile_id)?;
        if profile.kind != TransportKind::Ssh {
            return Err(invalid_profile());
        }
        let key_path = match selected_key_path {
            Some(path) => canonical_key_path(path)?,
            None => canonical_key_path(Path::new(
                profile
                    .ssh_key_path
                    .as_deref()
                    .ok_or_else(invalid_profile)?,
            ))?,
        };
        let managed_fields_changed = profile.ssh_key_path.as_deref() != Some(key_path.as_str())
            || profile.ssh_identities_only != Some(identities_only);
        if managed_fields_changed && !self.store.list_bindings_for_profile(profile_id)?.is_empty() {
            return Err(AppError::UserActionRequired(
                "cleanly unbind all repositories before changing managed transport fields"
                    .to_owned(),
            ));
        }
        profile.display_name = validated_display_name(display_name)?;
        profile.ssh_key_path = Some(key_path);
        profile.ssh_variant = Some("ssh".to_owned());
        profile.ssh_identities_only = Some(identities_only);
        profile.https_username = None;
        profile.https_use_http_path = None;
        profile.updated_at = Utc::now();
        self.store.update_profile(&profile)?;
        self.summary_for(&profile)
    }

    pub fn update_https_profile(
        &self,
        profile_id: &str,
        display_name: &str,
        username: &str,
    ) -> Result<TransportProfileSummary, AppError> {
        let _lifecycle_guard = self.lock_lifecycle()?;
        let mut profile = self.required_profile(profile_id)?;
        if profile.kind != TransportKind::Https {
            return Err(invalid_profile());
        }
        let username = validated_username(username)?;
        if profile.https_username.as_deref() != Some(username.as_str())
            && !self.store.list_bindings_for_profile(profile_id)?.is_empty()
        {
            return Err(AppError::UserActionRequired(
                "cleanly unbind all repositories before changing managed transport fields"
                    .to_owned(),
            ));
        }
        profile.display_name = validated_display_name(display_name)?;
        profile.ssh_key_path = None;
        profile.ssh_variant = None;
        profile.ssh_identities_only = None;
        profile.https_username = Some(username);
        profile.https_use_http_path = Some(true);
        profile.updated_at = Utc::now();
        self.store.update_profile(&profile)?;
        self.summary_for(&profile)
    }

    pub fn profile_deletion_impact(
        &self,
        profile_id: &str,
    ) -> Result<ProfileDeletionImpact, AppError> {
        let _lifecycle_guard = self.lock_lifecycle()?;
        self.store.profile_deletion_impact(profile_id)
    }

    pub fn delete_profile(
        &self,
        profile_id: &str,
        resolutions: &[ProfileDeletionResolution],
    ) -> Result<(), AppError> {
        let _lifecycle_guard = self.lock_lifecycle()?;
        let profile = self.required_profile(profile_id)?;
        let impact = self.store.profile_deletion_impact(profile_id)?;
        let expected = impact
            .repository_ids
            .iter()
            .cloned()
            .collect::<BTreeSet<_>>();
        let supplied = resolutions
            .iter()
            .map(|resolution| resolution.repository_id().to_owned())
            .collect::<BTreeSet<_>>();
        if supplied.len() != resolutions.len() || supplied != expected {
            return Err(AppError::InvalidInput(
                "profile deletion resolutions must cover every affected repository exactly once"
                    .to_owned(),
            ));
        }

        let resolutions = resolutions
            .iter()
            .map(|resolution| (resolution.repository_id().to_owned(), resolution))
            .collect::<BTreeMap<_, _>>();

        // Validate every requested transition before touching any repository. This keeps a bad
        // replacement on the last row from leaving earlier rows partially resolved.
        for repository_id in &impact.repository_ids {
            let repository = self.repositories.get(repository_id)?;
            if !self.trusts.is_trusted(repository_id)? {
                return Err(AppError::TrustRequired);
            }
            let resolution = resolutions
                .get(repository_id)
                .expect("validated deletion resolution");
            match resolution {
                ProfileDeletionResolution::Replace {
                    replacement_profile_id,
                    ..
                } => {
                    if replacement_profile_id == profile_id {
                        return Err(AppError::InvalidInput(
                            "replacement profile must differ from the deleted profile".to_owned(),
                        ));
                    }
                    let replacement = self.required_profile(replacement_profile_id)?;
                    let remote = self.primary_remote(&repository)?;
                    if replacement.kind != profile.kind || replacement.kind != remote.kind {
                        return Err(AppError::Transport(TransportFailure::profile_mismatch()));
                    }
                    config_plan(&replacement, &remote)?;
                }
                ProfileDeletionResolution::Unbind {
                    drift_resolution, ..
                } => {
                    if *drift_resolution == DriftResolution::Reapply {
                        return Err(AppError::InvalidInput(
                            "profile deletion cannot reapply the profile being deleted".to_owned(),
                        ));
                    }
                }
            }

            let lock = self.write_locks.lock_for(repository_id);
            let _guard = lock
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let binding = self.store.get_binding(repository_id)?.ok_or_else(|| {
                AppError::NotFound(format!("repository transport binding {repository_id}"))
            })?;
            if binding.transport_profile_id != profile_id {
                return Err(AppError::InvalidInput(
                    "profile deletion impact changed; refresh and retry".to_owned(),
                ));
            }
            let live = read_managed_snapshot(self.runner.as_ref(), &repository_path(&repository)?)?;
            let rejects_drift = matches!(resolution, ProfileDeletionResolution::Replace { .. })
                || matches!(
                    resolution,
                    ProfileDeletionResolution::Unbind {
                        drift_resolution: DriftResolution::Reject,
                        ..
                    }
                );
            if rejects_drift && live.sha256() != binding.applied_config_hash {
                self.store.mark_binding_drifted(repository_id, Utc::now())?;
                return Err(config_drift(repository_id));
            }
        }

        for repository_id in &impact.repository_ids {
            match resolutions
                .get(repository_id)
                .expect("validated deletion resolution")
            {
                ProfileDeletionResolution::Replace {
                    replacement_profile_id,
                    ..
                } => {
                    self.bind_repository_without_lifecycle_lock(
                        repository_id,
                        replacement_profile_id,
                        true,
                    )?;
                }
                ProfileDeletionResolution::Unbind {
                    drift_resolution, ..
                } => {
                    self.unbind_repository_without_lifecycle_lock(
                        repository_id,
                        *drift_resolution,
                    )?;
                }
            }
        }

        self.store
            .database()
            .with_immediate_transaction(|transaction| {
                let remaining: i64 = transaction.query_row(
                    "SELECT COUNT(*) FROM repository_transport_bindings WHERE transport_profile_id=?1",
                    [profile_id],
                    |row| row.get(0),
                )?;
                if remaining != 0 {
                    return Err(AppError::InvalidInput(
                        "transport profile still has repository bindings".to_owned(),
                    ));
                }
                let changed = transaction.execute(
                    "DELETE FROM transport_profiles WHERE id=?1",
                    [profile_id],
                )?;
                if changed != 1 {
                    return Err(AppError::NotFound(format!(
                        "transport profile {profile_id}"
                    )));
                }
                Ok(())
            })
    }

    pub fn bind_repository(
        &self,
        repository_id: &str,
        profile_id: &str,
        replace_existing: bool,
    ) -> Result<RepositoryTransportBindingSummary, AppError> {
        let _lifecycle_guard = self.lock_lifecycle()?;
        self.bind_repository_without_lifecycle_lock(repository_id, profile_id, replace_existing)
    }

    fn bind_repository_without_lifecycle_lock(
        &self,
        repository_id: &str,
        profile_id: &str,
        replace_existing: bool,
    ) -> Result<RepositoryTransportBindingSummary, AppError> {
        let repository = self.repositories.get(repository_id)?;
        if !self.trusts.is_trusted(repository_id)? {
            return Err(AppError::TrustRequired);
        }
        let lock = self.write_locks.lock_for(repository_id);
        let _guard = lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.bind_repository_locked(&repository, profile_id, replace_existing)
            .map(|binding| binding.summary())
    }

    pub fn unbind_repository(
        &self,
        repository_id: &str,
        resolution: DriftResolution,
    ) -> Result<(), AppError> {
        let _lifecycle_guard = self.lock_lifecycle()?;
        self.unbind_repository_without_lifecycle_lock(repository_id, resolution)
    }

    fn unbind_repository_without_lifecycle_lock(
        &self,
        repository_id: &str,
        resolution: DriftResolution,
    ) -> Result<(), AppError> {
        let repository = self.repositories.get(repository_id)?;
        if !self.trusts.is_trusted(repository_id)? {
            return Err(AppError::TrustRequired);
        }
        let lock = self.write_locks.lock_for(repository_id);
        let _guard = lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let binding = self.store.get_binding(repository_id)?.ok_or_else(|| {
            AppError::NotFound(format!("repository transport binding {repository_id}"))
        })?;
        let repository_path = repository_path(&repository)?;
        let live = read_managed_snapshot(self.runner.as_ref(), &repository_path)?;
        if live.sha256() != binding.applied_config_hash {
            self.store.mark_binding_drifted(repository_id, Utc::now())?;
            return match resolution {
                DriftResolution::Reject => Err(config_drift(repository_id)),
                DriftResolution::KeepExternal => self.store.delete_binding(repository_id),
                DriftResolution::Reapply => self.reapply_drifted_locked(&repository, binding, live),
            };
        }

        self.write_with_compensation(&repository, &live, &binding.before_config, "restoreConfig")?;
        if let Err(error) = self.store.delete_binding(repository_id) {
            return Err(self.compensate_or_partial(
                &repository,
                &binding.before_config,
                &live,
                error,
                "deleteBinding",
            ));
        }
        Ok(())
    }

    pub fn effective_for_repository(
        &self,
        repository_id: &str,
    ) -> Result<EffectiveTransport, AppError> {
        let _lifecycle_guard = self.lock_lifecycle()?;
        let repository = self.repositories.get(repository_id)?;
        let lock = self.write_locks.lock_for(repository_id);
        let _guard = match lock.try_lock() {
            Ok(guard) => guard,
            Err(std::sync::TryLockError::WouldBlock) => {
                return Err(AppError::Transport(TransportFailure::repository_busy()));
            }
            Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
        };
        let Some(binding) = self.store.get_binding(repository_id)? else {
            return Ok(EffectiveTransport::system_git(repository_id));
        };
        let profile = self.required_profile(&binding.transport_profile_id)?;
        let live = read_managed_snapshot(self.runner.as_ref(), &repository_path(&repository)?)?;
        let drift_status = if live.sha256() == binding.applied_config_hash {
            TransportDriftStatus::Clean
        } else {
            if binding.drift_status != TransportDriftStatus::Drifted {
                self.store.mark_binding_drifted(repository_id, Utc::now())?;
            }
            TransportDriftStatus::Drifted
        };
        Ok(EffectiveTransport::profile(
            repository_id,
            self.summary_for(&profile)?,
            drift_status,
        ))
    }

    pub(crate) fn required_profile(&self, profile_id: &str) -> Result<TransportProfile, AppError> {
        self.store
            .get_profile(profile_id)?
            .ok_or_else(|| AppError::NotFound(format!("transport profile {profile_id}")))
    }

    fn bind_repository_locked(
        &self,
        repository: &Repository,
        profile_id: &str,
        replace_existing: bool,
    ) -> Result<RepositoryTransportBinding, AppError> {
        if self
            .store
            .repository_has_unresolved_repair(&repository.id)?
        {
            return Err(AppError::Transport(
                TransportFailure::partial()
                    .with_resource(&repository.id)
                    .with_failed_step("repairConfig"),
            ));
        }
        let profile = self.required_profile(profile_id)?;
        let remote = self.primary_remote(repository)?;
        if profile.kind != remote.kind {
            return Err(AppError::Transport(
                TransportFailure::profile_mismatch().with_resource(&repository.id),
            ));
        }
        let desired = plan_snapshot(&config_plan(&profile, &remote)?)?;
        let repository_path = repository_path(repository)?;
        let existing = self.store.get_binding(&repository.id)?;
        let now = Utc::now();

        let (before_config, applied_config, current_config, bound_at) = if let Some(existing) =
            existing
        {
            let current = read_managed_snapshot(self.runner.as_ref(), &repository_path)?;
            if current.sha256() != existing.applied_config_hash {
                self.store.mark_binding_drifted(&repository.id, now)?;
                return Err(config_drift(&repository.id));
            }
            ensure_restorable_snapshot(&current)?;
            (existing.before_config, desired, current, existing.bound_at)
        } else {
            let current = read_managed_snapshot(self.runner.as_ref(), &repository_path)?;
            ensure_restorable_snapshot(&current)?;
            if !current.values.is_empty() && current != desired && !replace_existing {
                return Err(AppError::UserActionRequired(
                    "confirm replacement of existing repository transport configuration".to_owned(),
                ));
            }
            (current.clone(), desired, current, now)
        };
        ensure_restorable_snapshot(&applied_config)?;

        self.write_with_compensation(repository, &current_config, &applied_config, "applyConfig")?;
        let binding = RepositoryTransportBinding {
            repository_id: repository.id.clone(),
            transport_profile_id: profile.id,
            before_config,
            applied_config_hash: applied_config.sha256(),
            applied_config,
            drift_status: TransportDriftStatus::Clean,
            bound_at,
            updated_at: now,
        };
        if let Err(error) = self.store.upsert_binding(&binding) {
            return Err(self.compensate_or_partial(
                repository,
                &binding.applied_config,
                &current_config,
                error,
                "saveBinding",
            ));
        }
        Ok(binding)
    }

    fn reapply_drifted_locked(
        &self,
        repository: &Repository,
        mut binding: RepositoryTransportBinding,
        live: TransportConfigSnapshot,
    ) -> Result<(), AppError> {
        ensure_restorable_snapshot(&live)?;
        self.write_with_compensation(repository, &live, &binding.applied_config, "reapplyConfig")?;
        binding.drift_status = TransportDriftStatus::Clean;
        binding.updated_at = Utc::now();
        if let Err(error) = self.store.upsert_binding(&binding) {
            return Err(self.compensate_or_partial(
                repository,
                &binding.applied_config,
                &live,
                error,
                "saveBinding",
            ));
        }
        Ok(())
    }

    fn write_with_compensation(
        &self,
        repository: &Repository,
        original: &TransportConfigSnapshot,
        target: &TransportConfigSnapshot,
        failed_step: &str,
    ) -> Result<(), AppError> {
        let path = repository_path(repository)?;
        if let Err(error) = write_managed_snapshot(self.runner.as_ref(), &path, target) {
            return Err(self.compensate_or_partial(
                repository,
                target,
                original,
                error,
                failed_step,
            ));
        }
        Ok(())
    }

    fn compensate_or_partial(
        &self,
        repository: &Repository,
        attempted: &TransportConfigSnapshot,
        restore: &TransportConfigSnapshot,
        original_error: AppError,
        failed_step: &str,
    ) -> AppError {
        let path = match repository_path(repository) {
            Ok(path) => path,
            Err(_) => return partial_failure(&repository.id, failed_step),
        };
        if write_managed_snapshot(self.runner.as_ref(), &path, restore).is_ok() {
            return original_error;
        }
        let repair = TransportConfigRepair {
            id: Uuid::new_v4().to_string(),
            repository_id: repository.id.clone(),
            before_config: restore.clone(),
            attempted_config: attempted.clone(),
            error_code: stable_error_code(&original_error).to_owned(),
            created_at: Utc::now(),
            resolved_at: None,
        };
        let _ = self.store.insert_repair(&repair);
        partial_failure(&repository.id, failed_step)
    }

    fn primary_remote(&self, repository: &Repository) -> Result<ValidatedRemoteUrl, AppError> {
        let remotes = self.repositories.list_remotes(&repository.id)?;
        let remote = remotes
            .iter()
            .find(|remote| remote.name == "origin")
            .or_else(|| remotes.first())
            .ok_or_else(|| {
                AppError::NotFound(format!("remote for repository {}", repository.id))
            })?;
        let url = remote
            .fetch_url
            .as_deref()
            .or(remote.push_url.as_deref())
            .ok_or_else(|| {
                AppError::NotFound(format!("remote URL for repository {}", repository.id))
            })?;
        validate_clone_url(url)
    }

    fn summary_for(&self, profile: &TransportProfile) -> Result<TransportProfileSummary, AppError> {
        let available = match profile.kind {
            TransportKind::Ssh => profile
                .ssh_key_path
                .as_deref()
                .is_some_and(|path| Path::new(path).is_file()),
            TransportKind::Https => true,
        };
        let count = self.store.list_bindings_for_profile(&profile.id)?.len();
        let count = i64::try_from(count).map_err(|_| AppError::OutputLimit)?;
        Ok(profile.summary(available, count))
    }

    fn lock_lifecycle(&self) -> Result<std::sync::MutexGuard<'_, ()>, AppError> {
        self.lifecycle_lock
            .lock()
            .map_err(|_| AppError::Git("transport profile lifecycle lock failed".to_owned()))
    }
}

fn repository_path(repository: &Repository) -> Result<PathBuf, AppError> {
    let path = PathBuf::from(&repository.canonical_path);
    if !path.is_absolute() {
        return Err(AppError::InvalidInput(
            "repository path is not absolute".to_owned(),
        ));
    }
    Ok(path)
}

fn validated_display_name(value: &str) -> Result<String, AppError> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 128 || value.chars().any(char::is_control) {
        return Err(invalid_profile());
    }
    Ok(value.to_owned())
}

fn validated_username(value: &str) -> Result<String, AppError> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 256 || value.chars().any(char::is_control) {
        return Err(invalid_profile());
    }
    Ok(value.to_owned())
}

fn canonical_key_path(path: &Path) -> Result<String, AppError> {
    if !path.is_absolute() || !path.is_file() {
        return Err(invalid_profile());
    }
    let path = std::fs::canonicalize(path).map_err(|_| invalid_profile())?;
    let value = path.to_str().ok_or(AppError::NonUtf8Path)?;
    if value.is_empty() || value.chars().any(char::is_control) {
        return Err(invalid_profile());
    }
    Ok(value.to_owned())
}

fn invalid_profile() -> AppError {
    AppError::InvalidInput("Git transport profile is invalid".to_owned())
}

fn config_drift(repository_id: &str) -> AppError {
    AppError::Transport(
        TransportFailure::config_drift()
            .with_resource(repository_id)
            .with_failed_step("restoreConfig"),
    )
}

fn partial_failure(repository_id: &str, failed_step: &str) -> AppError {
    AppError::Transport(
        TransportFailure::partial()
            .with_resource(repository_id)
            .with_failed_step(failed_step),
    )
}

fn stable_error_code(error: &AppError) -> &'static str {
    match error {
        AppError::Database(_) => "storage.database",
        AppError::Io(_) => "storage.io",
        AppError::Git(_) => "git.failed",
        AppError::Timeout => "git.timeout",
        AppError::OutputLimit => "git.output-limit",
        AppError::Transport(failure) => failure.code(),
        _ => "git.transport.partial",
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::git::SystemGitRunner;

    #[test]
    fn profile_crud_trims_fields_and_exposes_only_the_ssh_filename() {
        let database = Database::open_in_memory().unwrap();
        let service = TransportProfileService::new(
            database,
            RepositoryWriteLocks::default(),
            Arc::new(SystemGitRunner::new()),
        );
        let directory = tempfile::tempdir().unwrap();
        let key = directory.path().join("id work");
        std::fs::write(&key, "fixture key").unwrap();

        let ssh = service
            .create_ssh_profile(" Work SSH ", &key, true)
            .unwrap();
        assert_eq!(ssh.display_name, "Work SSH");
        assert_eq!(ssh.ssh_key_file_name.as_deref(), Some("id work"));
        let serialized = serde_json::to_string(&ssh).unwrap();
        assert!(!serialized.contains(directory.path().to_string_lossy().as_ref()));

        let https = service.create_https_profile(" Web ", " creator ").unwrap();
        assert_eq!(https.display_name, "Web");
        assert_eq!(https.https_username.as_deref(), Some("creator"));
        assert!(service.create_https_profile("\n", "creator").is_err());
        assert!(
            service
                .create_ssh_profile("Bad", Path::new("relative"), true)
                .is_err()
        );

        service.delete_profile(&ssh.id, &[]).unwrap();
        assert_eq!(service.list_profiles().unwrap().len(), 1);
    }

    #[test]
    fn deletion_resolutions_must_be_unique_and_complete() {
        let first = ProfileDeletionResolution::Unbind {
            repository_id: "repository".to_owned(),
            drift_resolution: DriftResolution::Reject,
        };
        let second = ProfileDeletionResolution::Replace {
            repository_id: "repository".to_owned(),
            replacement_profile_id: "replacement".to_owned(),
        };
        let supplied = [&first, &second]
            .into_iter()
            .map(|resolution| resolution.repository_id())
            .collect::<BTreeSet<_>>();
        assert_eq!(supplied.len(), 1);
    }
}
