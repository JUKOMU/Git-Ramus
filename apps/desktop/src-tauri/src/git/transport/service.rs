use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;

use super::clone::{
    CloneCoordinator, CloneIntentRegistry, CloneProviderBinder, CloneRecoveryClassification,
};
use super::model::{
    CloneInput, CloneResult, EffectiveTransportSource, FetchInput, NetworkOperationResult,
    NetworkProgress, NetworkStage, PullInput, PushInput, RemoteTransportKind,
    RepositoryNetworkState, RepositoryOperationInProgress, RepositoryRemoteSummary,
    TransportDriftStatus, TransportKind, UpstreamCandidate,
};
use super::operation::TransportOperationRegistry;
use super::profile_service::TransportProfileService;
use super::progress::GitProgressParser;
use super::store::TransportStore;
use super::url::{ValidatedRemoteUrl, validate_clone_url};
use crate::db::Database;
use crate::error::{AppError, ErrorEnvelope, TransportFailure};
use crate::git::engine::{
    GitCommand, GitExecutionPolicy, GitProgressSink, GitRunContext, GitRunner,
};
use crate::git::model::{Remote, RepositorySnapshot};
use crate::git::repository::{RepositoryRepository, RepositoryWriteLocks};
use crate::git::service::{GitService, QueryContext, RepositoryScanRecord};
use crate::jobs::JobService;

const NETWORK_TIMEOUT: Duration = Duration::from_secs(15 * 60);

struct NetworkExecutionRequest {
    repository_id: String,
    context: QueryContext,
    operation_id: String,
    interactive: bool,
    job_kind: &'static str,
    title: String,
    failed_step: &'static str,
    args: Vec<OsString>,
    remote_name: Option<String>,
}

pub trait NetworkProgressReporter: Send + Sync {
    fn report(&self, progress: NetworkProgress);
}

impl<F> NetworkProgressReporter for F
where
    F: Fn(NetworkProgress) + Send + Sync,
{
    fn report(&self, progress: NetworkProgress) {
        self(progress);
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct NoopNetworkProgressReporter;

impl NetworkProgressReporter for NoopNetworkProgressReporter {
    fn report(&self, _progress: NetworkProgress) {}
}

#[derive(Clone)]
pub struct GitTransportService {
    _database: Database,
    git: GitService,
    profiles: TransportProfileService,
    _store: TransportStore,
    jobs: JobService,
    operations: TransportOperationRegistry,
    write_locks: RepositoryWriteLocks,
    runner: Arc<dyn GitRunner>,
    repositories: RepositoryRepository,
    clones: CloneCoordinator,
}

impl GitTransportService {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        database: Database,
        git: GitService,
        profiles: TransportProfileService,
        jobs: JobService,
        operations: TransportOperationRegistry,
        write_locks: RepositoryWriteLocks,
        runner: Arc<dyn GitRunner>,
    ) -> Self {
        let clones = CloneCoordinator::new(
            database.clone(),
            git.clone(),
            profiles.clone(),
            jobs.clone(),
            operations.clone(),
            runner.clone(),
        );
        Self {
            _database: database.clone(),
            git,
            profiles,
            _store: TransportStore::new(database.clone()),
            jobs,
            operations,
            write_locks,
            runner,
            repositories: RepositoryRepository::new(database),
            clones,
        }
    }

    pub fn with_clone_support(
        mut self,
        intents: CloneIntentRegistry,
        provider: Option<Arc<dyn CloneProviderBinder>>,
    ) -> Self {
        self.clones = self.clones.with_support(intents, provider);
        self
    }

    pub fn clone_intents(&self) -> CloneIntentRegistry {
        self.clones.intents()
    }

    pub fn clone_repository(
        &self,
        input: CloneInput,
        reporter: Arc<dyn NetworkProgressReporter>,
    ) -> Result<CloneResult, AppError> {
        self.clones.clone_repository(input, reporter)
    }

    pub fn classify_clone_recovery(&self) -> Result<Vec<CloneRecoveryClassification>, AppError> {
        self.clones.classify_incomplete_recovery()
    }

    pub fn fetch(
        &self,
        input: FetchInput,
        reporter: Arc<dyn NetworkProgressReporter>,
    ) -> Result<NetworkOperationResult, AppError> {
        uuid::Uuid::parse_str(&input.operation_id).map_err(|_| {
            AppError::InvalidInput("transport operation id must be a UUID".to_owned())
        })?;
        self.git
            .validate_repository_context(&input.context, &input.repository_id)?;
        if !self
            .git
            .is_repository_trusted_in_context(&input.context, &input.repository_id)?
        {
            return Err(AppError::TrustRequired);
        }
        let remote_name = validate_remote_name(&input.remote_name)?;
        let remote = self
            .repositories
            .get_remote(&input.repository_id, remote_name)?;
        let remote_url = remote
            .fetch_url
            .as_deref()
            .ok_or_else(|| AppError::NotFound(format!("fetch URL for Remote {remote_name}")))?;
        let validated_url = validate_clone_url(remote_url)?;
        self.validate_effective_transport(&input.repository_id, &validated_url)?;

        self.execute_network(
            NetworkExecutionRequest {
                repository_id: input.repository_id,
                context: input.context,
                operation_id: input.operation_id,
                interactive: input.interactive,
                job_kind: "git.transport.fetch",
                title: format!("Fetch {remote_name}"),
                failed_step: "fetch",
                args: vec![
                    OsString::from("fetch"),
                    OsString::from("--progress"),
                    OsString::from("--"),
                    OsString::from(remote_name),
                ],
                remote_name: Some(remote.name),
            },
            reporter,
        )
    }

    pub fn pull(
        &self,
        input: PullInput,
        reporter: Arc<dyn NetworkProgressReporter>,
    ) -> Result<NetworkOperationResult, AppError> {
        validate_operation_id(&input.operation_id)?;
        self.ensure_trusted_context(&input.context, &input.repository_id)?;
        let state = self.network_state(&input.context, &input.repository_id)?;
        let upstream = pull_preflight(&state)?;
        let remote_name = validate_remote_name(&upstream.remote_name)?;
        let (_, remote_url) = self.resolve_remote(&input.repository_id, remote_name, false)?;
        self.validate_effective_transport(&input.repository_id, &remote_url)?;

        self.execute_network(
            NetworkExecutionRequest {
                repository_id: input.repository_id,
                context: input.context,
                operation_id: input.operation_id,
                interactive: input.interactive,
                job_kind: "git.transport.pull",
                title: format!("Pull {remote_name}"),
                failed_step: "pull",
                args: vec![
                    OsString::from("pull"),
                    OsString::from("--ff-only"),
                    OsString::from("--progress"),
                ],
                remote_name: Some(upstream.remote_name),
            },
            reporter,
        )
    }

    pub fn push(
        &self,
        input: PushInput,
        reporter: Arc<dyn NetworkProgressReporter>,
    ) -> Result<NetworkOperationResult, AppError> {
        validate_operation_id(&input.operation_id)?;
        self.ensure_trusted_context(&input.context, &input.repository_id)?;
        let state = self.network_state(&input.context, &input.repository_id)?;
        ensure_repository_can_write_network(&state)?;
        let (target, set_upstream) = match (state.upstream, input.target) {
            (Some(upstream), None) => (upstream, false),
            (None, Some(target)) => (
                UpstreamCandidate {
                    remote_name: target.remote_name,
                    branch_name: target.branch_name,
                },
                true,
            ),
            (None, None) => {
                return Err(AppError::Transport(TransportFailure::upstream_required()));
            }
            (Some(_), Some(_)) => {
                return Err(AppError::InvalidInput(
                    "push target is allowed only when no upstream exists".to_owned(),
                ));
            }
        };
        let remote_name = validate_remote_name(&target.remote_name)?;
        validate_branch_name_shape(&target.branch_name)?;
        let repository = self.repositories.get(&input.repository_id)?;
        self.validate_branch_with_git(&repository, &target.branch_name)?;
        let (_, remote_url) = self.resolve_remote(&input.repository_id, remote_name, true)?;
        self.validate_effective_transport(&input.repository_id, &remote_url)?;
        let refspec = format!("HEAD:refs/heads/{}", target.branch_name);
        let mut args = vec![OsString::from("push"), OsString::from("--progress")];
        if set_upstream {
            args.push(OsString::from("--set-upstream"));
        }
        args.extend([
            OsString::from("--"),
            OsString::from(remote_name),
            OsString::from(refspec),
        ]);

        self.execute_network(
            NetworkExecutionRequest {
                repository_id: input.repository_id,
                context: input.context,
                operation_id: input.operation_id,
                interactive: input.interactive,
                job_kind: "git.transport.push",
                title: format!("Push {remote_name}"),
                failed_step: "push",
                args,
                remote_name: Some(target.remote_name),
            },
            reporter,
        )
    }

    pub fn network_state(
        &self,
        context: &QueryContext,
        repository_id: &str,
    ) -> Result<RepositoryNetworkState, AppError> {
        self.git
            .validate_repository_context(context, repository_id)?;
        let record = self.git.get_snapshot(context, repository_id)?;
        let record = if record.snapshot.is_some() {
            record
        } else {
            self.git
                .refresh_repository_in_context(context, repository_id)?
        };
        let snapshot = required_refreshed_snapshot(&record)?;
        self.network_state_from_snapshot(context, repository_id, &snapshot)
    }

    fn ensure_trusted_context(
        &self,
        context: &QueryContext,
        repository_id: &str,
    ) -> Result<(), AppError> {
        self.git
            .validate_repository_context(context, repository_id)?;
        if !self
            .git
            .is_repository_trusted_in_context(context, repository_id)?
        {
            return Err(AppError::TrustRequired);
        }
        Ok(())
    }

    fn resolve_remote(
        &self,
        repository_id: &str,
        remote_name: &str,
        for_push: bool,
    ) -> Result<(Remote, ValidatedRemoteUrl), AppError> {
        let remote = self.repositories.get_remote(repository_id, remote_name)?;
        let url = if for_push {
            remote.push_url.as_deref().or(remote.fetch_url.as_deref())
        } else {
            remote.fetch_url.as_deref()
        }
        .ok_or_else(|| AppError::NotFound(format!("URL for Remote {remote_name}")))?;
        let validated = validate_clone_url(url)?;
        Ok((remote, validated))
    }

    fn validate_branch_with_git(
        &self,
        repository: &crate::git::model::Repository,
        branch_name: &str,
    ) -> Result<(), AppError> {
        let output = self.runner.run(GitCommand {
            repo: PathBuf::from(&repository.canonical_path),
            args: vec![
                OsString::from("check-ref-format"),
                OsString::from("--branch"),
                OsString::from(branch_name),
            ],
            stdin: None,
            timeout: Duration::from_secs(10),
        })?;
        if output.status.success() {
            Ok(())
        } else {
            Err(AppError::InvalidInput("invalid Git branch name".to_owned()))
        }
    }

    fn execute_network(
        &self,
        request: NetworkExecutionRequest,
        reporter: Arc<dyn NetworkProgressReporter>,
    ) -> Result<NetworkOperationResult, AppError> {
        let repository = self.repositories.get(&request.repository_id)?;
        let operation_guard = self.operations.register(
            &request.operation_id,
            format!("repository:{}", request.repository_id),
        )?;
        let repository_lock = self.write_locks.lock_for(&request.repository_id);
        let repository_guard = match repository_lock.try_lock() {
            Ok(guard) => guard,
            Err(std::sync::TryLockError::WouldBlock) => {
                return Err(AppError::Transport(TransportFailure::repository_busy()));
            }
            Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
        };

        let queued =
            self.jobs
                .create_with_id(&request.operation_id, request.job_kind, &request.title)?;
        let running = match self.jobs.start(&queued.id) {
            Ok(job) => job,
            Err(error) => {
                let _ = self.jobs.cancel(&queued.id);
                return Err(error);
            }
        };
        reporter.report(NetworkProgress {
            operation_id: request.operation_id.clone(),
            stage: NetworkStage::Transferring,
            fraction: None,
            objects: None,
            bytes: None,
        });
        let progress = Arc::new(ProgressBridge::new(
            request.operation_id.clone(),
            self.jobs.clone(),
            reporter.clone(),
        ));
        let mut context = GitRunContext::new(if request.interactive {
            GitExecutionPolicy::ForegroundNetworkInteractive
        } else {
            GitExecutionPolicy::BackgroundNetworkNonInteractive
        })
        .with_progress(progress);
        context.cancellation = operation_guard.cancellation();

        let execution = self.runner.run_with_context(
            GitCommand {
                repo: PathBuf::from(&repository.canonical_path),
                args: request.args,
                stdin: None,
                timeout: NETWORK_TIMEOUT,
            },
            context,
        );

        // GitService refreshes under the same shared RepositoryWriteLocks registry. The network
        // write guard must be dropped first while the Operation Registry guard remains alive.
        drop(repository_guard);
        let refresh = self
            .git
            .refresh_repository_in_context(&request.context, &request.repository_id);

        match execution {
            Ok(output) if output.status.success() => {}
            Ok(output) => {
                let failure = classify_git_failure(&output.stderr)
                    .with_operation(&request.operation_id)
                    .with_resource(&request.repository_id)
                    .with_failed_step(request.failed_step);
                self.finish_failed_job(&running.id, &failure, false)?;
                reporter.report(terminal_progress(
                    &request.operation_id,
                    NetworkStage::Failed,
                ));
                let _ = refresh;
                return Err(AppError::Transport(failure));
            }
            Err(error) => {
                let (failure, canceled) = classify_execution_error(error);
                let failure = failure
                    .with_operation(&request.operation_id)
                    .with_resource(&request.repository_id)
                    .with_failed_step(request.failed_step);
                self.finish_failed_job(&running.id, &failure, canceled)?;
                reporter.report(terminal_progress(
                    &request.operation_id,
                    if canceled {
                        NetworkStage::Cancelled
                    } else {
                        NetworkStage::Failed
                    },
                ));
                let _ = refresh;
                return Err(AppError::Transport(failure));
            }
        }

        let record = refresh
            .map_err(|_| self.partial_after_operation(&request.operation_id, &running.id))?;
        let snapshot = required_refreshed_snapshot(&record)
            .map_err(|_| self.partial_after_operation(&request.operation_id, &running.id))?;
        let network_state = self
            .network_state_from_snapshot(&request.context, &request.repository_id, &snapshot)
            .map_err(|_| self.partial_after_operation(&request.operation_id, &running.id))?;
        let job = self.jobs.succeed(&running.id)?;
        reporter.report(terminal_progress(
            &request.operation_id,
            NetworkStage::Completed,
        ));
        Ok(NetworkOperationResult {
            operation_id: request.operation_id,
            repository_id: request.repository_id,
            remote_name: request.remote_name,
            job,
            snapshot,
            network_state,
        })
    }

    fn validate_effective_transport(
        &self,
        repository_id: &str,
        remote: &ValidatedRemoteUrl,
    ) -> Result<(), AppError> {
        let effective = self.profiles.effective_for_repository(repository_id)?;
        if effective.drift_status == Some(TransportDriftStatus::Drifted) {
            return Err(AppError::Transport(
                TransportFailure::config_drift().with_resource(repository_id),
            ));
        }
        if effective.source == EffectiveTransportSource::Profile
            && effective.kind != Some(remote.kind)
        {
            return Err(AppError::Transport(
                TransportFailure::profile_mismatch().with_resource(repository_id),
            ));
        }
        Ok(())
    }

    fn finish_failed_job(
        &self,
        job_id: &str,
        failure: &TransportFailure,
        canceled: bool,
    ) -> Result<(), AppError> {
        if canceled {
            self.jobs.cancel(job_id)?;
        } else {
            self.jobs.fail(
                job_id,
                ErrorEnvelope::from(AppError::Transport(failure.clone())),
            )?;
        }
        Ok(())
    }

    fn partial_after_operation(&self, operation_id: &str, job_id: &str) -> AppError {
        let failure = TransportFailure::partial()
            .with_operation(operation_id)
            .with_failed_step("refresh");
        let _ = self.jobs.fail(
            job_id,
            ErrorEnvelope::from(AppError::Transport(failure.clone())),
        );
        AppError::Transport(failure)
    }

    fn network_state_from_snapshot(
        &self,
        context: &QueryContext,
        repository_id: &str,
        snapshot: &RepositorySnapshot,
    ) -> Result<RepositoryNetworkState, AppError> {
        self.git
            .validate_repository_context(context, repository_id)?;
        let repository = self.repositories.get(repository_id)?;
        let repository_path = PathBuf::from(&repository.canonical_path);
        let lock = self.write_locks.lock_for(repository_id);
        let _guard = match lock.try_lock() {
            Ok(guard) => guard,
            Err(std::sync::TryLockError::WouldBlock) => {
                return Err(AppError::Transport(TransportFailure::repository_busy()));
            }
            Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
        };
        let branch = optional_probe_line(
            self.runner.as_ref(),
            &repository_path,
            &["symbolic-ref", "--quiet", "--short", "HEAD"],
        )?;
        let upstream_name = optional_probe_line(
            self.runner.as_ref(),
            &repository_path,
            &[
                "rev-parse",
                "--abbrev-ref",
                "--symbolic-full-name",
                "@{upstream}",
            ],
        )?;
        let git_dir = required_probe_line(
            self.runner.as_ref(),
            &repository_path,
            &["rev-parse", "--absolute-git-dir"],
        )?;
        let git_dir = PathBuf::from(git_dir);
        if !git_dir.is_absolute() {
            return Err(AppError::InvalidInput(
                "Git directory probe was not absolute".to_owned(),
            ));
        }
        let in_progress = operation_in_progress(&git_dir);
        let remotes = self.repositories.list_remotes(repository_id)?;
        let upstream = upstream_name
            .as_deref()
            .and_then(|upstream| split_upstream(upstream, &remotes));
        let remotes = remotes
            .into_iter()
            .filter_map(sanitized_remote_summary)
            .collect();
        Ok(RepositoryNetworkState {
            repository_id: repository_id.to_owned(),
            branch: branch.clone(),
            detached: branch.is_none() && snapshot.head_oid.is_some(),
            upstream,
            remotes,
            ahead: snapshot.ahead,
            behind: snapshot.behind,
            conflicted_count: snapshot.conflicted_count,
            in_progress,
        })
    }
}

pub(super) struct ProgressBridge {
    operation_id: String,
    parser: Mutex<GitProgressParser>,
    jobs: JobService,
    reporter: Arc<dyn NetworkProgressReporter>,
    maximum_fraction: Mutex<f64>,
}

impl ProgressBridge {
    pub(super) fn new(
        operation_id: String,
        jobs: JobService,
        reporter: Arc<dyn NetworkProgressReporter>,
    ) -> Self {
        Self {
            operation_id,
            parser: Mutex::new(GitProgressParser::default()),
            jobs,
            reporter,
            maximum_fraction: Mutex::new(0.0),
        }
    }
}

impl GitProgressSink for ProgressBridge {
    fn stderr_chunk(&self, chunk: &[u8]) -> bool {
        let events = self.parser.lock().push(chunk);
        let observed_progress = !events.is_empty();
        for event in events {
            if let Some(fraction) = event.fraction {
                let mut maximum = self.maximum_fraction.lock();
                if fraction > *maximum {
                    *maximum = fraction;
                    let _ = self.jobs.set_progress(&self.operation_id, fraction);
                }
            }
            self.reporter.report(NetworkProgress {
                operation_id: self.operation_id.clone(),
                stage: event.stage,
                fraction: event.fraction,
                objects: event.objects,
                bytes: event.bytes,
            });
        }
        observed_progress
    }
}

fn required_refreshed_snapshot(
    record: &RepositoryScanRecord,
) -> Result<RepositorySnapshot, AppError> {
    if record.error.is_some() {
        return Err(AppError::Transport(TransportFailure::partial()));
    }
    record
        .snapshot
        .clone()
        .filter(|snapshot| snapshot.refresh_error_summary.is_none())
        .ok_or_else(|| AppError::Transport(TransportFailure::partial()))
}

pub(super) fn terminal_progress(operation_id: &str, stage: NetworkStage) -> NetworkProgress {
    NetworkProgress {
        operation_id: operation_id.to_owned(),
        stage,
        fraction: (stage == NetworkStage::Completed).then_some(1.0),
        objects: None,
        bytes: None,
    }
}

fn validate_operation_id(operation_id: &str) -> Result<(), AppError> {
    uuid::Uuid::parse_str(operation_id)
        .map(|_| ())
        .map_err(|_| AppError::InvalidInput("transport operation id must be a UUID".to_owned()))
}

fn ensure_repository_can_write_network(state: &RepositoryNetworkState) -> Result<(), AppError> {
    if state.detached {
        return Err(AppError::Transport(TransportFailure::detached_head()));
    }
    if state.conflicted_count > 0 || state.in_progress.is_some() {
        return Err(AppError::Transport(
            TransportFailure::operation_in_progress(),
        ));
    }
    Ok(())
}

fn pull_preflight(state: &RepositoryNetworkState) -> Result<UpstreamCandidate, AppError> {
    ensure_repository_can_write_network(state)?;
    state
        .upstream
        .clone()
        .ok_or_else(|| AppError::Transport(TransportFailure::upstream_required()))
}

fn validate_branch_name_shape(value: &str) -> Result<(), AppError> {
    if value.is_empty()
        || value.len() > 1024
        || value.starts_with(['-', '.'])
        || value.ends_with(['/', '.'])
        || value.ends_with(".lock")
        || value.contains("..")
        || value.contains("@{")
        || value.contains("//")
        || value.chars().any(|character| {
            let code = u32::from(character);
            code <= 0x20 || code == 0x7f || "~^:?*[\\".contains(character)
        })
    {
        return Err(AppError::InvalidInput("invalid Git branch name".to_owned()));
    }
    Ok(())
}

fn optional_probe_line(
    runner: &dyn GitRunner,
    repository: &std::path::Path,
    args: &[&str],
) -> Result<Option<String>, AppError> {
    let output = run_probe(runner, repository, args)?;
    if output.status.success() {
        return decode_probe_line(&output.stdout).map(Some);
    }
    if matches!(output.status.code(), Some(1 | 128)) {
        return Ok(None);
    }
    Err(AppError::Git("Git state probe failed".to_owned()))
}

fn required_probe_line(
    runner: &dyn GitRunner,
    repository: &std::path::Path,
    args: &[&str],
) -> Result<String, AppError> {
    let output = run_probe(runner, repository, args)?;
    if !output.status.success() {
        return Err(AppError::Git("Git state probe failed".to_owned()));
    }
    decode_probe_line(&output.stdout)
}

fn run_probe(
    runner: &dyn GitRunner,
    repository: &std::path::Path,
    args: &[&str],
) -> Result<crate::git::GitOutput, AppError> {
    runner.run(GitCommand {
        repo: repository.to_path_buf(),
        args: args.iter().map(OsString::from).collect(),
        stdin: None,
        timeout: Duration::from_secs(10),
    })
}

fn decode_probe_line(bytes: &[u8]) -> Result<String, AppError> {
    let value = std::str::from_utf8(bytes)
        .map_err(|_| AppError::InvalidInput("Git state probe is not UTF-8".to_owned()))?
        .trim_end_matches(['\r', '\n']);
    if value.is_empty() || value.contains(['\r', '\n', '\0']) {
        return Err(AppError::InvalidInput(
            "Git state probe returned malformed output".to_owned(),
        ));
    }
    Ok(value.to_owned())
}

fn operation_in_progress(git_dir: &std::path::Path) -> Option<RepositoryOperationInProgress> {
    if git_dir.join("MERGE_HEAD").is_file() {
        return Some(RepositoryOperationInProgress::Merge);
    }
    if git_dir.join("rebase-merge").is_dir() || git_dir.join("rebase-apply").is_dir() {
        return Some(RepositoryOperationInProgress::Rebase);
    }
    if git_dir.join("CHERRY_PICK_HEAD").is_file() {
        return Some(RepositoryOperationInProgress::CherryPick);
    }
    if git_dir.join("REVERT_HEAD").is_file() {
        return Some(RepositoryOperationInProgress::Revert);
    }
    if git_dir.join("BISECT_LOG").is_file() || git_dir.join("BISECT_START").is_file() {
        return Some(RepositoryOperationInProgress::Bisect);
    }
    None
}

fn validate_remote_name(value: &str) -> Result<&str, AppError> {
    if value.is_empty()
        || value.len() > 255
        || value.starts_with('-')
        || value.chars().any(|character| {
            let code = u32::from(character);
            code <= 0x20 || code == 0x7f || "~^:?*[\\".contains(character)
        })
    {
        return Err(AppError::InvalidInput("unsafe Git Remote name".to_owned()));
    }
    Ok(value)
}

pub(super) fn classify_execution_error(error: AppError) -> (TransportFailure, bool) {
    match error {
        AppError::Canceled => (TransportFailure::cancelled(), true),
        AppError::Timeout => (TransportFailure::timeout(), false),
        AppError::Transport(failure) => {
            let canceled = failure.code() == "git.transport.cancelled";
            (failure, canceled)
        }
        _ => (TransportFailure::network_unreachable(), false),
    }
}

pub(super) fn classify_git_failure(stderr: &[u8]) -> TransportFailure {
    let stderr = String::from_utf8_lossy(stderr).to_ascii_lowercase();
    if contains_any(
        &stderr,
        &[
            "non-fast-forward",
            "not possible to fast-forward",
            "fetch first",
            "failed to push some refs",
        ],
    ) {
        return TransportFailure::non_fast_forward();
    }
    if contains_any(
        &stderr,
        &[
            "authentication failed",
            "could not read username",
            "terminal prompts disabled",
            "no such device or address",
        ],
    ) {
        return TransportFailure::authentication_required();
    }
    if contains_any(
        &stderr,
        &["host key verification failed", "remote host identification"],
    ) {
        return TransportFailure::host_key_unverified();
    }
    if contains_any(
        &stderr,
        &[
            "permission denied",
            "access denied",
            "the requested url returned error: 403",
        ],
    ) {
        return TransportFailure::permission_denied();
    }
    if contains_any(
        &stderr,
        &["certificate", "ssl", "tls", "schannel", "secure channel"],
    ) {
        return TransportFailure::tls();
    }
    if contains_any(
        &stderr,
        &[
            "repository not found",
            "does not appear to be a git repository",
            "couldn't find remote ref",
        ],
    ) {
        return TransportFailure::remote_not_found();
    }
    TransportFailure::network_unreachable()
}

fn contains_any(value: &str, markers: &[&str]) -> bool {
    markers.iter().any(|marker| value.contains(marker))
}

fn split_upstream(upstream: &str, remotes: &[Remote]) -> Option<UpstreamCandidate> {
    remotes
        .iter()
        .filter_map(|remote| {
            upstream
                .strip_prefix(&format!("{}/", remote.name))
                .filter(|branch| !branch.is_empty())
                .map(|branch| UpstreamCandidate {
                    remote_name: remote.name.clone(),
                    branch_name: branch.to_owned(),
                })
        })
        .max_by_key(|candidate| candidate.remote_name.len())
}

fn sanitized_remote_summary(remote: Remote) -> Option<RepositoryRemoteSummary> {
    let fetch = remote.fetch_url.as_deref().and_then(sanitized_remote)?;
    let push_url = remote
        .push_url
        .as_deref()
        .and_then(sanitized_remote)
        .map(|(url, _)| url);
    Some(RepositoryRemoteSummary {
        name: remote.name,
        fetch_url: fetch.0,
        push_url,
        kind: fetch.1,
    })
}

fn sanitized_remote(value: &str) -> Option<(String, RemoteTransportKind)> {
    let remote = validate_clone_url(value).ok()?;
    Some((
        remote.sanitized_display,
        match remote.kind {
            TransportKind::Ssh => RemoteTransportKind::Ssh,
            TransportKind::Https => RemoteTransportKind::Https,
        },
    ))
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use chrono::Utc;

    use super::*;
    use crate::git::engine::GitOutput;
    use crate::git::model::{Project, Repository, RepositoryKind, Trust};
    use crate::git::repository::{ProjectRepository, TrustRepository};

    struct CountingRunner {
        runs: AtomicUsize,
    }

    impl GitRunner for CountingRunner {
        fn run(&self, _command: GitCommand) -> Result<GitOutput, AppError> {
            self.runs.fetch_add(1, Ordering::SeqCst);
            Err(AppError::Git("test runner must not start".to_owned()))
        }
    }

    struct ServiceFixture {
        _directory: tempfile::TempDir,
        service: GitTransportService,
        operations: TransportOperationRegistry,
        locks: RepositoryWriteLocks,
        runner: Arc<CountingRunner>,
        repository_id: String,
        context: QueryContext,
    }

    impl ServiceFixture {
        fn new(trusted: bool) -> Self {
            let directory = tempfile::tempdir().unwrap();
            let database = Database::open_in_memory().unwrap();
            let project = Project::new(&directory.path().to_string_lossy(), "Project");
            ProjectRepository::new(database.clone())
                .create(&project)
                .unwrap();
            let repository = Repository::new(
                &directory.path().to_string_lossy(),
                "Repository",
                RepositoryKind::Normal,
            );
            let repositories = RepositoryRepository::new(database.clone());
            repositories.create(&repository).unwrap();
            repositories
                .add_to_project(&project.id, &repository.id, ".")
                .unwrap();
            repositories
                .add_remote(&Remote {
                    repository_id: repository.id.clone(),
                    name: "origin".to_owned(),
                    fetch_url: Some("https://git.example.test/acme/repository.git".to_owned()),
                    push_url: None,
                })
                .unwrap();
            if trusted {
                TrustRepository::new(database.clone())
                    .set(&Trust {
                        repository_id: repository.id.clone(),
                        trusted_at: Utc::now(),
                        trust_version: 1,
                    })
                    .unwrap();
            }
            let runner = Arc::new(CountingRunner {
                runs: AtomicUsize::new(0),
            });
            let runner_trait: Arc<dyn GitRunner> = runner.clone();
            let locks = RepositoryWriteLocks::default();
            let git = GitService::with_runner_concurrency_and_write_locks(
                database.clone(),
                runner_trait.clone(),
                1,
                locks.clone(),
            );
            let profiles =
                TransportProfileService::new(database.clone(), locks.clone(), runner_trait.clone());
            let operations = TransportOperationRegistry::default();
            let service = GitTransportService::new(
                database.clone(),
                git,
                profiles,
                JobService::new(database.clone()),
                operations.clone(),
                locks.clone(),
                runner_trait,
            );
            Self {
                _directory: directory,
                service,
                operations,
                locks,
                runner,
                repository_id: repository.id,
                context: QueryContext::project(project.id),
            }
        }

        fn input(&self) -> FetchInput {
            FetchInput {
                repository_id: self.repository_id.clone(),
                context: self.context.clone(),
                remote_name: "origin".to_owned(),
                operation_id: uuid::Uuid::new_v4().to_string(),
                interactive: true,
            }
        }

        fn fetch(&self, input: FetchInput) -> Result<NetworkOperationResult, AppError> {
            self.service
                .fetch(input, Arc::new(NoopNetworkProgressReporter))
        }
    }

    #[test]
    fn fetch_rejects_untrusted_repository_before_spawning_git() {
        let fixture = ServiceFixture::new(false);
        assert!(matches!(
            fixture.fetch(fixture.input()),
            Err(AppError::TrustRequired)
        ));
        assert_eq!(fixture.runner.runs.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn fetch_rejects_unknown_and_option_looking_remotes_before_spawning_git() {
        let fixture = ServiceFixture::new(true);
        for remote_name in ["missing", "--upload-pack=evil"] {
            let mut input = fixture.input();
            input.remote_name = remote_name.to_owned();
            assert!(fixture.fetch(input).is_err());
        }
        assert_eq!(fixture.runner.runs.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn fetch_rejects_duplicate_operation_or_held_repository_lock_before_git() {
        let fixture = ServiceFixture::new(true);
        let input = fixture.input();
        let duplicate_id = input.operation_id.clone();
        let held_id = fixture
            .operations
            .register(&duplicate_id, "repository:other")
            .unwrap();
        assert!(matches!(
            fixture.fetch(input),
            Err(AppError::Transport(failure))
                if failure.code() == "git.transport.repository-busy"
        ));
        drop(held_id);

        let held_operation = fixture
            .operations
            .register(
                uuid::Uuid::new_v4().to_string(),
                format!("repository:{}", fixture.repository_id),
            )
            .unwrap();
        assert!(matches!(
            fixture.fetch(fixture.input()),
            Err(AppError::Transport(failure))
                if failure.code() == "git.transport.repository-busy"
        ));
        drop(held_operation);

        let lock = fixture.locks.lock_for(&fixture.repository_id);
        let _guard = lock.lock().unwrap();
        assert!(matches!(
            fixture.fetch(fixture.input()),
            Err(AppError::Transport(failure))
                if failure.code() == "git.transport.repository-busy"
        ));
        assert_eq!(fixture.runner.runs.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn git_failure_classification_discards_raw_remote_output() {
        let failure = classify_git_failure(
            b"fatal: Authentication failed for https://user:password@example.test/private.git",
        );
        assert_eq!(failure.code(), "git.transport.authentication-required");
        let debug = format!("{failure:?}");
        assert!(!debug.contains("password"));
        assert!(!debug.contains("private.git"));
    }

    fn ready_network_state() -> RepositoryNetworkState {
        RepositoryNetworkState {
            repository_id: "repository".to_owned(),
            branch: Some("main".to_owned()),
            detached: false,
            upstream: Some(UpstreamCandidate {
                remote_name: "origin".to_owned(),
                branch_name: "main".to_owned(),
            }),
            remotes: Vec::new(),
            ahead: 0,
            behind: 0,
            conflicted_count: 0,
            in_progress: None,
        }
    }

    #[test]
    fn pull_preflight_rejects_detached_missing_upstream_conflicts_and_operations() {
        let mut state = ready_network_state();
        state.detached = true;
        assert!(matches!(
            pull_preflight(&state),
            Err(AppError::Transport(failure))
                if failure.code() == "git.transport.detached-head"
        ));

        let mut state = ready_network_state();
        state.upstream = None;
        assert!(matches!(
            pull_preflight(&state),
            Err(AppError::Transport(failure))
                if failure.code() == "git.transport.upstream-required"
        ));

        let mut state = ready_network_state();
        state.conflicted_count = 1;
        assert!(matches!(
            pull_preflight(&state),
            Err(AppError::Transport(failure))
                if failure.code() == "git.transport.operation-in-progress"
        ));

        for operation in [
            RepositoryOperationInProgress::Merge,
            RepositoryOperationInProgress::Rebase,
            RepositoryOperationInProgress::CherryPick,
            RepositoryOperationInProgress::Revert,
            RepositoryOperationInProgress::Bisect,
        ] {
            let mut state = ready_network_state();
            state.in_progress = Some(operation);
            assert!(matches!(
                pull_preflight(&state),
                Err(AppError::Transport(failure))
                    if failure.code() == "git.transport.operation-in-progress"
            ));
        }
    }

    #[test]
    fn git_directory_markers_are_machine_readable_for_every_supported_operation() {
        for (marker, operation) in [
            ("MERGE_HEAD", RepositoryOperationInProgress::Merge),
            (
                "CHERRY_PICK_HEAD",
                RepositoryOperationInProgress::CherryPick,
            ),
            ("REVERT_HEAD", RepositoryOperationInProgress::Revert),
            ("BISECT_LOG", RepositoryOperationInProgress::Bisect),
        ] {
            let directory = tempfile::tempdir().unwrap();
            std::fs::write(directory.path().join(marker), "marker").unwrap();
            assert_eq!(operation_in_progress(directory.path()), Some(operation));
        }
        for marker in ["rebase-merge", "rebase-apply"] {
            let directory = tempfile::tempdir().unwrap();
            std::fs::create_dir(directory.path().join(marker)).unwrap();
            assert_eq!(
                operation_in_progress(directory.path()),
                Some(RepositoryOperationInProgress::Rebase)
            );
        }
    }

    #[test]
    fn push_branch_shape_has_no_option_or_refspec_escape_hatch() {
        assert!(validate_branch_name_shape("feature/safe").is_ok());
        for invalid in [
            "-force",
            ".hidden",
            "refs//heads",
            "topic..other",
            "topic@{upstream}",
            "topic.lock",
            "topic:refs/heads/evil",
        ] {
            assert!(validate_branch_name_shape(invalid).is_err(), "{invalid}");
        }
        assert_eq!(
            classify_git_failure(b"! [rejected] main -> main (fetch first)").code(),
            "git.transport.non-fast-forward"
        );
    }
}
