use std::collections::HashMap;
use std::ffi::OsString;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use futures_util::future::BoxFuture;
use parking_lot::Mutex;

use super::config::config_plan;
use super::model::{
    CloneInput, CloneIntent, CloneOperation, CloneProjectSummary, CloneProjectTarget, CloneResult,
    CloneResultStatus, CloneSource, CloneStage, NetworkProgress, NetworkStage, TransportKind,
};
use super::operation::TransportOperationRegistry;
use super::profile_service::TransportProfileService;
use super::service::{
    NetworkProgressReporter, ProgressBridge, classify_execution_error, classify_git_failure,
    terminal_progress,
};
use super::store::TransportStore;
use super::url::validate_clone_url;
use crate::db::Database;
use crate::error::{AppError, TransportFailure};
use crate::git::engine::{GitCommand, GitExecutionPolicy, GitRunContext, GitRunner};
use crate::git::model::{Project, Repository, RepositoryKind};
use crate::git::parser::detect_repository;
use crate::git::service::{GitService, ProjectCreateInput};
use crate::jobs::JobService;
use crate::providers::model::RemoteRepository;
use crate::providers::service::ProviderService;

const STAGING_PREFIX: &str = ".git-ramus-clone-";
const MARKER_SUFFIX: &str = ".owner";
const MAX_MARKER_BYTES: u64 = 1024;
const CLONE_INTENT_TTL: ChronoDuration = ChronoDuration::minutes(10);
const MAX_ACTIVE_CLONE_INTENTS: usize = 1_024;
pub const GIT_CLIENT_PLUGIN_ID: &str = "git-ramus.git-client";
const CLONE_TIMEOUT: Duration = Duration::from_secs(30 * 60);
const LOCAL_GIT_TIMEOUT: Duration = Duration::from_secs(30);

pub trait CloneClock: Send + Sync {
    fn now(&self) -> DateTime<Utc>;
}

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemCloneClock;

impl CloneClock for SystemCloneClock {
    fn now(&self) -> DateTime<Utc> {
        Utc::now()
    }
}

#[derive(Clone)]
struct CloneIntentRecord {
    intent: CloneIntent,
    creator_plugin_id: String,
    account_id: String,
    consumed: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConsumedCloneIntent {
    pub intent_id: String,
    pub creator_plugin_id: String,
    pub account_id: String,
    pub repository: RemoteRepository,
}

#[derive(Clone)]
pub struct CloneIntentRegistry {
    records: Arc<Mutex<HashMap<String, CloneIntentRecord>>>,
    clock: Arc<dyn CloneClock>,
}

impl Default for CloneIntentRegistry {
    fn default() -> Self {
        Self::with_clock(Arc::new(SystemCloneClock))
    }
}

impl CloneIntentRegistry {
    pub fn with_clock(clock: Arc<dyn CloneClock>) -> Self {
        Self {
            records: Arc::new(Mutex::new(HashMap::new())),
            clock,
        }
    }

    fn insert_verified(
        &self,
        creator_plugin_id: &str,
        account_id: &str,
        repository: RemoteRepository,
    ) -> Result<CloneIntent, AppError> {
        validate_intent_identity(creator_plugin_id, "Clone intent creator")?;
        validate_intent_identity(account_id, "Provider account")?;
        let https = validate_clone_url(&repository.https_url)?;
        let ssh = validate_clone_url(&repository.ssh_url)?;
        if https.kind != TransportKind::Https || ssh.kind != TransportKind::Ssh {
            return Err(AppError::InvalidInput(
                "Provider repository transports are invalid".to_owned(),
            ));
        }
        if repository.repository_id.trim().is_empty()
            || repository.repository_id.len() > 256
            || repository.repository_id.chars().any(char::is_control)
        {
            return Err(AppError::InvalidInput(
                "Provider repository id is invalid".to_owned(),
            ));
        }
        let created_at = self.clock.now();
        let intent = CloneIntent {
            id: uuid::Uuid::new_v4().to_string(),
            repository,
            available_transports: vec![TransportKind::Https, TransportKind::Ssh],
            created_at,
            expires_at: created_at + CLONE_INTENT_TTL,
        };
        let mut records = self.records.lock();
        purge_expired_intents(&mut records, created_at);
        if records.len() >= MAX_ACTIVE_CLONE_INTENTS {
            return Err(AppError::InvalidInput(
                "Too many active Clone intents".to_owned(),
            ));
        }
        records.insert(
            intent.id.clone(),
            CloneIntentRecord {
                intent: intent.clone(),
                creator_plugin_id: creator_plugin_id.to_owned(),
                account_id: account_id.to_owned(),
                consumed: false,
            },
        );
        Ok(intent)
    }

    pub fn get(&self, intent_id: &str) -> Result<CloneIntent, AppError> {
        let now = self.clock.now();
        let mut records = self.records.lock();
        purge_expired_intents(&mut records, now);
        records
            .get(intent_id)
            .filter(|record| !record.consumed)
            .map(|record| record.intent.clone())
            .ok_or_else(|| AppError::NotFound(format!("Clone intent {intent_id}")))
    }

    pub fn consume(
        &self,
        intent_id: &str,
        consumer_plugin_id: &str,
    ) -> Result<ConsumedCloneIntent, AppError> {
        if consumer_plugin_id != GIT_CLIENT_PLUGIN_ID {
            return Err(AppError::PermissionDenied);
        }
        let now = self.clock.now();
        let mut records = self.records.lock();
        purge_expired_intents(&mut records, now);
        let Some(record) = records.get_mut(intent_id) else {
            return Err(AppError::NotFound(format!("Clone intent {intent_id}")));
        };
        if record.consumed {
            return Err(AppError::InvalidInput(
                "Clone intent was already consumed".to_owned(),
            ));
        }
        record.consumed = true;
        let record = records
            .remove(intent_id)
            .expect("consumed Clone intent remains under the registry lock");
        Ok(ConsumedCloneIntent {
            intent_id: record.intent.id,
            creator_plugin_id: record.creator_plugin_id,
            account_id: record.account_id,
            repository: record.intent.repository,
        })
    }

    pub fn cancel(&self, intent_id: &str) -> bool {
        let mut records = self.records.lock();
        purge_expired_intents(&mut records, self.clock.now());
        records.remove(intent_id).is_some()
    }
}

pub trait CloneRepositoryResolver: Send + Sync {
    fn repository_for_clone<'a>(
        &'a self,
        account_id: &'a str,
        repository_id: &'a str,
    ) -> BoxFuture<'a, Result<RemoteRepository, AppError>>;
}

impl CloneRepositoryResolver for ProviderService {
    fn repository_for_clone<'a>(
        &'a self,
        account_id: &'a str,
        repository_id: &'a str,
    ) -> BoxFuture<'a, Result<RemoteRepository, AppError>> {
        Box::pin(ProviderService::repository_for_clone(
            self,
            account_id,
            repository_id,
        ))
    }
}

#[derive(Clone)]
pub struct CloneIntentBroker {
    registry: CloneIntentRegistry,
    resolver: Arc<dyn CloneRepositoryResolver>,
}

impl CloneIntentBroker {
    pub fn new(registry: CloneIntentRegistry, resolver: Arc<dyn CloneRepositoryResolver>) -> Self {
        Self { registry, resolver }
    }

    pub fn registry(&self) -> CloneIntentRegistry {
        self.registry.clone()
    }

    pub async fn create(
        &self,
        creator_plugin_id: &str,
        account_id: &str,
        repository_id: &str,
    ) -> Result<CloneIntent, AppError> {
        validate_intent_identity(repository_id, "Provider repository")?;
        let repository = self
            .resolver
            .repository_for_clone(account_id, repository_id)
            .await?;
        if repository.repository_id != repository_id {
            return Err(AppError::Provider(
                crate::error::ProviderFailure::invalid_response(),
            ));
        }
        self.registry
            .insert_verified(creator_plugin_id, account_id, repository)
    }
}

fn purge_expired_intents(records: &mut HashMap<String, CloneIntentRecord>, now: DateTime<Utc>) {
    records.retain(|_, record| !record.consumed && record.intent.expires_at > now);
}

fn validate_intent_identity(value: &str, label: &str) -> Result<(), AppError> {
    if value.trim().is_empty() || value.len() > 256 || value.chars().any(char::is_control) {
        return Err(AppError::InvalidInput(format!("{label} is invalid")));
    }
    Ok(())
}

pub trait CloneProviderBinder: Send + Sync {
    fn bind_clone_remote(
        &self,
        repository_id: &str,
        remote_name: &str,
        intent: &ConsumedCloneIntent,
    ) -> Result<(), AppError>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CloneRecoveryAction {
    CleanupStaging,
    RetryClone,
    RetryRegistration,
    Interrupted,
    UnsafePath,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CloneRecoveryClassification {
    pub operation_id: String,
    pub actions: Vec<CloneRecoveryAction>,
}

impl CloneProviderBinder for ProviderService {
    fn bind_clone_remote(
        &self,
        repository_id: &str,
        remote_name: &str,
        intent: &ConsumedCloneIntent,
    ) -> Result<(), AppError> {
        ProviderService::bind_clone_remote(
            self,
            repository_id,
            remote_name,
            &intent.account_id,
            &intent.repository,
        )
        .map(|_| ())
    }
}

#[derive(Clone)]
pub struct CloneCoordinator {
    git: GitService,
    profiles: TransportProfileService,
    store: TransportStore,
    jobs: JobService,
    operations: TransportOperationRegistry,
    runner: Arc<dyn GitRunner>,
    intents: CloneIntentRegistry,
    provider: Option<Arc<dyn CloneProviderBinder>>,
}

impl CloneCoordinator {
    pub fn new(
        database: Database,
        git: GitService,
        profiles: TransportProfileService,
        jobs: JobService,
        operations: TransportOperationRegistry,
        runner: Arc<dyn GitRunner>,
    ) -> Self {
        Self {
            git,
            profiles,
            store: TransportStore::new(database),
            jobs,
            operations,
            runner,
            intents: CloneIntentRegistry::default(),
            provider: None,
        }
    }

    pub fn intents(&self) -> CloneIntentRegistry {
        self.intents.clone()
    }

    pub fn with_support(
        mut self,
        intents: CloneIntentRegistry,
        provider: Option<Arc<dyn CloneProviderBinder>>,
    ) -> Self {
        self.intents = intents;
        self.provider = provider;
        self
    }

    pub fn clone_repository(
        &self,
        input: CloneInput,
        reporter: Arc<dyn NetworkProgressReporter>,
    ) -> Result<CloneResult, AppError> {
        uuid::Uuid::parse_str(&input.operation_id)
            .map_err(|_| AppError::InvalidInput("Clone operation id must be a UUID".to_owned()))?;
        let paths = ClonePaths::allocate(
            &input.destination_parent,
            &input.folder_name,
            &input.operation_id,
        )?;
        let existing_project = self.validate_project_target(&input.project_target, &paths)?;
        let staging_path = path_string(&paths.staging)?;
        let owner_marker_path = path_string(&paths.marker)?;
        let final_path = path_string(&paths.final_path)?;
        let profile = match input.profile_id.as_deref() {
            Some(profile_id) => {
                let profile = self.profiles.required_profile(profile_id)?;
                if profile.kind != input.transport_kind {
                    return Err(AppError::Transport(TransportFailure::profile_mismatch()));
                }
                Some(profile)
            }
            None => None,
        };
        let resolved = self.resolve_source(&input.source, input.transport_kind)?;
        let profile_plan = profile
            .as_ref()
            .map(|profile| config_plan(profile, &resolved.url))
            .transpose()?;

        let operation_guard = self
            .operations
            .register(&input.operation_id, format!("clone:{final_path}"))?;
        let queued = self.jobs.create_with_id(
            &input.operation_id,
            "git.transport.clone",
            &format!("Clone {}", input.folder_name),
        )?;
        let now = Utc::now();
        let operation = CloneOperation {
            operation_id: input.operation_id.clone(),
            job_id: queued.id.clone(),
            source_summary: resolved.summary.clone(),
            intent_id: resolved
                .intent
                .as_ref()
                .map(|intent| intent.intent_id.clone()),
            transport_profile_id: input.profile_id.clone(),
            provider_instance_id: resolved
                .intent
                .as_ref()
                .map(|intent| intent.repository.instance_id.clone()),
            provider_account_id: resolved
                .intent
                .as_ref()
                .map(|intent| intent.account_id.clone()),
            provider_repository_id: resolved
                .intent
                .as_ref()
                .map(|intent| intent.repository.repository_id.clone()),
            staging_path,
            owner_marker_path,
            final_path,
            project_target: input.project_target.clone(),
            current_stage: CloneStage::Validating,
            filesystem_complete: false,
            repository_id: None,
            project_id: existing_project.as_ref().map(|project| project.id.clone()),
            profile_applied: input.profile_id.is_none(),
            provider_binding_complete: resolved.intent.is_none(),
            created_at: now,
            updated_at: now,
        };
        if let Err(error) = self.store.insert_clone_operation(&operation) {
            let _ = self.jobs.cancel(&queued.id);
            return Err(error);
        }
        let running = match self.jobs.start(&queued.id) {
            Ok(job) => job,
            Err(error) => {
                let _ = self.store.update_clone_stage(
                    &input.operation_id,
                    CloneStage::Failed,
                    Utc::now(),
                );
                return Err(error);
            }
        };
        if let Err(error) = paths.write_marker() {
            return Err(self.fail_before_rename(
                &input.operation_id,
                &running.id,
                &paths,
                error,
                false,
                reporter.as_ref(),
            ));
        }

        if let Err(error) =
            self.store
                .update_clone_stage(&input.operation_id, CloneStage::Transferring, Utc::now())
        {
            return Err(self.fail_before_rename(
                &input.operation_id,
                &running.id,
                &paths,
                error,
                false,
                reporter.as_ref(),
            ));
        }
        reporter.report(NetworkProgress {
            operation_id: input.operation_id.clone(),
            stage: NetworkStage::Transferring,
            fraction: None,
            objects: None,
            bytes: None,
        });
        let progress = Arc::new(ProgressBridge::new(
            input.operation_id.clone(),
            self.jobs.clone(),
            reporter.clone(),
        ));
        let cancellation = operation_guard.cancellation();
        let mut context = GitRunContext::new(if input.interactive {
            GitExecutionPolicy::ForegroundNetworkInteractive
        } else {
            GitExecutionPolicy::BackgroundNetworkNonInteractive
        })
        .with_progress(progress);
        context.cancellation = cancellation.clone();
        let mut clone_args = Vec::<OsString>::new();
        if let Some(plan) = &profile_plan {
            for (key, value) in &plan.values {
                clone_args.push(OsString::from("-c"));
                clone_args.push(OsString::from(format!("{key}={value}")));
            }
        }
        clone_args.extend([
            OsString::from("clone"),
            OsString::from("--no-checkout"),
            OsString::from("--no-recurse-submodules"),
            OsString::from("--progress"),
            OsString::from("--"),
            OsString::from(&resolved.url.execution_url),
            paths.staging.as_os_str().to_owned(),
        ]);
        let transfer = self.runner.run_with_context(
            GitCommand {
                repo: paths.parent.clone(),
                args: clone_args,
                stdin: None,
                timeout: CLONE_TIMEOUT,
            },
            context,
        );
        match transfer {
            Ok(output) if output.status.success() => {}
            Ok(output) => {
                let failure = AppError::Transport(
                    classify_git_failure(&output.stderr)
                        .with_operation(&input.operation_id)
                        .with_failed_step("clone"),
                );
                return Err(self.fail_before_rename(
                    &input.operation_id,
                    &running.id,
                    &paths,
                    failure,
                    false,
                    reporter.as_ref(),
                ));
            }
            Err(error) => {
                let (failure, canceled) = classify_execution_error(error);
                let failure = AppError::Transport(
                    failure
                        .with_operation(&input.operation_id)
                        .with_failed_step("clone"),
                );
                return Err(self.fail_before_rename(
                    &input.operation_id,
                    &running.id,
                    &paths,
                    failure,
                    canceled,
                    reporter.as_ref(),
                ));
            }
        }

        if cancellation.load(Ordering::Acquire) {
            let failure = AppError::Transport(
                TransportFailure::cancelled()
                    .with_operation(&input.operation_id)
                    .with_failed_step("validateClone"),
            );
            return Err(self.fail_before_rename(
                &input.operation_id,
                &running.id,
                &paths,
                failure,
                true,
                reporter.as_ref(),
            ));
        }
        if let Err(error) =
            self.validate_cloned_repository(&paths, &resolved.url.execution_url, &cancellation)
        {
            let (error, canceled) =
                contextualize_clone_stage_error(error, &input.operation_id, "validateClone");
            return Err(self.fail_before_rename(
                &input.operation_id,
                &running.id,
                &paths,
                error,
                canceled,
                reporter.as_ref(),
            ));
        }
        if let Err(error) =
            self.store
                .update_clone_stage(&input.operation_id, CloneStage::CheckingOut, Utc::now())
        {
            return Err(self.fail_before_rename(
                &input.operation_id,
                &running.id,
                &paths,
                error,
                false,
                reporter.as_ref(),
            ));
        }
        reporter.report(NetworkProgress {
            operation_id: input.operation_id.clone(),
            stage: NetworkStage::CheckingOut,
            fraction: None,
            objects: None,
            bytes: None,
        });
        if let Err(error) = self.checkout_safely(&paths, &input.operation_id, &cancellation) {
            let (error, canceled) =
                contextualize_clone_stage_error(error, &input.operation_id, "checkoutClone");
            return Err(self.fail_before_rename(
                &input.operation_id,
                &running.id,
                &paths,
                error,
                canceled,
                reporter.as_ref(),
            ));
        }
        if cancellation.load(Ordering::Acquire) {
            let failure = AppError::Transport(
                TransportFailure::cancelled()
                    .with_operation(&input.operation_id)
                    .with_failed_step("checkoutClone"),
            );
            return Err(self.fail_before_rename(
                &input.operation_id,
                &running.id,
                &paths,
                failure,
                true,
                reporter.as_ref(),
            ));
        }
        if let Err(error) = paths.rename_staging_to_final() {
            let failure = match error {
                AppError::Transport(failure)
                    if failure.code() == "git.transport.destination-exists" =>
                {
                    AppError::Transport(
                        failure
                            .with_operation(&input.operation_id)
                            .with_failed_step("renameClone"),
                    )
                }
                _ => AppError::Transport(
                    TransportFailure::partial()
                        .with_operation(&input.operation_id)
                        .with_failed_step("renameClone"),
                ),
            };
            let _ =
                self.store
                    .update_clone_stage(&input.operation_id, CloneStage::Partial, Utc::now());
            let _ = self.jobs.fail(
                &running.id,
                crate::error::ErrorEnvelope::from(failure_for_envelope(&failure)),
            );
            reporter.report(terminal_progress(
                &input.operation_id,
                NetworkStage::Partial,
            ));
            return Err(failure);
        }
        if let Err(error) = self
            .store
            .mark_clone_filesystem_complete(&input.operation_id, Utc::now())
        {
            return Err(self.partial_after_rename(
                &input.operation_id,
                &running.id,
                "persistFilesystem",
                error,
                reporter.as_ref(),
            ));
        }
        self.ensure_not_cancelled_after_rename(
            &cancellation,
            &input.operation_id,
            &running.id,
            "persistFilesystem",
            reporter.as_ref(),
        )?;

        let registration =
            self.register_clone(&input, &paths, existing_project, resolved.intent.as_ref());
        let (project, repository, snapshot) = match registration {
            Ok(result) => result,
            Err(error) => {
                return Err(self.partial_after_rename(
                    &input.operation_id,
                    &running.id,
                    "registerClone",
                    error,
                    reporter.as_ref(),
                ));
            }
        };
        self.ensure_not_cancelled_after_rename(
            &cancellation,
            &input.operation_id,
            &running.id,
            "registerClone",
            reporter.as_ref(),
        )?;
        if let Some(profile_id) = input.profile_id.as_deref() {
            if let Err(error) = self.store.update_clone_stage(
                &input.operation_id,
                CloneStage::ApplyingProfile,
                Utc::now(),
            ) {
                return Err(self.partial_after_rename(
                    &input.operation_id,
                    &running.id,
                    "applyProfile",
                    error,
                    reporter.as_ref(),
                ));
            }
            if let Err(error) = self
                .profiles
                .bind_repository(&repository.id, profile_id, false)
            {
                return Err(self.partial_after_rename(
                    &input.operation_id,
                    &running.id,
                    "applyProfile",
                    error,
                    reporter.as_ref(),
                ));
            }
            self.ensure_not_cancelled_after_rename(
                &cancellation,
                &input.operation_id,
                &running.id,
                "applyProfile",
                reporter.as_ref(),
            )?;
            if let Err(error) = self
                .store
                .mark_clone_profile(&input.operation_id, Utc::now())
            {
                return Err(self.partial_after_rename(
                    &input.operation_id,
                    &running.id,
                    "applyProfile",
                    error,
                    reporter.as_ref(),
                ));
            }
            self.ensure_not_cancelled_after_rename(
                &cancellation,
                &input.operation_id,
                &running.id,
                "applyProfile",
                reporter.as_ref(),
            )?;
        }
        if let Some(intent) = resolved.intent.as_ref() {
            let Some(provider) = &self.provider else {
                return Err(self.partial_after_rename(
                    &input.operation_id,
                    &running.id,
                    "bindProvider",
                    AppError::InvalidInput("Clone Provider binding is unavailable".to_owned()),
                    reporter.as_ref(),
                ));
            };
            if let Err(error) = provider.bind_clone_remote(&repository.id, "origin", intent) {
                return Err(self.partial_after_rename(
                    &input.operation_id,
                    &running.id,
                    "bindProvider",
                    error,
                    reporter.as_ref(),
                ));
            }
            self.ensure_not_cancelled_after_rename(
                &cancellation,
                &input.operation_id,
                &running.id,
                "bindProvider",
                reporter.as_ref(),
            )?;
            if let Err(error) = self
                .store
                .mark_clone_provider_binding(&input.operation_id, Utc::now())
            {
                return Err(self.partial_after_rename(
                    &input.operation_id,
                    &running.id,
                    "bindProvider",
                    error,
                    reporter.as_ref(),
                ));
            }
            self.ensure_not_cancelled_after_rename(
                &cancellation,
                &input.operation_id,
                &running.id,
                "bindProvider",
                reporter.as_ref(),
            )?;
        }
        if let Err(error) =
            self.store
                .update_clone_stage(&input.operation_id, CloneStage::Refreshing, Utc::now())
        {
            return Err(self.partial_after_rename(
                &input.operation_id,
                &running.id,
                "finalizeClone",
                error,
                reporter.as_ref(),
            ));
        }
        self.ensure_not_cancelled_after_rename(
            &cancellation,
            &input.operation_id,
            &running.id,
            "finalizeClone",
            reporter.as_ref(),
        )?;
        if let Err(error) = paths.remove_marker() {
            return Err(self.partial_after_rename(
                &input.operation_id,
                &running.id,
                "finalizeClone",
                error,
                reporter.as_ref(),
            ));
        }
        self.ensure_not_cancelled_after_rename(
            &cancellation,
            &input.operation_id,
            &running.id,
            "finalizeClone",
            reporter.as_ref(),
        )?;
        if let Err(error) =
            self.store
                .update_clone_stage(&input.operation_id, CloneStage::Completed, Utc::now())
        {
            return Err(self.partial_after_rename(
                &input.operation_id,
                &running.id,
                "finalizeClone",
                error,
                reporter.as_ref(),
            ));
        }
        if !operation_guard.finish_if_not_cancelled() {
            return Err(self.partial_after_rename(
                &input.operation_id,
                &running.id,
                "finalizeClone",
                AppError::Canceled,
                reporter.as_ref(),
            ));
        }
        let job = match self.jobs.succeed(&running.id) {
            Ok(job) => job,
            Err(error) => {
                return Err(self.partial_after_rename(
                    &input.operation_id,
                    &running.id,
                    "finalizeClone",
                    error,
                    reporter.as_ref(),
                ));
            }
        };
        reporter.report(terminal_progress(
            &input.operation_id,
            NetworkStage::Completed,
        ));
        Ok(CloneResult {
            operation_id: input.operation_id,
            intent_id: resolved.intent.map(|intent| intent.intent_id),
            status: CloneResultStatus::Completed,
            job,
            project: CloneProjectSummary::from(&project),
            repository: (&repository).into(),
            snapshot,
        })
    }

    pub fn classify_incomplete_recovery(
        &self,
    ) -> Result<Vec<CloneRecoveryClassification>, AppError> {
        self.store
            .list_incomplete_clone_operations()?
            .into_iter()
            .map(|operation| self.classify_recovery_operation(operation))
            .collect()
    }

    fn classify_recovery_operation(
        &self,
        operation: CloneOperation,
    ) -> Result<CloneRecoveryClassification, AppError> {
        let paths = match ClonePaths::from_operation(&operation) {
            Ok(paths) => paths,
            Err(_) => {
                return Ok(CloneRecoveryClassification {
                    operation_id: operation.operation_id,
                    actions: vec![CloneRecoveryAction::UnsafePath],
                });
            }
        };
        let staging = safe_existing_directory(&paths.staging);
        let final_path = safe_existing_directory(&paths.final_path);
        let marker_exists = fs::symlink_metadata(&paths.marker).is_ok();
        let marker_valid = marker_exists
            && validate_marker(
                &paths.parent,
                &paths.staging,
                &paths.marker,
                &paths.operation_id,
            )
            .is_ok();
        let unsafe_existing = matches!(staging, ExistingDirectory::Unsafe)
            || matches!(final_path, ExistingDirectory::Unsafe)
            || (marker_exists && !marker_valid);
        if unsafe_existing {
            return Ok(CloneRecoveryClassification {
                operation_id: operation.operation_id,
                actions: vec![CloneRecoveryAction::UnsafePath],
            });
        }
        let actions = match (staging, final_path, marker_valid) {
            (ExistingDirectory::Safe, ExistingDirectory::Missing, true) => vec![
                CloneRecoveryAction::CleanupStaging,
                CloneRecoveryAction::RetryClone,
            ],
            (ExistingDirectory::Missing, ExistingDirectory::Safe, _)
                if operation.filesystem_complete =>
            {
                vec![CloneRecoveryAction::RetryRegistration]
            }
            (ExistingDirectory::Missing, ExistingDirectory::Missing, true) => {
                fs::remove_file(&paths.marker)?;
                self.store.update_clone_stage(
                    &operation.operation_id,
                    CloneStage::Failed,
                    Utc::now(),
                )?;
                let interrupted = TransportFailure::interrupted()
                    .with_operation(&operation.operation_id)
                    .with_failed_step("cloneRecovery");
                if self.jobs.list()?.into_iter().any(|job| {
                    job.id == operation.job_id
                        && job.status == crate::jobs::model::JobStatus::Running
                }) {
                    let _ = self.jobs.fail(
                        &operation.job_id,
                        crate::error::ErrorEnvelope::from(AppError::Transport(interrupted)),
                    );
                }
                vec![CloneRecoveryAction::Interrupted]
            }
            _ => vec![CloneRecoveryAction::UnsafePath],
        };
        Ok(CloneRecoveryClassification {
            operation_id: operation.operation_id,
            actions,
        })
    }

    fn resolve_source(
        &self,
        source: &CloneSource,
        transport_kind: TransportKind,
    ) -> Result<ResolvedCloneSource, AppError> {
        match source {
            CloneSource::Manual(url) => {
                let url = validate_clone_url(url)?;
                if url.kind != transport_kind {
                    return Err(AppError::Transport(TransportFailure::profile_mismatch()));
                }
                Ok(ResolvedCloneSource {
                    summary: url.sanitized_display.clone(),
                    url,
                    intent: None,
                })
            }
            CloneSource::Intent(intent_id) => {
                let intent = self.intents.consume(intent_id, GIT_CLIENT_PLUGIN_ID)?;
                let raw_url = match transport_kind {
                    TransportKind::Https => &intent.repository.https_url,
                    TransportKind::Ssh => &intent.repository.ssh_url,
                };
                let url = validate_clone_url(raw_url)?;
                if url.kind != transport_kind {
                    return Err(AppError::Transport(TransportFailure::profile_mismatch()));
                }
                Ok(ResolvedCloneSource {
                    summary: intent.repository.full_name.clone(),
                    url,
                    intent: Some(intent),
                })
            }
        }
    }

    fn validate_project_target(
        &self,
        target: &CloneProjectTarget,
        paths: &ClonePaths,
    ) -> Result<Option<Project>, AppError> {
        match target {
            CloneProjectTarget::Existing { project_id } => {
                let project = self.git.get_project(project_id)?;
                validate_existing_project_destination(&project, &paths.final_path)?;
                Ok(Some(project))
            }
            CloneProjectTarget::New { name } => {
                validate_project_name(name)?;
                Ok(None)
            }
        }
    }

    fn validate_cloned_repository(
        &self,
        paths: &ClonePaths,
        expected_origin: &str,
        cancellation: &Arc<AtomicBool>,
    ) -> Result<(), AppError> {
        let metadata = fs::symlink_metadata(&paths.staging)?;
        if !metadata.is_dir() || is_symlink_or_reparse(&metadata) {
            return Err(unsafe_path());
        }
        let detected = detect_repository(&paths.staging)?;
        if detected.kind != RepositoryKind::Normal
            || detected.canonical_path != fs::canonicalize(&paths.staging)?
        {
            return Err(unsafe_path());
        }
        let git_dir = paths.staging.join(".git");
        let git_metadata = fs::symlink_metadata(&git_dir)?;
        if !git_metadata.is_dir() || is_symlink_or_reparse(&git_metadata) {
            return Err(unsafe_path());
        }
        let canonical_git_dir = fs::canonicalize(&git_dir)?;
        if !canonical_git_dir.starts_with(fs::canonicalize(&paths.staging)?) {
            return Err(unsafe_path());
        }
        let origin = self.required_git_line(
            &paths.staging,
            &["config", "--local", "--get", "remote.origin.url"],
            cancellation,
        )?;
        if origin != expected_origin {
            return Err(AppError::InvalidInput(
                "Clone origin did not match the validated source".to_owned(),
            ));
        }
        self.required_git_success(
            &paths.staging,
            &["rev-parse", "--verify", "HEAD^{commit}"],
            cancellation,
        )?;
        self.required_git_success(
            &paths.staging,
            &["rev-parse", "--verify", "HEAD^{tree}"],
            cancellation,
        )?;
        Ok(())
    }

    fn checkout_safely(
        &self,
        paths: &ClonePaths,
        operation_id: &str,
        cancellation: &Arc<AtomicBool>,
    ) -> Result<(), AppError> {
        let mut guard = CheckoutSafetyGuard::install(paths, operation_id)?;
        let hooks = path_string(&guard.hooks_path)?;
        let result = self.required_git_success(
            &paths.staging,
            &[
                "-c",
                &format!("core.hooksPath={hooks}"),
                "checkout",
                "--force",
                "--no-recurse-submodules",
            ],
            cancellation,
        );
        let cleanup = guard.finish();
        result.and(cleanup)
    }

    fn register_clone(
        &self,
        input: &CloneInput,
        paths: &ClonePaths,
        existing_project: Option<Project>,
        _intent: Option<&ConsumedCloneIntent>,
    ) -> Result<
        (
            Project,
            Repository,
            Option<crate::git::model::RepositorySnapshot>,
        ),
        AppError,
    > {
        self.store
            .update_clone_stage(&input.operation_id, CloneStage::Registering, Utc::now())?;
        let project = match existing_project {
            Some(project) => project,
            None => {
                let CloneProjectTarget::New { name } = &input.project_target else {
                    return Err(AppError::InvalidInput(
                        "Clone Project target changed".to_owned(),
                    ));
                };
                let project = self.git.create_project(ProjectCreateInput {
                    root_path: path_string(&paths.final_path)?,
                    name: name.clone(),
                    scan_depth: Some(0),
                    exclude_patterns: Vec::new(),
                })?;
                self.store
                    .mark_clone_project(&input.operation_id, &project.id, Utc::now())?;
                project
            }
        };
        let scan = self.git.scan_project(&project.id)?;
        let canonical_final = fs::canonicalize(&paths.final_path)?;
        let record = scan
            .repositories
            .into_iter()
            .find(|record| Path::new(&record.repository.canonical_path) == canonical_final)
            .ok_or_else(|| {
                AppError::NotFound("cloned Repository was not registered in the Project".to_owned())
            })?;
        if record.error.is_some() {
            return Err(AppError::Git("cloned Repository refresh failed".to_owned()));
        }
        self.git.trust_repository(&record.repository.id)?;
        self.store.mark_clone_repository(
            &input.operation_id,
            &record.repository.id,
            Some(&project.id),
            Utc::now(),
        )?;
        Ok((project, record.repository, record.snapshot))
    }

    fn required_git_success(
        &self,
        repository: &Path,
        args: &[&str],
        cancellation: &Arc<AtomicBool>,
    ) -> Result<(), AppError> {
        let output = self.run_local_git(repository, args, cancellation)?;
        if output.status.success() {
            Ok(())
        } else {
            Err(AppError::Git("Clone validation command failed".to_owned()))
        }
    }

    fn required_git_line(
        &self,
        repository: &Path,
        args: &[&str],
        cancellation: &Arc<AtomicBool>,
    ) -> Result<String, AppError> {
        let output = self.run_local_git(repository, args, cancellation)?;
        if !output.status.success() {
            return Err(AppError::Git("Clone validation command failed".to_owned()));
        }
        let value = std::str::from_utf8(&output.stdout)
            .map_err(|_| AppError::InvalidInput("Clone validation output is not UTF-8".to_owned()))?
            .trim_end_matches(['\r', '\n']);
        if value.is_empty() || value.contains(['\r', '\n', '\0']) {
            return Err(AppError::InvalidInput(
                "Clone validation output is malformed".to_owned(),
            ));
        }
        Ok(value.to_owned())
    }

    fn run_local_git(
        &self,
        repository: &Path,
        args: &[&str],
        cancellation: &Arc<AtomicBool>,
    ) -> Result<crate::git::engine::GitOutput, AppError> {
        let mut context = GitRunContext::new(GitExecutionPolicy::LocalNonInteractive);
        context.cancellation = cancellation.clone();
        self.runner.run_with_context(
            GitCommand {
                repo: repository.to_path_buf(),
                args: args.iter().map(|value| OsString::from(*value)).collect(),
                stdin: None,
                timeout: LOCAL_GIT_TIMEOUT,
            },
            context,
        )
    }

    fn fail_before_rename(
        &self,
        operation_id: &str,
        job_id: &str,
        paths: &ClonePaths,
        error: AppError,
        canceled: bool,
        reporter: &dyn NetworkProgressReporter,
    ) -> AppError {
        let cleanup_needed = fs::symlink_metadata(&paths.marker).is_ok()
            || fs::symlink_metadata(&paths.staging).is_ok();
        if cleanup_needed && paths.cleanup_owned_staging().is_err() {
            return self.partial_after_rename(
                operation_id,
                job_id,
                "cleanupClone",
                error,
                reporter,
            );
        }
        let stage = if canceled {
            CloneStage::Cancelled
        } else {
            CloneStage::Failed
        };
        let _ = self
            .store
            .update_clone_stage(operation_id, stage, Utc::now());
        if canceled {
            let _ = self.jobs.cancel(job_id);
        } else {
            let _ = self.jobs.fail(
                job_id,
                crate::error::ErrorEnvelope::from(failure_for_envelope(&error)),
            );
        }
        reporter.report(terminal_progress(
            operation_id,
            if canceled {
                NetworkStage::Cancelled
            } else {
                NetworkStage::Failed
            },
        ));
        error
    }

    fn partial_after_rename(
        &self,
        operation_id: &str,
        job_id: &str,
        failed_step: &str,
        _error: AppError,
        reporter: &dyn NetworkProgressReporter,
    ) -> AppError {
        let failure = TransportFailure::partial()
            .with_operation(operation_id)
            .with_failed_step(failed_step);
        let _ = self
            .store
            .update_clone_stage(operation_id, CloneStage::Partial, Utc::now());
        let _ = self.jobs.fail(
            job_id,
            crate::error::ErrorEnvelope::from(AppError::Transport(failure.clone())),
        );
        reporter.report(terminal_progress(operation_id, NetworkStage::Partial));
        AppError::Transport(failure)
    }

    fn ensure_not_cancelled_after_rename(
        &self,
        cancellation: &Arc<AtomicBool>,
        operation_id: &str,
        job_id: &str,
        failed_step: &str,
        reporter: &dyn NetworkProgressReporter,
    ) -> Result<(), AppError> {
        if cancellation.load(Ordering::Acquire) {
            return Err(self.partial_after_rename(
                operation_id,
                job_id,
                failed_step,
                AppError::Canceled,
                reporter,
            ));
        }
        Ok(())
    }
}

struct ResolvedCloneSource {
    summary: String,
    url: super::url::ValidatedRemoteUrl,
    intent: Option<ConsumedCloneIntent>,
}

fn contextualize_clone_stage_error(
    error: AppError,
    operation_id: &str,
    failed_step: &str,
) -> (AppError, bool) {
    match error {
        AppError::Canceled | AppError::Timeout | AppError::Transport(_) => {
            let (failure, canceled) = classify_execution_error(error);
            (
                AppError::Transport(
                    failure
                        .with_operation(operation_id)
                        .with_failed_step(failed_step),
                ),
                canceled,
            )
        }
        error => (error, false),
    }
}

fn failure_for_envelope(error: &AppError) -> AppError {
    match error {
        AppError::Transport(failure) => AppError::Transport(failure.clone()),
        AppError::Canceled => AppError::Transport(TransportFailure::cancelled()),
        AppError::Timeout => AppError::Transport(TransportFailure::timeout()),
        _ => AppError::Transport(TransportFailure::partial()),
    }
}

struct CheckoutSafetyGuard {
    attributes_path: PathBuf,
    original_attributes: Option<Vec<u8>>,
    hooks_path: PathBuf,
    active: bool,
}

impl CheckoutSafetyGuard {
    fn install(paths: &ClonePaths, operation_id: &str) -> Result<Self, AppError> {
        let info = paths.staging.join(".git").join("info");
        let info_metadata = fs::symlink_metadata(&info)?;
        if !info_metadata.is_dir() || is_symlink_or_reparse(&info_metadata) {
            return Err(unsafe_path());
        }
        let attributes_path = info.join("attributes");
        let original_attributes = match fs::symlink_metadata(&attributes_path) {
            Ok(metadata) => {
                if !metadata.is_file()
                    || is_symlink_or_reparse(&metadata)
                    || metadata.len() > 1024 * 1024
                {
                    return Err(unsafe_path());
                }
                Some(fs::read(&attributes_path)?)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => return Err(AppError::Io(error)),
        };
        let hooks_path = paths
            .staging
            .join(".git")
            .join(format!("git-ramus-empty-hooks-{operation_id}"));
        if path_is_occupied(&hooks_path)? {
            return Err(AppError::Transport(TransportFailure::destination_exists()));
        }
        fs::create_dir(&hooks_path)?;
        if let Err(error) = fs::write(
            &attributes_path,
            b"* -filter -diff -merge -working-tree-encoding\n**/* -filter -diff -merge -working-tree-encoding\n",
        ) {
            let _ = fs::remove_dir(&hooks_path);
            return Err(AppError::Io(error));
        }
        Ok(Self {
            attributes_path,
            original_attributes,
            hooks_path,
            active: true,
        })
    }

    fn finish(&mut self) -> Result<(), AppError> {
        if !self.active {
            return Ok(());
        }
        match &self.original_attributes {
            Some(original) => fs::write(&self.attributes_path, original)?,
            None => match fs::remove_file(&self.attributes_path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(AppError::Io(error)),
            },
        }
        fs::remove_dir(&self.hooks_path)?;
        self.active = false;
        Ok(())
    }
}

impl Drop for CheckoutSafetyGuard {
    fn drop(&mut self) {
        let _ = self.finish();
    }
}

fn validate_existing_project_destination(
    project: &Project,
    final_path: &Path,
) -> Result<(), AppError> {
    let root = fs::canonicalize(&project.root_path)?;
    let parent = final_path.parent().ok_or_else(unsafe_path)?;
    let parent = fs::canonicalize(parent)?;
    let relative_parent = parent.strip_prefix(&root).map_err(|_| unsafe_path())?;
    let folder = final_path
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(unsafe_path)?;
    let relative = relative_parent.join(folder);
    let components = relative
        .components()
        .map(|component| match component {
            std::path::Component::Normal(value) => value
                .to_str()
                .map(str::to_owned)
                .ok_or(AppError::NonUtf8Path),
            _ => Err(unsafe_path()),
        })
        .collect::<Result<Vec<_>, _>>()?;
    let depth = i64::try_from(components.len()).map_err(|_| AppError::OutputLimit)?;
    if depth > project.scan_depth {
        return Err(AppError::InvalidInput(
            "Clone destination is deeper than the Project scan depth".to_owned(),
        ));
    }
    let mut prefix = String::new();
    for component in components {
        if !prefix.is_empty() {
            prefix.push('/');
        }
        prefix.push_str(&component);
        if clone_path_is_excluded(&component, &prefix, &project.exclude_patterns) {
            return Err(AppError::InvalidInput(
                "Clone destination is excluded by the Project scan rules".to_owned(),
            ));
        }
    }
    Ok(())
}

fn validate_project_name(value: &str) -> Result<(), AppError> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 128 || value.chars().any(char::is_control) {
        return Err(AppError::InvalidInput(
            "Clone Project name is invalid".to_owned(),
        ));
    }
    Ok(())
}

fn clone_path_is_excluded(name: &str, relative: &str, patterns: &[String]) -> bool {
    const DEFAULT_EXCLUDES: &[&str] = &[
        ".git",
        "node_modules",
        "target",
        "dist",
        "build",
        ".next",
        ".cache",
        ".venv",
        "vendor",
    ];
    DEFAULT_EXCLUDES.contains(&name)
        || patterns.iter().any(|pattern| {
            let normalized = pattern.replace('\\', "/");
            clone_glob_match(&normalized, name) || clone_glob_match(&normalized, relative)
        })
}

fn clone_glob_match(pattern: &str, value: &str) -> bool {
    let pattern = pattern.as_bytes();
    let value = value.as_bytes();
    let mut state = vec![false; value.len() + 1];
    state[0] = true;
    for &token in pattern {
        let mut next = vec![false; value.len() + 1];
        if token == b'*' {
            let mut seen = false;
            for index in 0..=value.len() {
                seen |= state[index];
                next[index] = seen;
            }
        } else {
            for index in 0..value.len() {
                if state[index] && (token == b'?' || token == value[index]) {
                    next[index + 1] = true;
                }
            }
        }
        state = next;
    }
    state[value.len()]
}

fn path_string(path: &Path) -> Result<String, AppError> {
    path.to_str()
        .map(str::to_owned)
        .ok_or(AppError::NonUtf8Path)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClonePaths {
    pub parent: PathBuf,
    pub staging: PathBuf,
    pub marker: PathBuf,
    pub final_path: PathBuf,
    operation_id: String,
    staging_basename: String,
}

impl ClonePaths {
    fn from_operation(operation: &CloneOperation) -> Result<Self, AppError> {
        uuid::Uuid::parse_str(&operation.operation_id).map_err(|_| unsafe_path())?;
        let staging = PathBuf::from(&operation.staging_path);
        let marker = PathBuf::from(&operation.owner_marker_path);
        let final_path = PathBuf::from(&operation.final_path);
        if !staging.is_absolute() || !marker.is_absolute() || !final_path.is_absolute() {
            return Err(unsafe_path());
        }
        let parent = staging.parent().ok_or_else(unsafe_path)?.to_path_buf();
        if marker.parent() != Some(parent.as_path())
            || final_path.parent() != Some(parent.as_path())
        {
            return Err(unsafe_path());
        }
        let staging_basename = format!("{STAGING_PREFIX}{}", operation.operation_id);
        if staging.file_name().and_then(|value| value.to_str()) != Some(staging_basename.as_str())
            || marker.file_name().and_then(|value| value.to_str())
                != Some(format!("{staging_basename}{MARKER_SUFFIX}").as_str())
        {
            return Err(unsafe_path());
        }
        let final_name = final_path
            .file_name()
            .and_then(|value| value.to_str())
            .ok_or_else(unsafe_path)?;
        validate_clone_folder_name(final_name)?;
        safe_canonical_parent(&parent)?;
        Ok(Self {
            parent,
            staging,
            marker,
            final_path,
            operation_id: operation.operation_id.clone(),
            staging_basename,
        })
    }

    pub fn allocate(
        parent: &Path,
        folder_name: &str,
        operation_id: &str,
    ) -> Result<Self, AppError> {
        uuid::Uuid::parse_str(operation_id)
            .map_err(|_| AppError::InvalidInput("Clone operation id must be a UUID".to_owned()))?;
        validate_clone_folder_name(folder_name)?;
        if !parent.is_absolute() {
            return Err(unsafe_path());
        }
        let metadata = fs::symlink_metadata(parent)?;
        if !metadata.is_dir() || is_symlink_or_reparse(&metadata) {
            return Err(unsafe_path());
        }
        let parent = dunce::canonicalize(parent)?;
        let staging_basename = format!("{STAGING_PREFIX}{operation_id}");
        let staging = parent.join(&staging_basename);
        let marker = parent.join(format!("{staging_basename}{MARKER_SUFFIX}"));
        let final_path = parent.join(folder_name);
        if path_is_occupied(&staging)?
            || path_is_occupied(&marker)?
            || path_is_occupied(&final_path)?
        {
            return Err(AppError::Transport(TransportFailure::destination_exists()));
        }
        Ok(Self {
            parent,
            staging,
            marker,
            final_path,
            operation_id: operation_id.to_owned(),
            staging_basename,
        })
    }

    pub fn write_marker(&self) -> Result<(), AppError> {
        if path_is_occupied(&self.staging)? || path_is_occupied(&self.marker)? {
            return Err(AppError::Transport(TransportFailure::destination_exists()));
        }
        let mut marker = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&self.marker)?;
        marker.write_all(marker_contents(&self.operation_id, &self.staging_basename).as_bytes())?;
        marker.sync_all()?;
        Ok(())
    }

    pub fn cleanup_owned_staging(&self) -> Result<(), AppError> {
        cleanup_staging(
            &self.parent,
            &self.staging,
            &self.marker,
            &self.operation_id,
        )
    }

    pub fn rename_staging_to_final(&self) -> Result<(), AppError> {
        validate_owned_staging(
            &self.parent,
            &self.staging,
            &self.marker,
            &self.operation_id,
        )?;
        if path_is_occupied(&self.final_path)? {
            return Err(AppError::Transport(TransportFailure::destination_exists()));
        }
        atomic_rename_noreplace(&self.staging, &self.final_path).map_err(|error| {
            if fs::symlink_metadata(&self.final_path).is_ok() {
                AppError::Transport(TransportFailure::destination_exists())
            } else {
                AppError::Io(error)
            }
        })
    }

    pub fn remove_marker(&self) -> Result<(), AppError> {
        validate_marker(
            &self.parent,
            &self.staging,
            &self.marker,
            &self.operation_id,
        )?;
        fs::remove_file(&self.marker)?;
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExistingDirectory {
    Missing,
    Safe,
    Unsafe,
}

fn safe_existing_directory(path: &Path) -> ExistingDirectory {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() && !is_symlink_or_reparse(&metadata) => {
            ExistingDirectory::Safe
        }
        Ok(_) => ExistingDirectory::Unsafe,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => ExistingDirectory::Missing,
        Err(_) => ExistingDirectory::Unsafe,
    }
}

fn path_is_occupied(path: &Path) -> Result<bool, AppError> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(AppError::Io(error)),
    }
}

pub fn cleanup_staging(
    parent: &Path,
    staging: &Path,
    marker: &Path,
    operation_id: &str,
) -> Result<(), AppError> {
    safe_canonical_parent(parent)?;
    validate_marker(parent, staging, marker, operation_id)?;
    if staging.exists() {
        validate_owned_staging(parent, staging, marker, operation_id)?;
        fs::remove_dir_all(staging)?;
    } else if fs::symlink_metadata(staging).is_ok() {
        return Err(unsafe_path());
    }
    fs::remove_file(marker)?;
    Ok(())
}

fn validate_owned_staging(
    parent: &Path,
    staging: &Path,
    marker: &Path,
    operation_id: &str,
) -> Result<(), AppError> {
    let canonical_parent = safe_canonical_parent(parent)?;
    validate_marker(parent, staging, marker, operation_id)?;
    let basename = staging
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(unsafe_path)?;
    if basename != format!("{STAGING_PREFIX}{operation_id}") {
        return Err(unsafe_path());
    }
    let metadata = fs::symlink_metadata(staging)?;
    if !metadata.is_dir() || is_symlink_or_reparse(&metadata) {
        return Err(unsafe_path());
    }
    let canonical_staging = fs::canonicalize(staging)?;
    if canonical_staging.parent() != Some(canonical_parent.as_path())
        || canonical_staging.file_name() != staging.file_name()
    {
        return Err(unsafe_path());
    }
    Ok(())
}

fn validate_marker(
    parent: &Path,
    staging: &Path,
    marker: &Path,
    operation_id: &str,
) -> Result<(), AppError> {
    uuid::Uuid::parse_str(operation_id).map_err(|_| unsafe_path())?;
    let canonical_parent = safe_canonical_parent(parent)?;
    let Some(staging_parent) = staging.parent() else {
        return Err(unsafe_path());
    };
    if marker.parent() != Some(staging_parent)
        || safe_canonical_parent(staging_parent)? != canonical_parent
    {
        return Err(unsafe_path());
    }
    let staging_basename = staging
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(unsafe_path)?;
    if staging_basename != format!("{STAGING_PREFIX}{operation_id}")
        || marker.file_name().and_then(|value| value.to_str())
            != Some(format!("{staging_basename}{MARKER_SUFFIX}").as_str())
    {
        return Err(unsafe_path());
    }
    let metadata = fs::symlink_metadata(marker)?;
    if !metadata.is_file() || is_symlink_or_reparse(&metadata) || metadata.len() > MAX_MARKER_BYTES
    {
        return Err(unsafe_path());
    }
    let contents = fs::read_to_string(marker)?;
    if contents != marker_contents(operation_id, staging_basename) {
        return Err(unsafe_path());
    }
    Ok(())
}

fn marker_contents(operation_id: &str, staging_basename: &str) -> String {
    format!("{operation_id}\n{staging_basename}\n")
}

fn safe_canonical_parent(parent: &Path) -> Result<PathBuf, AppError> {
    let metadata = fs::symlink_metadata(parent)?;
    if !metadata.is_dir() || is_symlink_or_reparse(&metadata) {
        return Err(unsafe_path());
    }
    Ok(fs::canonicalize(parent)?)
}

fn validate_clone_folder_name(value: &str) -> Result<(), AppError> {
    let upper = value
        .split('.')
        .next()
        .unwrap_or(value)
        .to_ascii_uppercase();
    let windows_device = matches!(upper.as_str(), "CON" | "PRN" | "AUX" | "NUL")
        || upper
            .strip_prefix("COM")
            .or_else(|| upper.strip_prefix("LPT"))
            .is_some_and(|number| {
                matches!(number, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9")
            });
    if value.is_empty()
        || value.len() > 255
        || matches!(value, "." | "..")
        || value.ends_with(['.', ' '])
        || Path::new(value).is_absolute()
        || value.contains(['/', '\\'])
        || value.chars().any(char::is_control)
        || windows_device
    {
        return Err(AppError::InvalidInput(
            "unsafe Clone destination folder name".to_owned(),
        ));
    }
    Ok(())
}

fn is_symlink_or_reparse(metadata: &fs::Metadata) -> bool {
    if metadata.file_type().is_symlink() {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    false
}

#[cfg(target_os = "linux")]
fn atomic_rename_noreplace(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let source = CString::new(source.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let destination = CString::new(destination.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let result = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            libc::AT_FDCWD,
            source.as_ptr(),
            libc::AT_FDCWD,
            destination.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn atomic_rename_noreplace(source: &Path, destination: &Path) -> std::io::Result<()> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;

    let source = CString::new(source.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let destination = CString::new(destination.as_os_str().as_bytes())
        .map_err(|_| std::io::Error::from(std::io::ErrorKind::InvalidInput))?;
    let result =
        unsafe { libc::renamex_np(source.as_ptr(), destination.as_ptr(), libc::RENAME_EXCL) };
    if result == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

#[cfg(windows)]
fn atomic_rename_noreplace(source: &Path, destination: &Path) -> std::io::Result<()> {
    // `std::fs::rename` on Windows fails when the destination exists.
    fs::rename(source, destination)
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "ios", windows)))]
fn atomic_rename_noreplace(_source: &Path, _destination: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "atomic no-replace rename is unsupported on this platform",
    ))
}

fn unsafe_path() -> AppError {
    AppError::Transport(TransportFailure::unsafe_path())
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use chrono::{DateTime, Duration, Utc};

    use super::{CloneClock, CloneIntentRegistry, ClonePaths, cleanup_staging};
    use crate::error::AppError;
    use crate::providers::model::{
        ProviderKind, ProviderPermission, ProviderVisibility, RemoteRepository,
    };

    const OPERATION_ID: &str = "b95c216a-dac4-45d1-8169-8dbfbc0c0315";

    #[test]
    fn clone_owner_marker_is_a_sidecar_and_cleanup_requires_exact_operation_ownership() {
        let parent = tempfile::tempdir().unwrap();
        let paths = ClonePaths::allocate(parent.path(), "repository", OPERATION_ID).unwrap();
        assert!(!paths.staging.exists());
        assert_eq!(paths.marker.parent(), Some(parent.path()));
        paths.write_marker().unwrap();
        std::fs::create_dir(&paths.staging).unwrap();
        assert!(paths.cleanup_owned_staging().is_ok());
        assert!(!paths.marker.exists());

        let foreign = parent.path().join(".git-ramus-clone-foreign");
        std::fs::create_dir(&foreign).unwrap();
        assert!(
            cleanup_staging(parent.path(), &foreign, &paths.marker, "other-operation").is_err()
        );
        assert!(foreign.exists());
    }

    #[test]
    fn clone_rename_never_replaces_a_destination_that_appears_after_allocation() {
        let parent = tempfile::tempdir().unwrap();
        let paths = ClonePaths::allocate(parent.path(), "repository", OPERATION_ID).unwrap();
        paths.write_marker().unwrap();
        std::fs::create_dir(&paths.staging).unwrap();
        std::fs::create_dir(&paths.final_path).unwrap();
        assert!(paths.rename_staging_to_final().is_err());
        assert!(paths.staging.exists());
        assert!(paths.final_path.exists());
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn clone_allocation_rejects_a_dangling_link_at_the_final_destination() {
        let parent = tempfile::tempdir().unwrap();
        let destination = parent.path().join("repository");
        let missing = parent.path().join("missing-target");
        #[cfg(unix)]
        std::os::unix::fs::symlink(&missing, &destination).unwrap();
        #[cfg(windows)]
        if let Err(error) = std::os::windows::fs::symlink_dir(&missing, &destination) {
            eprintln!("symlink privilege unavailable; skipping dangling-link assertion: {error}");
            return;
        }

        let error = ClonePaths::allocate(parent.path(), "repository", OPERATION_ID).unwrap_err();
        assert!(matches!(
            error,
            AppError::Transport(failure)
                if failure.code() == "git.transport.destination-exists"
        ));
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn clone_paths_are_anchored_to_the_authorized_canonical_parent() {
        let root = tempfile::tempdir().unwrap();
        let real_ancestor = root.path().join("real-ancestor");
        let alias = root.path().join("ancestor-alias");
        let parent = real_ancestor.join("authorized-parent");
        std::fs::create_dir(&real_ancestor).unwrap();
        std::fs::create_dir(&parent).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&real_ancestor, &alias).unwrap();
        #[cfg(windows)]
        if let Err(error) = std::os::windows::fs::symlink_dir(&real_ancestor, &alias) {
            eprintln!(
                "symlink privilege unavailable; skipping canonical-parent assertion: {error}"
            );
            return;
        }

        let paths =
            ClonePaths::allocate(&alias.join("authorized-parent"), "repository", OPERATION_ID)
                .unwrap();
        assert_eq!(paths.parent, dunce::canonicalize(&parent).unwrap());
        assert_eq!(paths.staging.parent(), Some(paths.parent.as_path()));
        assert_eq!(paths.final_path.parent(), Some(paths.parent.as_path()));
    }

    #[test]
    fn clone_paths_do_not_persist_a_lexical_parent_alias() {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("authorized-parent");
        std::fs::create_dir(&parent).unwrap();
        let lexical_alias = parent.join(".");

        let paths = ClonePaths::allocate(&lexical_alias, "repository", OPERATION_ID).unwrap();
        assert_eq!(paths.parent, dunce::canonicalize(&parent).unwrap());
    }

    struct TestClock(Mutex<DateTime<Utc>>);

    impl CloneClock for TestClock {
        fn now(&self) -> DateTime<Utc> {
            *self.0.lock().unwrap()
        }
    }

    fn remote_repository() -> RemoteRepository {
        RemoteRepository {
            provider_kind: ProviderKind::Gitlab,
            instance_id: "instance".to_owned(),
            repository_id: "42".to_owned(),
            namespace: "acme".to_owned(),
            name: "repository".to_owned(),
            full_name: "acme/repository".to_owned(),
            web_url: "https://git.example.test/acme/repository".to_owned(),
            https_url: "https://git.example.test/acme/repository.git".to_owned(),
            ssh_url: "git@git.example.test:acme/repository.git".to_owned(),
            default_branch: Some("main".to_owned()),
            visibility: ProviderVisibility::Private,
            archived: false,
            fork: false,
            permission: ProviderPermission::Write,
            updated_at: Utc::now(),
        }
    }

    #[test]
    fn clone_intents_are_ten_minute_one_use_git_client_capabilities_without_secrets() {
        let now = Utc::now();
        let clock = Arc::new(TestClock(Mutex::new(now)));
        let registry = CloneIntentRegistry::with_clock(clock.clone());
        let intent = registry
            .insert_verified("git-ramus.provider-center", "account", remote_repository())
            .unwrap();
        assert_eq!(intent.expires_at, now + Duration::minutes(10));
        assert!(registry.consume(&intent.id, "external.plugin").is_err());
        let consumed = registry
            .consume(&intent.id, "git-ramus.git-client")
            .unwrap();
        assert_eq!(consumed.account_id, "account");
        assert!(
            registry
                .consume(&intent.id, "git-ramus.git-client")
                .is_err()
        );
        let serialized = serde_json::to_string(&intent).unwrap();
        assert!(!serialized.contains("provider-pat-fixture"));

        let expired = registry
            .insert_verified("git-ramus.provider-center", "account", remote_repository())
            .unwrap();
        *clock.0.lock().unwrap() = now + Duration::minutes(11);
        assert!(
            registry
                .consume(&expired.id, "git-ramus.git-client")
                .is_err()
        );
        assert!(registry.get(&expired.id).is_err());

        *clock.0.lock().unwrap() = now;
        let abandoned = registry
            .insert_verified("git-ramus.provider-center", "account", remote_repository())
            .unwrap();
        *clock.0.lock().unwrap() = now + Duration::minutes(11);
        let active = registry
            .insert_verified("git-ramus.provider-center", "account", remote_repository())
            .unwrap();
        assert!(!registry.records.lock().contains_key(&abandoned.id));
        assert_eq!(registry.records.lock().len(), 1);
        assert!(registry.cancel(&active.id));
        assert!(registry.records.lock().is_empty());
    }

    #[cfg(any(unix, windows))]
    #[test]
    fn cleanup_rejects_a_staging_symlink_or_reparse_point() {
        let parent = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let paths = ClonePaths::allocate(parent.path(), "repository", OPERATION_ID).unwrap();
        paths.write_marker().unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(outside.path(), &paths.staging).unwrap();
        #[cfg(windows)]
        if let Err(error) = std::os::windows::fs::symlink_dir(outside.path(), &paths.staging) {
            eprintln!("symlink privilege unavailable; skipping reparse-point assertion: {error}");
            return;
        }
        assert!(paths.cleanup_owned_staging().is_err());
        assert!(outside.path().exists());
    }
}
