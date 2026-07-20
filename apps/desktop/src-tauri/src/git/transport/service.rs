use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;

use super::model::{
    EffectiveTransportSource, FetchInput, NetworkOperationResult, NetworkProgress, NetworkStage,
    RemoteTransportKind, RepositoryNetworkState, RepositoryRemoteSummary, TransportDriftStatus,
    TransportKind, UpstreamCandidate,
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

const FETCH_TIMEOUT: Duration = Duration::from_secs(15 * 60);

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
        }
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
        let repository = self.repositories.get(&input.repository_id)?;
        let remote = self
            .repositories
            .get_remote(&input.repository_id, remote_name)?;
        let remote_url = remote
            .fetch_url
            .as_deref()
            .ok_or_else(|| AppError::NotFound(format!("fetch URL for Remote {remote_name}")))?;
        let validated_url = validate_clone_url(remote_url)?;
        self.validate_effective_transport(&input.repository_id, &validated_url)?;

        let _operation = self.operations.register(
            &input.operation_id,
            format!("repository:{}", input.repository_id),
        )?;
        let repository_lock = self.write_locks.lock_for(&input.repository_id);
        let repository_guard = match repository_lock.try_lock() {
            Ok(guard) => guard,
            Err(std::sync::TryLockError::WouldBlock) => {
                return Err(AppError::Transport(TransportFailure::repository_busy()));
            }
            Err(std::sync::TryLockError::Poisoned(error)) => error.into_inner(),
        };

        let queued = self.jobs.create_with_id(
            &input.operation_id,
            "git.transport.fetch",
            &format!("Fetch {remote_name}"),
        )?;
        let running = match self.jobs.start(&queued.id) {
            Ok(job) => job,
            Err(error) => {
                let _ = self.jobs.cancel(&queued.id);
                return Err(error);
            }
        };
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
        let mut context = GitRunContext::new(if input.interactive {
            GitExecutionPolicy::ForegroundNetworkInteractive
        } else {
            GitExecutionPolicy::BackgroundNetworkNonInteractive
        })
        .with_progress(progress);
        context.cancellation = _operation.cancellation();

        let execution = self.runner.run_with_context(
            GitCommand {
                repo: PathBuf::from(&repository.canonical_path),
                args: vec![
                    OsString::from("fetch"),
                    OsString::from("--progress"),
                    OsString::from("--"),
                    OsString::from(remote_name),
                ],
                stdin: None,
                timeout: FETCH_TIMEOUT,
            },
            context,
        );

        // GitService refreshes under the same shared RepositoryWriteLocks registry. The network
        // write guard must be dropped first while the Operation Registry guard remains alive.
        drop(repository_guard);
        let refresh = self
            .git
            .refresh_repository_in_context(&input.context, &input.repository_id);

        let output = match execution {
            Ok(output) if output.status.success() => output,
            Ok(output) => {
                let failure = classify_git_failure(&output.stderr)
                    .with_operation(&input.operation_id)
                    .with_resource(&input.repository_id)
                    .with_failed_step("fetch");
                self.finish_failed_job(&running.id, &failure, false)?;
                reporter.report(terminal_progress(&input.operation_id, NetworkStage::Failed));
                let _ = refresh;
                return Err(AppError::Transport(failure));
            }
            Err(error) => {
                let (failure, canceled) = classify_execution_error(error);
                let failure = failure
                    .with_operation(&input.operation_id)
                    .with_resource(&input.repository_id)
                    .with_failed_step("fetch");
                self.finish_failed_job(&running.id, &failure, canceled)?;
                reporter.report(terminal_progress(
                    &input.operation_id,
                    if canceled {
                        NetworkStage::Cancelled
                    } else {
                        NetworkStage::Failed
                    },
                ));
                let _ = refresh;
                return Err(AppError::Transport(failure));
            }
        };
        drop(output);

        let record =
            refresh.map_err(|_| self.partial_after_fetch(&input.operation_id, &running.id))?;
        let snapshot = required_refreshed_snapshot(&record)
            .map_err(|_| self.partial_after_fetch(&input.operation_id, &running.id))?;
        let network_state =
            self.network_state_from_snapshot(&input.context, &input.repository_id, &snapshot)?;
        let job = self.jobs.succeed(&running.id)?;
        reporter.report(terminal_progress(
            &input.operation_id,
            NetworkStage::Completed,
        ));
        Ok(NetworkOperationResult {
            operation_id: input.operation_id,
            repository_id: input.repository_id,
            remote_name: Some(remote.name),
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

    fn partial_after_fetch(&self, operation_id: &str, job_id: &str) -> AppError {
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
        let remotes = self.repositories.list_remotes(repository_id)?;
        let upstream = snapshot
            .upstream
            .as_deref()
            .and_then(|upstream| split_upstream(upstream, &remotes));
        let remotes = remotes
            .into_iter()
            .filter_map(sanitized_remote_summary)
            .collect();
        Ok(RepositoryNetworkState {
            repository_id: repository_id.to_owned(),
            branch: snapshot.branch.clone(),
            detached: snapshot.branch.is_none() && snapshot.head_oid.is_some(),
            upstream,
            remotes,
            ahead: snapshot.ahead,
            behind: snapshot.behind,
            conflicted_count: snapshot.conflicted_count,
            in_progress: None,
        })
    }
}

struct ProgressBridge {
    operation_id: String,
    parser: Mutex<GitProgressParser>,
    jobs: JobService,
    reporter: Arc<dyn NetworkProgressReporter>,
    maximum_fraction: Mutex<f64>,
}

impl ProgressBridge {
    fn new(
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
    fn stderr_chunk(&self, chunk: &[u8]) {
        let events = self.parser.lock().push(chunk);
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

fn terminal_progress(operation_id: &str, stage: NetworkStage) -> NetworkProgress {
    NetworkProgress {
        operation_id: operation_id.to_owned(),
        stage,
        fraction: (stage == NetworkStage::Completed).then_some(1.0),
        objects: None,
        bytes: None,
    }
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

fn classify_execution_error(error: AppError) -> (TransportFailure, bool) {
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

fn classify_git_failure(stderr: &[u8]) -> TransportFailure {
    let stderr = String::from_utf8_lossy(stderr).to_ascii_lowercase();
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
}
