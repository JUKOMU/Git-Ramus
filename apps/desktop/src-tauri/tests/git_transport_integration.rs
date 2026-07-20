use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use chrono::Utc;
use futures_util::future::BoxFuture;
use git_ramus_desktop_lib::db::Database;
use git_ramus_desktop_lib::error::AppError;
use git_ramus_desktop_lib::git::engine::{
    GitCommand, GitOutput, GitRunContext, GitRunner, SystemGitRunner,
};
use git_ramus_desktop_lib::git::model::{Project, Remote, Repository, RepositoryKind, Trust};
use git_ramus_desktop_lib::git::repository::{
    ProjectRepository, RepositoryRepository, RepositoryWriteLocks, TrustRepository,
};
use git_ramus_desktop_lib::git::service::{GitService, QueryContext};
use git_ramus_desktop_lib::git::transport::clone::{
    CloneIntentBroker, CloneIntentRegistry, ClonePaths, CloneProviderBinder, CloneRecoveryAction,
    CloneRepositoryResolver, ConsumedCloneIntent,
};
use git_ramus_desktop_lib::git::transport::model::{
    CloneInput, CloneOperation, CloneProjectTarget, CloneSource, CloneStage,
    EffectiveTransportSource, FetchInput, NetworkProgress, NetworkStage, PullInput, PushInput,
    PushTarget, TransportKind,
};
use git_ramus_desktop_lib::git::transport::operation::{
    TransportAuthorizationDomain, TransportOperationRegistry,
};
use git_ramus_desktop_lib::git::transport::profile_service::{
    DriftResolution, ProfileDeletionResolution, TransportProfileService,
};
use git_ramus_desktop_lib::git::transport::service::{
    GitTransportService, NetworkProgressReporter, NoopNetworkProgressReporter,
};
use git_ramus_desktop_lib::git::transport::store::TransportStore;
use git_ramus_desktop_lib::jobs::JobService;
use git_ramus_desktop_lib::jobs::model::JobStatus;
use git_ramus_desktop_lib::providers::model::{
    ProviderKind, ProviderPermission, ProviderVisibility, RemoteRepository,
};
use tempfile::TempDir;

struct FailingConfigWriteRunner {
    inner: SystemGitRunner,
    fail_after: usize,
    writes: AtomicUsize,
}

#[derive(Default)]
struct RecordingNetworkProgressReporter {
    events: Mutex<Vec<NetworkProgress>>,
}

struct RecordingGitRunner {
    inner: Arc<dyn GitRunner>,
    commands: Arc<Mutex<Vec<Vec<String>>>>,
}

#[derive(Clone)]
enum CloneFault {
    CancelTransfer,
    CancelCheckout,
    FailRegistration { folder_name: String },
}

struct CloneFaultGitRunner {
    inner: Arc<dyn GitRunner>,
    fault: CloneFault,
}

impl CloneFaultGitRunner {
    fn is_clone_transfer(command: &GitCommand) -> bool {
        command
            .args
            .iter()
            .any(|argument| argument == "--no-checkout")
            && command.args.iter().any(|argument| argument == "clone")
    }

    fn is_clone_checkout(command: &GitCommand) -> bool {
        command.args.iter().any(|argument| argument == "checkout")
    }

    fn should_fail_registration(&self, command: &GitCommand) -> bool {
        let CloneFault::FailRegistration { folder_name } = &self.fault else {
            return false;
        };
        command
            .repo
            .file_name()
            .is_some_and(|name| name.to_string_lossy() == folder_name.as_str())
            && command.repo.join(".git").is_dir()
    }
}

impl GitRunner for CloneFaultGitRunner {
    fn run(&self, command: GitCommand) -> Result<GitOutput, AppError> {
        if self.should_fail_registration(&command) {
            return Err(AppError::Git(
                "injected Clone registration refresh failure".to_owned(),
            ));
        }
        self.inner.run(command)
    }

    fn run_with_context(
        &self,
        command: GitCommand,
        context: GitRunContext,
    ) -> Result<GitOutput, AppError> {
        if matches!(self.fault, CloneFault::CancelTransfer) && Self::is_clone_transfer(&command) {
            let staging = command
                .args
                .last()
                .map(std::path::PathBuf::from)
                .expect("Clone command has a Staging destination");
            std::fs::create_dir(&staging).expect("partial Staging directory creates");
            std::fs::write(staging.join("partial.pack"), "partial Clone")
                .expect("partial Clone data writes");
            return Err(AppError::Canceled);
        }
        if matches!(self.fault, CloneFault::CancelCheckout) && Self::is_clone_checkout(&command) {
            return Err(AppError::Canceled);
        }
        if self.should_fail_registration(&command) {
            return Err(AppError::Git(
                "injected Clone registration refresh failure".to_owned(),
            ));
        }
        self.inner.run_with_context(command, context)
    }
}

#[derive(Default)]
struct RecordingCloneProviderBinder {
    calls: Mutex<Vec<(String, String, String)>>,
    fail: std::sync::atomic::AtomicBool,
    cancel: Mutex<Option<(TransportOperationRegistry, String)>>,
}

impl CloneProviderBinder for RecordingCloneProviderBinder {
    fn bind_clone_remote(
        &self,
        repository_id: &str,
        remote_name: &str,
        intent: &ConsumedCloneIntent,
    ) -> Result<(), AppError> {
        if self.fail.load(Ordering::SeqCst) {
            return Err(AppError::Provider(
                git_ramus_desktop_lib::error::ProviderFailure::partial(),
            ));
        }
        self.calls.lock().unwrap().push((
            repository_id.to_owned(),
            remote_name.to_owned(),
            intent.repository.repository_id.clone(),
        ));
        if let Some((operations, operation_id)) = self.cancel.lock().unwrap().clone() {
            assert!(operations.cancel(&operation_id));
        }
        Ok(())
    }
}

struct RecordingCloneRepositoryResolver {
    repository: RemoteRepository,
    requests: Mutex<Vec<(String, String)>>,
}

impl CloneRepositoryResolver for RecordingCloneRepositoryResolver {
    fn repository_for_clone<'a>(
        &'a self,
        account_id: &'a str,
        repository_id: &'a str,
    ) -> BoxFuture<'a, Result<RemoteRepository, AppError>> {
        Box::pin(async move {
            self.requests
                .lock()
                .unwrap()
                .push((account_id.to_owned(), repository_id.to_owned()));
            Ok(self.repository.clone())
        })
    }
}

struct CloneFixture {
    base: TransportFixture,
    project_root: std::path::PathBuf,
    project_id: String,
    intent_id: String,
    https_profile_id: String,
    provider: Arc<RecordingCloneProviderBinder>,
    resolver: Arc<RecordingCloneRepositoryResolver>,
    hook_sentinel: std::path::PathBuf,
}

impl CloneFixture {
    fn with_provider_source_and_https_bare_remote() -> Self {
        Self::with_fault(None)
    }

    fn with_fault(fault: Option<CloneFault>) -> Self {
        let mut base = match fault {
            Some(fault) => TransportFixture::with_https_bare_remote_and_clone_fault(fault),
            None => TransportFixture::with_https_bare_remote(),
        };
        let hook_sentinel = base.install_checkout_attacks();
        let registry = CloneIntentRegistry::default();
        let resolver = Arc::new(RecordingCloneRepositoryResolver {
            repository: RemoteRepository {
                provider_kind: ProviderKind::Gitlab,
                instance_id: "provider-instance".to_owned(),
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
            },
            requests: Mutex::new(Vec::new()),
        });
        let broker = CloneIntentBroker::new(registry.clone(), resolver.clone());
        let intent = tokio::runtime::Runtime::new()
            .unwrap()
            .block_on(broker.create("git-ramus.provider-center", "provider-account", "42"))
            .unwrap();
        let profile = base
            .service
            .create_https_profile("Clone HTTPS", "creator")
            .unwrap();
        let provider = Arc::new(RecordingCloneProviderBinder::default());
        base.transport = base
            .transport
            .clone()
            .with_clone_support(registry, Some(provider.clone()));
        Self {
            project_root: base._directory.path().to_path_buf(),
            project_id: base.project_id.clone(),
            intent_id: intent.id,
            https_profile_id: profile.id,
            provider,
            resolver,
            hook_sentinel,
            base,
        }
    }

    fn progress(&self) -> Arc<dyn NetworkProgressReporter> {
        Arc::new(NoopNetworkProgressReporter)
    }

    fn local_git_config(&self, folder: &str, key: &str) -> Option<String> {
        let output = Command::new("git")
            .current_dir(self.project_root.join(folder))
            .args(["config", "--local", "--get", key])
            .output()
            .unwrap();
        output
            .status
            .success()
            .then(|| String::from_utf8(output.stdout).unwrap().trim().to_owned())
    }

    fn fail_provider_binding(&self) {
        self.provider.fail.store(true, Ordering::SeqCst);
    }

    fn cancel_during_provider_binding(&self, operation_id: &str) {
        *self.provider.cancel.lock().unwrap() =
            Some((self.base.operations.clone(), operation_id.to_owned()));
    }

    fn input(&self, folder_name: &str) -> CloneInput {
        self.input_at(self.project_root.clone(), folder_name)
    }

    fn input_at(&self, destination_parent: std::path::PathBuf, folder_name: &str) -> CloneInput {
        CloneInput {
            source: CloneSource::Intent(self.intent_id.clone()),
            transport_kind: TransportKind::Https,
            profile_id: Some(self.https_profile_id.clone()),
            destination_parent,
            folder_name: folder_name.to_owned(),
            project_target: CloneProjectTarget::Existing {
                project_id: self.project_id.clone(),
            },
            operation_id: uuid::Uuid::new_v4().to_string(),
            interactive: true,
        }
    }
}

fn seed_clone_recovery_operation(
    fixture: &TransportFixture,
    folder_name: &str,
) -> (String, ClonePaths) {
    let operation_id = uuid::Uuid::new_v4().to_string();
    let paths = ClonePaths::allocate(fixture._directory.path(), folder_name, &operation_id)
        .expect("recovery paths allocate");
    let jobs = JobService::new(fixture.database.clone());
    jobs.create_with_id(&operation_id, "git.transport.clone", "Interrupted Clone")
        .expect("recovery Job creates");
    jobs.start(&operation_id).expect("recovery Job starts");
    let now = Utc::now();
    TransportStore::new(fixture.database.clone())
        .insert_clone_operation(&CloneOperation {
            operation_id: operation_id.clone(),
            job_id: operation_id.clone(),
            source_summary: "git.example.test/acme/repository".to_owned(),
            intent_id: None,
            transport_profile_id: None,
            provider_instance_id: None,
            provider_account_id: None,
            provider_repository_id: None,
            staging_path: paths.staging.to_string_lossy().into_owned(),
            owner_marker_path: paths.marker.to_string_lossy().into_owned(),
            final_path: paths.final_path.to_string_lossy().into_owned(),
            project_target: CloneProjectTarget::Existing {
                project_id: fixture.project_id.clone(),
            },
            current_stage: CloneStage::Transferring,
            filesystem_complete: false,
            repository_id: None,
            project_id: Some(fixture.project_id.clone()),
            profile_applied: false,
            provider_binding_complete: false,
            created_at: now,
            updated_at: now,
        })
        .expect("recovery Operation persists");
    (operation_id, paths)
}

impl RecordingGitRunner {
    fn record(&self, command: &GitCommand) {
        self.commands.lock().unwrap().push(
            command
                .args
                .iter()
                .map(|argument| argument.to_string_lossy().into_owned())
                .collect(),
        );
    }
}

impl GitRunner for RecordingGitRunner {
    fn run(&self, command: GitCommand) -> Result<GitOutput, AppError> {
        self.record(&command);
        self.inner.run(command)
    }

    fn run_with_context(
        &self,
        command: GitCommand,
        context: GitRunContext,
    ) -> Result<GitOutput, AppError> {
        self.record(&command);
        self.inner.run_with_context(command, context)
    }
}

impl NetworkProgressReporter for RecordingNetworkProgressReporter {
    fn report(&self, progress: NetworkProgress) {
        self.events.lock().unwrap().push(progress);
    }
}

impl GitRunner for FailingConfigWriteRunner {
    fn run(&self, command: GitCommand) -> Result<GitOutput, AppError> {
        let is_write = command.args.iter().any(|argument| {
            matches!(
                argument.to_str(),
                Some("--replace-all") | Some("--unset-all")
            )
        });
        if is_write && self.writes.fetch_add(1, Ordering::SeqCst) >= self.fail_after {
            return Err(AppError::Git(
                "injected transport config write failure".to_owned(),
            ));
        }
        self.inner.run(command)
    }
}

struct TransportFixture {
    _directory: TempDir,
    repository_path: std::path::PathBuf,
    remote_writer_path: Option<std::path::PathBuf>,
    repository_id: String,
    project_id: String,
    database: Database,
    service: TransportProfileService,
    git: GitService,
    transport: GitTransportService,
    operations: TransportOperationRegistry,
    captured_git_commands: Arc<Mutex<Vec<Vec<String>>>>,
    global_config_path: std::path::PathBuf,
}

impl TransportFixture {
    fn new() -> Self {
        Self::with_optional_local_config(None)
    }

    fn with_local_config(key: &str, value: &str) -> Self {
        Self::with_optional_local_config(Some((key, value)))
    }

    fn with_optional_local_config(local_config: Option<(&str, &str)>) -> Self {
        Self::with_options(local_config, None, false, None)
    }

    fn with_failing_config_writes(fail_after: usize) -> Self {
        Self::with_options(None, Some(fail_after), false, None)
    }

    fn with_https_bare_remote() -> Self {
        let fixture = Self::with_options(None, None, true, None);
        fixture.trust();
        fixture
    }

    fn with_https_bare_remote_and_clone_fault(fault: CloneFault) -> Self {
        let fixture = Self::with_options(None, None, true, Some(fault));
        fixture.trust();
        fixture
    }

    fn with_untracked_local_branch() -> Self {
        let fixture = Self::with_https_bare_remote();
        run_git(
            &fixture.repository_path,
            &["checkout", "--quiet", "-b", "local-feature"],
        );
        fixture.commit_local("local-feature.txt");
        fixture
    }

    fn with_options(
        local_config: Option<(&str, &str)>,
        fail_config_writes_after: Option<usize>,
        with_bare_remote: bool,
        clone_fault: Option<CloneFault>,
    ) -> Self {
        let directory = tempfile::tempdir().expect("temporary transport fixture");
        let repository_path = directory.path().join("repository");
        let (remote_url, remote_writer_path, bare_remote_path) = if with_bare_remote {
            let bare_remote_path = directory.path().join("remote.git");
            let remote_writer_path = directory.path().join("remote-writer");
            std::fs::create_dir(&bare_remote_path).expect("Bare Remote directory");
            std::fs::create_dir(&remote_writer_path).expect("Remote writer directory");
            run_git(&bare_remote_path, &["init", "--bare", "--quiet"]);
            run_git(&remote_writer_path, &["init", "--quiet"]);
            run_git(
                &remote_writer_path,
                &["config", "user.name", "Git-Ramus Fixture"],
            );
            run_git(
                &remote_writer_path,
                &["config", "user.email", "fixture@git-ramus.invalid"],
            );
            std::fs::write(remote_writer_path.join("seed.txt"), "seed\n")
                .expect("seed file writes");
            run_git(&remote_writer_path, &["add", "--", "seed.txt"]);
            run_git(&remote_writer_path, &["commit", "--quiet", "-m", "seed"]);
            run_git(&remote_writer_path, &["branch", "-M", "main"]);
            run_git_strings(
                &remote_writer_path,
                &[
                    "remote".to_owned(),
                    "add".to_owned(),
                    "origin".to_owned(),
                    bare_remote_path.to_string_lossy().into_owned(),
                ],
            );
            run_git(
                &remote_writer_path,
                &["push", "--quiet", "-u", "origin", "main"],
            );
            run_git(
                &bare_remote_path,
                &["symbolic-ref", "HEAD", "refs/heads/main"],
            );
            run_git_strings(
                directory.path(),
                &[
                    "clone".to_owned(),
                    "--quiet".to_owned(),
                    bare_remote_path.to_string_lossy().into_owned(),
                    repository_path.to_string_lossy().into_owned(),
                ],
            );
            let remote_url = "https://git.example.test/acme/repository.git";
            run_git(
                &repository_path,
                &["remote", "set-url", "origin", remote_url],
            );
            (remote_url, Some(remote_writer_path), Some(bare_remote_path))
        } else {
            std::fs::create_dir(&repository_path).expect("repository directory");
            run_git(&repository_path, &["init", "--quiet"]);
            let remote_url = "https://gitlab.example/group/repository.git";
            run_git(&repository_path, &["remote", "add", "origin", remote_url]);
            (remote_url, None, None)
        };
        if let Some((key, value)) = local_config {
            run_git(&repository_path, &["config", "--local", key, value]);
        }

        let database = Database::open_in_memory().expect("database opens");
        let project_root = std::fs::canonicalize(directory.path())
            .expect("project root canonicalizes")
            .to_string_lossy()
            .into_owned();
        let project = Project::new(&project_root, "Transport fixture");
        ProjectRepository::new(database.clone())
            .create(&project)
            .expect("project persists");
        let canonical_path = std::fs::canonicalize(&repository_path)
            .expect("repository canonicalizes")
            .to_string_lossy()
            .into_owned();
        let repository =
            Repository::new(&canonical_path, "Transport fixture", RepositoryKind::Normal);
        let repositories = RepositoryRepository::new(database.clone());
        repositories
            .create(&repository)
            .expect("repository persists");
        repositories
            .add_to_project(&project.id, &repository.id, "repository")
            .expect("repository joins project");
        repositories
            .add_remote(&Remote {
                repository_id: repository.id.clone(),
                name: "origin".to_owned(),
                fetch_url: Some(remote_url.to_owned()),
                push_url: None,
            })
            .expect("remote persists");

        let home = directory.path().join("home");
        let xdg = directory.path().join("xdg");
        std::fs::create_dir_all(&home).expect("sealed home");
        std::fs::create_dir_all(&xdg).expect("sealed XDG home");
        let global_config = directory.path().join("global.gitconfig");
        let global_contents = bare_remote_path
            .as_ref()
            .map(|path| {
                let file_url = url::Url::from_file_path(path)
                    .expect("Bare Remote converts to file URL")
                    .to_string();
                format!(
                    "[url \"{file_url}\"]\n\tinsteadOf = {remote_url}\n[protocol \"file\"]\n\tallow = always\n"
                )
            })
            .unwrap_or_default();
        std::fs::write(&global_config, global_contents).expect("sealed global config");
        let runner = SystemGitRunner::new().with_sealed_config(home, xdg, global_config.clone());
        let runner: Arc<dyn GitRunner> = match fail_config_writes_after {
            Some(fail_after) => Arc::new(FailingConfigWriteRunner {
                inner: runner,
                fail_after,
                writes: AtomicUsize::new(0),
            }),
            None => Arc::new(runner),
        };
        let runner: Arc<dyn GitRunner> = match clone_fault {
            Some(fault) => Arc::new(CloneFaultGitRunner {
                inner: runner,
                fault,
            }),
            None => runner,
        };
        let captured_git_commands = Arc::new(Mutex::new(Vec::new()));
        let runner: Arc<dyn GitRunner> = Arc::new(RecordingGitRunner {
            inner: runner,
            commands: captured_git_commands.clone(),
        });
        let write_locks = RepositoryWriteLocks::default();
        let git = GitService::with_runner_concurrency_and_write_locks(
            database.clone(),
            runner.clone(),
            4,
            write_locks.clone(),
        );
        let service =
            TransportProfileService::new(database.clone(), write_locks.clone(), runner.clone());
        let operations = TransportOperationRegistry::default();
        let transport = GitTransportService::new(
            database.clone(),
            git.clone(),
            service.clone(),
            JobService::new(database.clone()),
            operations.clone(),
            write_locks,
            runner,
        );

        Self {
            _directory: directory,
            repository_path,
            remote_writer_path,
            repository_id: repository.id,
            project_id: project.id,
            database,
            service,
            git,
            transport,
            operations,
            captured_git_commands,
            global_config_path: global_config,
        }
    }

    fn trust(&self) {
        TrustRepository::new(self.database.clone())
            .set(&Trust {
                repository_id: self.repository_id.clone(),
                trusted_at: Utc::now(),
                trust_version: 1,
            })
            .expect("repository trusted");
    }

    fn git_config(&self, key: &str) -> Option<String> {
        let output = Command::new("git")
            .current_dir(&self.repository_path)
            .args(["config", "--local", "--get", key])
            .output()
            .expect("Git config read starts");
        if output.status.success() {
            Some(String::from_utf8(output.stdout).unwrap().trim().to_owned())
        } else {
            assert_eq!(output.status.code(), Some(1));
            None
        }
    }

    fn set_git_config(&self, key: &str, value: &str) {
        run_git(&self.repository_path, &["config", "--local", key, value]);
    }

    fn project_context(&self) -> QueryContext {
        QueryContext::project(self.project_id.clone())
    }

    fn advance_remote(&self, file_name: &str) {
        let writer = self
            .remote_writer_path
            .as_ref()
            .expect("fixture has a Bare Remote writer");
        std::fs::write(writer.join(file_name), "remote update\n").expect("Remote update writes");
        run_git_strings(
            writer,
            &["add".to_owned(), "--".to_owned(), file_name.to_owned()],
        );
        run_git(writer, &["commit", "--quiet", "-m", "remote update"]);
        run_git(writer, &["push", "--quiet", "origin", "main"]);
    }

    fn pull_input(&self) -> PullInput {
        PullInput {
            repository_id: self.repository_id.clone(),
            context: self.project_context(),
            operation_id: uuid::Uuid::new_v4().to_string(),
            interactive: true,
        }
    }

    fn push_input(&self) -> PushInput {
        PushInput {
            repository_id: self.repository_id.clone(),
            context: self.project_context(),
            target: None,
            operation_id: uuid::Uuid::new_v4().to_string(),
            interactive: true,
        }
    }

    fn commit_local(&self, file_name: &str) {
        run_git(
            &self.repository_path,
            &["config", "user.name", "Git-Ramus Fixture"],
        );
        run_git(
            &self.repository_path,
            &["config", "user.email", "fixture@git-ramus.invalid"],
        );
        std::fs::write(self.repository_path.join(file_name), "local update\n")
            .expect("local update writes");
        run_git_strings(
            &self.repository_path,
            &["add".to_owned(), "--".to_owned(), file_name.to_owned()],
        );
        run_git(
            &self.repository_path,
            &["commit", "--quiet", "-m", "local update"],
        );
    }

    fn head_oid(&self) -> String {
        git_stdout(&self.repository_path, &["rev-parse", "HEAD"])
    }

    fn git_dir(&self) -> std::path::PathBuf {
        self.repository_path.join(".git")
    }

    fn upstream(&self) -> Option<String> {
        let output = Command::new("git")
            .current_dir(&self.repository_path)
            .args([
                "rev-parse",
                "--abbrev-ref",
                "--symbolic-full-name",
                "@{upstream}",
            ])
            .output()
            .expect("Git upstream probe starts");
        output
            .status
            .success()
            .then(|| String::from_utf8(output.stdout).unwrap().trim().to_owned())
    }

    fn rewrite_remote_branch(&self, branch_name: &str) {
        let writer = self
            .remote_writer_path
            .as_ref()
            .expect("fixture has a Bare Remote writer");
        run_git_strings(
            writer,
            &[
                "checkout".to_owned(),
                "--quiet".to_owned(),
                "-B".to_owned(),
                branch_name.to_owned(),
                "main".to_owned(),
            ],
        );
        std::fs::write(writer.join("remote-rewrite.txt"), "remote rewrite\n")
            .expect("Remote rewrite writes");
        run_git(writer, &["add", "--", "remote-rewrite.txt"]);
        run_git(
            writer,
            &["commit", "--quiet", "-m", "rewrite remote branch"],
        );
        run_git_strings(
            writer,
            &[
                "push".to_owned(),
                "--quiet".to_owned(),
                "--force".to_owned(),
                "origin".to_owned(),
                branch_name.to_owned(),
            ],
        );
    }

    fn captured_git_args(&self) -> Vec<Vec<String>> {
        self.captured_git_commands.lock().unwrap().clone()
    }

    fn install_checkout_attacks(&self) -> std::path::PathBuf {
        let writer = self
            .remote_writer_path
            .as_ref()
            .expect("fixture has a Bare Remote writer");
        std::fs::write(writer.join(".gitattributes"), "*.txt filter=attack\n")
            .expect("attack attributes write");
        run_git(writer, &["add", "--", ".gitattributes"]);
        run_git(writer, &["commit", "--quiet", "-m", "add checkout attack"]);
        run_git(writer, &["push", "--quiet", "origin", "main"]);

        let hooks = self._directory.path().join("attacker-hooks");
        std::fs::create_dir(&hooks).unwrap();
        let sentinel = self._directory.path().join("hook-was-invoked.txt");
        let hook = hooks.join("post-checkout");
        std::fs::write(
            &hook,
            format!(
                "#!/bin/sh\nprintf invoked > '{}'\n",
                sentinel.to_string_lossy().replace('\\', "/")
            ),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&hook, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let mut config = std::fs::OpenOptions::new()
            .append(true)
            .open(&self.global_config_path)
            .unwrap();
        use std::io::Write as _;
        writeln!(
            config,
            "[filter \"attack\"]\n\tsmudge = git-ramus-filter-must-not-run\n\trequired = true\n[core]\n\thooksPath = {}",
            hooks.to_string_lossy().replace('\\', "/")
        )
        .unwrap();
        sentinel
    }
}

fn run_git(repository: &Path, arguments: &[&str]) {
    let output = Command::new("git")
        .current_dir(repository)
        .args(arguments)
        .output()
        .expect("Git starts");
    assert!(
        output.status.success(),
        "Git fixture command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run_git_strings(repository: &Path, arguments: &[String]) {
    let output = Command::new("git")
        .current_dir(repository)
        .args(arguments)
        .output()
        .expect("Git starts");
    assert!(
        output.status.success(),
        "Git fixture command failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn git_stdout(repository: &Path, arguments: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(repository)
        .args(arguments)
        .output()
        .expect("Git starts");
    assert!(output.status.success());
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

#[test]
fn profile_binding_requires_trust_and_external_git_reads_the_applied_profile() {
    let fixture = TransportFixture::new();
    let profile = fixture
        .service
        .create_https_profile("Work HTTPS", "creator")
        .unwrap();
    let error = fixture
        .service
        .bind_repository(&fixture.repository_id, &profile.id, false)
        .unwrap_err();
    assert!(matches!(error, AppError::TrustRequired));

    fixture.trust();
    fixture
        .service
        .bind_repository(&fixture.repository_id, &profile.id, false)
        .unwrap();
    assert_eq!(
        fixture.git_config("credential.useHttpPath").as_deref(),
        Some("true")
    );
    assert_eq!(
        fixture
            .service
            .effective_for_repository(&fixture.repository_id)
            .unwrap()
            .source,
        EffectiveTransportSource::Profile
    );
}

#[test]
fn profile_switch_then_unbind_restores_original_and_drift_blocks_restore() {
    let fixture = TransportFixture::with_local_config("credential.useHttpPath", "false");
    fixture.trust();
    let first = fixture.service.create_https_profile("One", "one").unwrap();
    let second = fixture.service.create_https_profile("Two", "two").unwrap();
    fixture
        .service
        .bind_repository(&fixture.repository_id, &first.id, true)
        .unwrap();
    fixture
        .service
        .bind_repository(&fixture.repository_id, &second.id, true)
        .unwrap();
    fixture
        .service
        .unbind_repository(&fixture.repository_id, DriftResolution::Reject)
        .unwrap();
    assert_eq!(
        fixture.git_config("credential.useHttpPath").as_deref(),
        Some("false")
    );

    fixture
        .service
        .bind_repository(&fixture.repository_id, &first.id, true)
        .unwrap();
    fixture.set_git_config("credential.useHttpPath", "external");
    let error = fixture
        .service
        .unbind_repository(&fixture.repository_id, DriftResolution::Reject)
        .unwrap_err();
    assert!(matches!(
        error,
        AppError::Transport(failure)
            if failure.code() == "git.transport.config-drift"
    ));
}

#[test]
fn profile_binding_requires_confirmation_before_replacing_custom_config() {
    let fixture = TransportFixture::with_local_config("credential.useHttpPath", "false");
    fixture.trust();
    let profile = fixture
        .service
        .create_https_profile("Work HTTPS", "creator")
        .unwrap();

    let error = fixture
        .service
        .bind_repository(&fixture.repository_id, &profile.id, false)
        .unwrap_err();
    assert!(matches!(error, AppError::UserActionRequired(_)));
    assert_eq!(
        fixture.git_config("credential.useHttpPath").as_deref(),
        Some("false")
    );
    assert_eq!(
        fixture
            .service
            .effective_for_repository(&fixture.repository_id)
            .unwrap()
            .source,
        EffectiveTransportSource::SystemGit
    );
}

#[test]
fn profile_binding_snapshots_and_restores_every_host_owned_config_key() {
    let fixture = TransportFixture::new();
    fixture.trust();
    let previous_key = "credential.https://gitlab.example/other/repository.git.username";
    fixture.set_git_config(previous_key, "previous");
    let profile = fixture
        .service
        .create_https_profile("Work HTTPS", "creator")
        .unwrap();

    assert!(matches!(
        fixture
            .service
            .bind_repository(&fixture.repository_id, &profile.id, false),
        Err(AppError::UserActionRequired(_))
    ));
    fixture
        .service
        .bind_repository(&fixture.repository_id, &profile.id, true)
        .unwrap();
    assert!(fixture.git_config(previous_key).is_none());

    fixture
        .service
        .unbind_repository(&fixture.repository_id, DriftResolution::Reject)
        .unwrap();
    assert_eq!(
        fixture.git_config(previous_key).as_deref(),
        Some("previous")
    );
}

#[test]
fn profile_update_allows_bound_display_rename_but_rejects_managed_field_changes() {
    let fixture = TransportFixture::new();
    fixture.trust();
    let profile = fixture
        .service
        .create_https_profile("Work HTTPS", "creator")
        .unwrap();
    fixture
        .service
        .bind_repository(&fixture.repository_id, &profile.id, false)
        .unwrap();

    let renamed = fixture
        .service
        .update_https_profile(&profile.id, "Renamed HTTPS", "creator")
        .unwrap();
    assert_eq!(renamed.display_name, "Renamed HTTPS");
    assert!(matches!(
        fixture
            .service
            .update_https_profile(&profile.id, "Renamed HTTPS", "different"),
        Err(AppError::UserActionRequired(_))
    ));
    assert_eq!(
        fixture
            .git_config("credential.https://gitlab.example/group/repository.git.username")
            .as_deref(),
        Some("creator")
    );
}

#[test]
fn profile_drift_can_be_kept_or_explicitly_reapplied() {
    let fixture = TransportFixture::with_local_config("credential.useHttpPath", "false");
    fixture.trust();
    let profile = fixture
        .service
        .create_https_profile("Work HTTPS", "creator")
        .unwrap();
    fixture
        .service
        .bind_repository(&fixture.repository_id, &profile.id, true)
        .unwrap();
    fixture.set_git_config("credential.useHttpPath", "external");
    fixture
        .service
        .unbind_repository(&fixture.repository_id, DriftResolution::Reapply)
        .unwrap();
    assert_eq!(
        fixture.git_config("credential.useHttpPath").as_deref(),
        Some("true")
    );
    assert_eq!(
        fixture
            .service
            .effective_for_repository(&fixture.repository_id)
            .unwrap()
            .source,
        EffectiveTransportSource::Profile
    );

    fixture.set_git_config("credential.useHttpPath", "keep-me");
    fixture
        .service
        .unbind_repository(&fixture.repository_id, DriftResolution::KeepExternal)
        .unwrap();
    assert_eq!(
        fixture.git_config("credential.useHttpPath").as_deref(),
        Some("keep-me")
    );
    assert_eq!(
        fixture
            .service
            .effective_for_repository(&fixture.repository_id)
            .unwrap()
            .source,
        EffectiveTransportSource::SystemGit
    );
}

#[test]
fn profile_binding_restores_config_when_binding_persistence_fails() {
    let fixture = TransportFixture::with_local_config("credential.useHttpPath", "false");
    fixture.trust();
    let profile = fixture
        .service
        .create_https_profile("Work HTTPS", "creator")
        .unwrap();
    fixture
        .database
        .with_connection(|connection| connection.execute_batch("PRAGMA query_only=ON"))
        .unwrap();

    let error = fixture
        .service
        .bind_repository(&fixture.repository_id, &profile.id, true)
        .unwrap_err();
    assert!(matches!(error, AppError::Database(_)));
    assert_eq!(
        fixture.git_config("credential.useHttpPath").as_deref(),
        Some("false")
    );
    assert!(
        fixture
            .git_config("credential.https://gitlab.example/group/repository.git.username")
            .is_none()
    );

    fixture
        .database
        .with_connection(|connection| connection.execute_batch("PRAGMA query_only=OFF"))
        .unwrap();
    assert_eq!(
        fixture
            .service
            .effective_for_repository(&fixture.repository_id)
            .unwrap()
            .source,
        EffectiveTransportSource::SystemGit
    );
}

#[test]
fn profile_restore_failure_creates_a_repair_and_returns_partial() {
    let fixture = TransportFixture::with_failing_config_writes(1);
    fixture.trust();
    let profile = fixture
        .service
        .create_https_profile("Work HTTPS", "creator")
        .unwrap();

    let error = fixture
        .service
        .bind_repository(&fixture.repository_id, &profile.id, false)
        .unwrap_err();
    assert!(matches!(
        error,
        AppError::Transport(failure) if failure.code() == "git.transport.partial"
    ));
    assert!(
        TransportStore::new(fixture.database.clone())
            .repository_has_unresolved_repair(&fixture.repository_id)
            .unwrap()
    );
}

#[test]
fn profile_deletion_requires_exact_resolutions_and_can_replace_a_binding() {
    let fixture = TransportFixture::new();
    fixture.trust();
    let first = fixture
        .service
        .create_https_profile("First", "first")
        .unwrap();
    let second = fixture
        .service
        .create_https_profile("Second", "second")
        .unwrap();
    fixture
        .service
        .bind_repository(&fixture.repository_id, &first.id, false)
        .unwrap();

    assert!(fixture.service.delete_profile(&first.id, &[]).is_err());
    fixture
        .service
        .delete_profile(
            &first.id,
            &[ProfileDeletionResolution::Replace {
                repository_id: fixture.repository_id.clone(),
                replacement_profile_id: second.id.clone(),
            }],
        )
        .unwrap();
    let profiles = fixture.service.list_profiles().unwrap();
    assert!(profiles.iter().all(|profile| profile.id != first.id));
    assert_eq!(
        fixture
            .service
            .effective_for_repository(&fixture.repository_id)
            .unwrap()
            .profile
            .unwrap()
            .id,
        second.id
    );
    assert_eq!(
        fixture
            .git_config("credential.https://gitlab.example/group/repository.git.username")
            .as_deref(),
        Some("second")
    );
}

#[test]
fn fetch_updates_remote_refs_and_persisted_ahead_behind() {
    let fixture = TransportFixture::with_https_bare_remote();
    fixture.advance_remote("remote-only.txt");
    let progress = Arc::new(RecordingNetworkProgressReporter::default());
    let result = fixture
        .transport
        .fetch(
            FetchInput {
                repository_id: fixture.repository_id.clone(),
                context: fixture.project_context(),
                remote_name: "origin".to_owned(),
                operation_id: uuid::Uuid::new_v4().to_string(),
                interactive: true,
            },
            progress.clone(),
        )
        .unwrap();
    assert_eq!(result.remote_name.as_deref(), Some("origin"));
    assert_eq!(result.job.status, JobStatus::Succeeded);
    assert_eq!(
        result.network_state.remotes[0].fetch_url,
        "https://git.example.test/acme/repository.git"
    );
    let events = progress.events.lock().unwrap();
    assert_eq!(events[0].stage, NetworkStage::Validating);
    assert_eq!(events[1].stage, NetworkStage::AwaitingAuthentication);
    assert_eq!(events[2].stage, NetworkStage::Transferring);
    assert_eq!(events.last().unwrap().stage, NetworkStage::Completed);
    assert!(
        events
            .iter()
            .all(|event| event.operation_id == result.operation_id)
    );
    drop(events);
    let serialized = serde_json::to_string(&result).unwrap();
    assert!(!serialized.contains("file://"));
    assert!(!serialized.contains(fixture._directory.path().to_string_lossy().as_ref()));
    let snapshot = fixture
        .git
        .get_snapshot(&fixture.project_context(), &fixture.repository_id)
        .unwrap()
        .snapshot
        .unwrap();
    assert_eq!(snapshot.behind, 1);
}

#[test]
fn reserved_fetch_honors_cancellation_before_preflight_without_starting_git_or_a_job() {
    let fixture = TransportFixture::with_https_bare_remote();
    let operation_id = uuid::Uuid::new_v4().to_string();
    let operation_guard = fixture
        .transport
        .reserve_operation(
            operation_id.clone(),
            "plugin.owner",
            TransportAuthorizationDomain::Repositories,
        )
        .unwrap();
    assert!(
        fixture
            .transport
            .cancel_owned_operation(
                &operation_id,
                "plugin.owner",
                TransportAuthorizationDomain::Repositories,
            )
            .unwrap()
    );
    let command_count = fixture.captured_git_commands.lock().unwrap().len();

    let error = fixture
        .transport
        .fetch_reserved(
            FetchInput {
                repository_id: fixture.repository_id.clone(),
                context: fixture.project_context(),
                remote_name: "origin".to_owned(),
                operation_id: operation_id.clone(),
                interactive: true,
            },
            Arc::new(NoopNetworkProgressReporter),
            operation_guard,
        )
        .unwrap_err();

    assert!(matches!(
        error,
        AppError::Transport(failure) if failure.code() == "git.transport.cancelled"
    ));
    assert_eq!(
        fixture.captured_git_commands.lock().unwrap().len(),
        command_count
    );
    assert!(
        JobService::new(fixture.database.clone())
            .list()
            .unwrap()
            .iter()
            .all(|job| job.id != operation_id)
    );
    assert!(
        fixture
            .transport
            .operation_authorization(&operation_id)
            .is_none()
    );
}

#[test]
fn fetch_rejects_a_remote_that_mismatches_the_bound_profile() {
    let fixture = TransportFixture::with_https_bare_remote();
    let profile = fixture
        .service
        .create_https_profile("Work HTTPS", "creator")
        .unwrap();
    fixture
        .service
        .bind_repository(&fixture.repository_id, &profile.id, false)
        .unwrap();
    RepositoryRepository::new(fixture.database.clone())
        .add_remote(&Remote {
            repository_id: fixture.repository_id.clone(),
            name: "origin".to_owned(),
            fetch_url: Some("git@git.example.test:acme/repository.git".to_owned()),
            push_url: None,
        })
        .unwrap();

    let error = fixture
        .transport
        .fetch(
            FetchInput {
                repository_id: fixture.repository_id.clone(),
                context: fixture.project_context(),
                remote_name: "origin".to_owned(),
                operation_id: uuid::Uuid::new_v4().to_string(),
                interactive: true,
            },
            Arc::new(NoopNetworkProgressReporter),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        AppError::Transport(failure)
            if failure.code() == "git.transport.profile-mismatch"
    ));
}

#[test]
fn pull_fast_forwards_but_divergence_never_creates_a_merge_or_rebase() {
    let fixture = TransportFixture::with_https_bare_remote();
    fixture.advance_remote("remote.txt");
    fixture
        .transport
        .pull(fixture.pull_input(), Arc::new(NoopNetworkProgressReporter))
        .unwrap();
    assert!(fixture.repository_path.join("remote.txt").is_file());

    fixture.commit_local("local.txt");
    fixture.advance_remote("other.txt");
    let before = fixture.head_oid();
    let error = fixture
        .transport
        .pull(fixture.pull_input(), Arc::new(NoopNetworkProgressReporter))
        .unwrap_err();
    assert!(matches!(
        error,
        AppError::Transport(failure)
            if failure.code() == "git.transport.non-fast-forward"
    ));
    assert_eq!(fixture.head_oid(), before);
    assert!(!fixture.git_dir().join("MERGE_HEAD").exists());
    assert!(!fixture.git_dir().join("rebase-merge").exists());
    assert!(!fixture.git_dir().join("rebase-apply").exists());
    assert!(fixture.captured_git_args().contains(&vec![
        "pull".to_owned(),
        "--ff-only".to_owned(),
        "--progress".to_owned(),
    ]));
    assert!(
        fixture
            .captured_git_args()
            .iter()
            .flatten()
            .all(|argument| argument != "stash" && argument != "--rebase")
    );
}

#[test]
fn push_sets_upstream_once_and_rejects_non_fast_forward_without_force() {
    let fixture = TransportFixture::with_untracked_local_branch();
    fixture
        .transport
        .push(
            PushInput {
                target: Some(PushTarget {
                    remote_name: "origin".to_owned(),
                    branch_name: "feature/safe".to_owned(),
                }),
                ..fixture.push_input()
            },
            Arc::new(NoopNetworkProgressReporter),
        )
        .unwrap();
    assert_eq!(fixture.upstream().as_deref(), Some("origin/feature/safe"));

    fixture.rewrite_remote_branch("feature/safe");
    let error = fixture
        .transport
        .push(
            PushInput {
                target: None,
                ..fixture.push_input()
            },
            Arc::new(NoopNetworkProgressReporter),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        AppError::Transport(failure)
            if failure.code() == "git.transport.non-fast-forward"
    ));
    assert!(
        fixture
            .captured_git_args()
            .iter()
            .flatten()
            .all(|argument| { argument != "--force" && argument != "--force-with-lease" })
    );
    let commands = fixture.captured_git_args();
    assert!(commands.contains(&vec![
        "push".to_owned(),
        "--progress".to_owned(),
        "--set-upstream".to_owned(),
        "--".to_owned(),
        "origin".to_owned(),
        "HEAD:refs/heads/feature/safe".to_owned(),
    ]));
    assert!(commands.contains(&vec![
        "push".to_owned(),
        "--progress".to_owned(),
        "--".to_owned(),
        "origin".to_owned(),
        "HEAD:refs/heads/feature/safe".to_owned(),
    ]));
}

#[test]
fn reserved_clone_honors_cancellation_before_preflight_without_consuming_the_intent() {
    let fixture = CloneFixture::with_provider_source_and_https_bare_remote();
    let input = fixture.input("cancel-before-preflight");
    let operation_id = input.operation_id.clone();
    let operation_guard = fixture
        .base
        .transport
        .reserve_operation(
            operation_id.clone(),
            "plugin.owner",
            TransportAuthorizationDomain::CloneIntents,
        )
        .unwrap();
    assert!(
        fixture
            .base
            .transport
            .cancel_owned_operation(
                &operation_id,
                "plugin.owner",
                TransportAuthorizationDomain::CloneIntents,
            )
            .unwrap()
    );

    let error = fixture
        .base
        .transport
        .clone_repository_reserved(input, fixture.progress(), operation_guard)
        .unwrap_err();

    assert!(matches!(
        error,
        AppError::Transport(failure) if failure.code() == "git.transport.cancelled"
    ));
    assert!(
        fixture
            .base
            .transport
            .clone_intents()
            .get(&fixture.intent_id)
            .is_ok()
    );
    assert!(
        !fixture
            .project_root
            .join("cancel-before-preflight")
            .exists()
    );
    assert!(
        JobService::new(fixture.base.database.clone())
            .list()
            .unwrap()
            .iter()
            .all(|job| job.id != operation_id)
    );
}

#[test]
fn clone_uses_staging_registers_project_and_applies_profile_without_leaking_provider_pat() {
    let fixture = CloneFixture::with_provider_source_and_https_bare_remote();
    let result = fixture
        .base
        .transport
        .clone_repository(
            CloneInput {
                source: CloneSource::Intent(fixture.intent_id.clone()),
                transport_kind: TransportKind::Https,
                profile_id: Some(fixture.https_profile_id.clone()),
                destination_parent: fixture.project_root.clone(),
                folder_name: "cloned-repository".to_owned(),
                project_target: CloneProjectTarget::Existing {
                    project_id: fixture.project_id.clone(),
                },
                operation_id: uuid::Uuid::new_v4().to_string(),
                interactive: true,
            },
            fixture.progress(),
        )
        .unwrap();
    assert!(fixture.project_root.join("cloned-repository/.git").is_dir());
    assert_eq!(result.project.id, fixture.project_id);
    assert_eq!(
        fixture
            .local_git_config("cloned-repository", "credential.useHttpPath")
            .as_deref(),
        Some("true")
    );
    assert_eq!(fixture.provider.calls.lock().unwrap().len(), 1);
    assert_eq!(
        fixture.resolver.requests.lock().unwrap().as_slice(),
        &[("provider-account".to_owned(), "42".to_owned())]
    );
    assert!(!fixture.hook_sentinel.exists());
    let serialized = serde_json::to_string(&result).unwrap();
    assert!(!serialized.contains("provider-pat-fixture"));
    assert!(!serialized.contains(fixture.project_root.to_string_lossy().as_ref()));
    assert!(
        fixture
            .base
            .captured_git_args()
            .iter()
            .any(|args| args.iter().any(|argument| argument == "--no-checkout"))
    );
    assert!(fixture.base.captured_git_args().iter().all(|args| {
        !args
            .iter()
            .any(|argument| argument.contains("provider-pat-fixture"))
    }));
    let operation = TransportStore::new(fixture.base.database.clone())
        .get_clone_operation(&result.operation_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        operation.transport_profile_id.as_deref(),
        Some(fixture.https_profile_id.as_str())
    );
    assert_eq!(
        operation.provider_instance_id.as_deref(),
        Some("provider-instance")
    );
    assert_eq!(
        operation.provider_account_id.as_deref(),
        Some("provider-account")
    );
    assert_eq!(operation.provider_repository_id.as_deref(), Some("42"));
}

#[test]
fn deleting_a_project_detaches_completed_clone_journal_history() {
    let fixture = CloneFixture::with_provider_source_and_https_bare_remote();
    let result = fixture
        .base
        .transport
        .clone_repository(fixture.input("deletable-clone"), fixture.progress())
        .unwrap();
    let store = TransportStore::new(fixture.base.database.clone());
    assert_eq!(
        store
            .get_clone_operation(&result.operation_id)
            .unwrap()
            .unwrap()
            .project_id
            .as_deref(),
        Some(fixture.project_id.as_str())
    );

    fixture
        .base
        .git
        .delete_project_by_id(&fixture.project_id)
        .unwrap();

    assert!(fixture.base.git.get_project(&fixture.project_id).is_err());
    assert_eq!(
        store
            .get_clone_operation(&result.operation_id)
            .unwrap()
            .unwrap()
            .project_id,
        None
    );
}

#[test]
fn clone_can_create_a_new_project_root_and_register_the_repository_at_depth_zero() {
    let fixture = CloneFixture::with_provider_source_and_https_bare_remote();
    let mut input = fixture.input("new-project-repository");
    input.project_target = CloneProjectTarget::New {
        name: "New Clone Project".to_owned(),
    };
    let result = fixture
        .base
        .transport
        .clone_repository(input, fixture.progress())
        .unwrap();
    let final_path = fixture.project_root.join("new-project-repository");
    let project = ProjectRepository::new(fixture.base.database.clone())
        .get(&result.project.id)
        .unwrap();
    assert_ne!(project.id, fixture.project_id);
    assert_eq!(project.scan_depth, 0);
    assert_eq!(
        dunce::canonicalize(&project.root_path).unwrap(),
        dunce::canonicalize(&final_path).unwrap()
    );
    assert!(final_path.join(".git").is_dir());
    assert_eq!(
        TransportStore::new(fixture.base.database.clone())
            .get_clone_operation(&result.operation_id)
            .unwrap()
            .unwrap()
            .project_id
            .as_deref(),
        Some(project.id.as_str())
    );
}

#[test]
fn clone_provider_binding_failure_is_partial_and_never_deletes_the_final_repository() {
    let fixture = CloneFixture::with_provider_source_and_https_bare_remote();
    fixture.fail_provider_binding();
    let operation_id = uuid::Uuid::new_v4().to_string();
    let progress = Arc::new(RecordingNetworkProgressReporter::default());
    let error = fixture
        .base
        .transport
        .clone_repository(
            CloneInput {
                source: CloneSource::Intent(fixture.intent_id.clone()),
                transport_kind: TransportKind::Https,
                profile_id: Some(fixture.https_profile_id.clone()),
                destination_parent: fixture.project_root.clone(),
                folder_name: "partial-clone".to_owned(),
                project_target: CloneProjectTarget::Existing {
                    project_id: fixture.project_id.clone(),
                },
                operation_id: operation_id.clone(),
                interactive: true,
            },
            progress.clone(),
        )
        .unwrap_err();
    assert!(matches!(
        error,
        AppError::Transport(failure) if failure.code() == "git.transport.partial"
    ));
    assert!(fixture.project_root.join("partial-clone/.git").is_dir());
    let operation = TransportStore::new(fixture.base.database.clone())
        .get_clone_operation(&operation_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        operation.current_stage,
        git_ramus_desktop_lib::git::transport::model::CloneStage::Partial
    );
    assert!(operation.filesystem_complete);
    assert!(operation.repository_id.is_some());
    assert!(operation.profile_applied);
    assert!(!operation.provider_binding_complete);
    assert!(std::path::Path::new(&operation.owner_marker_path).is_file());
    let events = progress.events.lock().unwrap();
    assert_eq!(events[0].stage, NetworkStage::Validating);
    assert_eq!(events[1].stage, NetworkStage::AwaitingAuthentication);
    assert_eq!(events[2].stage, NetworkStage::Transferring);
    assert_eq!(events.last().unwrap().stage, NetworkStage::Partial);
    drop(events);
    let recovery = fixture.base.transport.classify_clone_recovery().unwrap();
    assert_eq!(recovery.len(), 1);
    assert_eq!(
        recovery[0].actions,
        vec![CloneRecoveryAction::RetryRegistration]
    );
    assert!(
        fixture
            .base
            .git
            .delete_project_by_id(&fixture.project_id)
            .is_err()
    );
    assert_eq!(
        TransportStore::new(fixture.base.database.clone())
            .get_clone_operation(&operation_id)
            .unwrap()
            .unwrap()
            .project_id
            .as_deref(),
        Some(fixture.project_id.as_str())
    );
}

#[test]
fn clone_cancellation_after_final_rename_is_partial_and_cannot_report_success() {
    let fixture = CloneFixture::with_provider_source_and_https_bare_remote();
    let input = fixture.input("cancelled-after-rename");
    let operation_id = input.operation_id.clone();
    fixture.cancel_during_provider_binding(&operation_id);

    let error = fixture
        .base
        .transport
        .clone_repository(input, fixture.progress())
        .unwrap_err();
    assert!(matches!(
        error,
        AppError::Transport(failure) if failure.code() == "git.transport.partial"
    ));

    let final_path = fixture.project_root.join("cancelled-after-rename");
    assert!(final_path.join(".git").is_dir());
    let operation = TransportStore::new(fixture.base.database.clone())
        .get_clone_operation(&operation_id)
        .unwrap()
        .unwrap();
    assert_eq!(operation.current_stage, CloneStage::Partial);
    assert!(operation.filesystem_complete);
    assert!(operation.repository_id.is_some());
    assert!(operation.profile_applied);
    assert!(!operation.provider_binding_complete);
    assert!(std::path::Path::new(&operation.owner_marker_path).is_file());
    let job = JobService::new(fixture.base.database.clone())
        .list()
        .unwrap()
        .into_iter()
        .find(|job| job.id == operation_id)
        .unwrap();
    assert_eq!(job.status, JobStatus::Failed);
}

#[test]
fn clone_rejects_unsupported_urls_before_creating_a_job_or_destination() {
    let fixture = TransportFixture::with_https_bare_remote();
    let operation_id = uuid::Uuid::new_v4().to_string();
    let final_path = fixture._directory.path().join("unsupported-url");
    let error = fixture
        .transport
        .clone_repository(
            CloneInput {
                source: CloneSource::Manual("file:///private/repository.git".to_owned()),
                transport_kind: TransportKind::Https,
                profile_id: None,
                destination_parent: fixture._directory.path().to_path_buf(),
                folder_name: "unsupported-url".to_owned(),
                project_target: CloneProjectTarget::Existing {
                    project_id: fixture.project_id.clone(),
                },
                operation_id: operation_id.clone(),
                interactive: true,
            },
            Arc::new(NoopNetworkProgressReporter),
        )
        .unwrap_err();
    assert!(matches!(error, AppError::InvalidInput(_)));
    assert!(!final_path.exists());
    assert!(
        TransportStore::new(fixture.database.clone())
            .get_clone_operation(&operation_id)
            .unwrap()
            .is_none()
    );
    assert!(
        JobService::new(fixture.database.clone())
            .list()
            .unwrap()
            .iter()
            .all(|job| job.id != operation_id)
    );
}

#[cfg(unix)]
#[test]
fn clone_rejects_non_utf8_persisted_paths_without_leaving_a_job() {
    use std::os::unix::ffi::OsStringExt;

    let fixture = TransportFixture::with_https_bare_remote();
    let parent = fixture
        ._directory
        .path()
        .join(std::ffi::OsString::from_vec(b"non-utf8-\xff".to_vec()));
    std::fs::create_dir(&parent).unwrap();
    let operation_id = uuid::Uuid::new_v4().to_string();
    let error = fixture
        .transport
        .clone_repository(
            CloneInput {
                source: CloneSource::Manual(
                    "https://git.example.test/acme/repository.git".to_owned(),
                ),
                transport_kind: TransportKind::Https,
                profile_id: None,
                destination_parent: parent,
                folder_name: "repository".to_owned(),
                project_target: CloneProjectTarget::New {
                    name: "Non-UTF8".to_owned(),
                },
                operation_id: operation_id.clone(),
                interactive: false,
            },
            Arc::new(NoopNetworkProgressReporter),
        )
        .unwrap_err();
    assert!(matches!(error, AppError::NonUtf8Path));
    assert!(
        JobService::new(fixture.database.clone())
            .list()
            .unwrap()
            .is_empty()
    );
    assert!(
        TransportStore::new(fixture.database.clone())
            .get_clone_operation(&operation_id)
            .unwrap()
            .is_none()
    );
}

#[test]
fn clone_validates_project_scan_rules_and_existing_destinations_before_consuming_intent() {
    let fixture = CloneFixture::with_provider_source_and_https_bare_remote();
    let projects = ProjectRepository::new(fixture.base.database.clone());
    let mut project = projects.get(&fixture.project_id).unwrap();
    let nested = fixture.project_root.join("nested");
    std::fs::create_dir(&nested).unwrap();

    project.scan_depth = 1;
    project.updated_at = Utc::now();
    projects.update(&project).unwrap();
    let too_deep = fixture
        .base
        .transport
        .clone_repository(fixture.input_at(nested, "too-deep"), fixture.progress())
        .unwrap_err();
    assert!(matches!(
        too_deep,
        AppError::InvalidInput(message) if message.contains("scan depth")
    ));

    project.scan_depth = 3;
    project.exclude_patterns = vec!["blocked*".to_owned()];
    project.updated_at = Utc::now();
    projects.update(&project).unwrap();
    let excluded = fixture
        .base
        .transport
        .clone_repository(fixture.input("blocked-repository"), fixture.progress())
        .unwrap_err();
    assert!(matches!(
        excluded,
        AppError::InvalidInput(message) if message.contains("excluded")
    ));

    let occupied = fixture.project_root.join("occupied-repository");
    std::fs::create_dir(&occupied).unwrap();
    std::fs::write(occupied.join("owned.txt"), "do not replace").unwrap();
    let destination_exists = fixture
        .base
        .transport
        .clone_repository(fixture.input("occupied-repository"), fixture.progress())
        .unwrap_err();
    assert!(matches!(
        destination_exists,
        AppError::Transport(failure)
            if failure.code() == "git.transport.destination-exists"
    ));
    assert_eq!(
        std::fs::read_to_string(occupied.join("owned.txt")).unwrap(),
        "do not replace"
    );

    project.exclude_patterns.clear();
    project.updated_at = Utc::now();
    projects.update(&project).unwrap();
    let result = fixture
        .base
        .transport
        .clone_repository(fixture.input("allowed-repository"), fixture.progress())
        .unwrap();
    assert_eq!(result.project.id, fixture.project_id);
    assert!(
        fixture
            .project_root
            .join("allowed-repository/.git")
            .is_dir()
    );
}

fn assert_clone_cancellation(fault: CloneFault, folder_name: &str) {
    let fixture = CloneFixture::with_fault(Some(fault));
    let input = fixture.input(folder_name);
    let operation_id = input.operation_id.clone();
    let error = fixture
        .base
        .transport
        .clone_repository(input, fixture.progress())
        .unwrap_err();
    assert!(matches!(
        error,
        AppError::Transport(failure) if failure.code() == "git.transport.cancelled"
    ));

    let operation = TransportStore::new(fixture.base.database.clone())
        .get_clone_operation(&operation_id)
        .unwrap()
        .unwrap();
    assert_eq!(operation.current_stage, CloneStage::Cancelled);
    assert!(!std::path::Path::new(&operation.staging_path).exists());
    assert!(!std::path::Path::new(&operation.owner_marker_path).exists());
    assert!(!std::path::Path::new(&operation.final_path).exists());
    let job = JobService::new(fixture.base.database.clone())
        .list()
        .unwrap()
        .into_iter()
        .find(|job| job.id == operation_id)
        .unwrap();
    assert_eq!(job.status, JobStatus::Canceled);
    assert!(
        fixture
            .base
            .transport
            .classify_clone_recovery()
            .unwrap()
            .iter()
            .all(|classification| classification.operation_id != operation_id)
    );
}

#[test]
fn clone_transfer_cancellation_cleans_only_owned_staging_and_marks_the_job_cancelled() {
    assert_clone_cancellation(CloneFault::CancelTransfer, "cancelled-transfer");
}

#[test]
fn clone_checkout_cancellation_uses_the_same_token_and_cleans_owned_staging() {
    assert_clone_cancellation(CloneFault::CancelCheckout, "cancelled-checkout");
}

#[test]
fn clone_registration_failure_is_partial_and_recovery_never_cleans_the_final_path() {
    let folder_name = "registration-failure";
    let fixture = CloneFixture::with_fault(Some(CloneFault::FailRegistration {
        folder_name: folder_name.to_owned(),
    }));
    let input = fixture.input(folder_name);
    let operation_id = input.operation_id.clone();
    let error = fixture
        .base
        .transport
        .clone_repository(input, fixture.progress())
        .unwrap_err();
    assert!(matches!(
        error,
        AppError::Transport(failure) if failure.code() == "git.transport.partial"
    ));
    let final_path = fixture.project_root.join(folder_name);
    assert!(final_path.join(".git").is_dir());

    let operation = TransportStore::new(fixture.base.database.clone())
        .get_clone_operation(&operation_id)
        .unwrap()
        .unwrap();
    assert_eq!(operation.current_stage, CloneStage::Partial);
    assert!(operation.filesystem_complete);
    assert!(std::path::Path::new(&operation.owner_marker_path).is_file());
    let recovery = fixture.base.transport.classify_clone_recovery().unwrap();
    let classification = recovery
        .iter()
        .find(|classification| classification.operation_id == operation_id)
        .unwrap();
    assert_eq!(
        classification.actions,
        vec![CloneRecoveryAction::RetryRegistration]
    );
    assert!(final_path.join(".git").is_dir());
}

#[test]
fn clone_persists_a_new_project_before_a_registration_refresh_failure() {
    let folder_name = "new-project-registration-failure";
    let fixture = CloneFixture::with_fault(Some(CloneFault::FailRegistration {
        folder_name: folder_name.to_owned(),
    }));
    let mut input = fixture.input(folder_name);
    let operation_id = input.operation_id.clone();
    input.project_target = CloneProjectTarget::New {
        name: "Recoverable New Project".to_owned(),
    };
    let error = fixture
        .base
        .transport
        .clone_repository(input, fixture.progress())
        .unwrap_err();
    assert!(matches!(
        error,
        AppError::Transport(failure) if failure.code() == "git.transport.partial"
    ));

    let operation = TransportStore::new(fixture.base.database.clone())
        .get_clone_operation(&operation_id)
        .unwrap()
        .unwrap();
    let project_id = operation
        .project_id
        .expect("new Project identity is durable before refresh");
    let project = ProjectRepository::new(fixture.base.database.clone())
        .get(&project_id)
        .unwrap();
    assert_eq!(project.scan_depth, 0);
    assert_eq!(
        dunce::canonicalize(&project.root_path).unwrap(),
        dunce::canonicalize(fixture.project_root.join(folder_name)).unwrap()
    );
    assert_eq!(
        fixture
            .base
            .transport
            .classify_clone_recovery()
            .unwrap()
            .into_iter()
            .find(|classification| classification.operation_id == operation_id)
            .unwrap()
            .actions,
        vec![CloneRecoveryAction::RetryRegistration]
    );
}

#[test]
fn clone_recovery_classifies_owned_staging_stale_markers_and_unsafe_mismatches() {
    let fixture = TransportFixture::with_https_bare_remote();
    let (staging_id, staging) = seed_clone_recovery_operation(&fixture, "staging-recovery");
    staging.write_marker().unwrap();
    std::fs::create_dir(&staging.staging).unwrap();

    let (stale_id, stale) = seed_clone_recovery_operation(&fixture, "stale-marker");
    stale.write_marker().unwrap();

    let (unsafe_id, unsafe_paths) = seed_clone_recovery_operation(&fixture, "unsafe-recovery");
    unsafe_paths.write_marker().unwrap();
    std::fs::write(&unsafe_paths.marker, "foreign owner\n").unwrap();
    std::fs::create_dir(&unsafe_paths.staging).unwrap();

    let recovery = fixture.transport.classify_clone_recovery().unwrap();
    let actions = |operation_id: &str| {
        recovery
            .iter()
            .find(|classification| classification.operation_id == operation_id)
            .unwrap()
            .actions
            .clone()
    };
    assert_eq!(
        actions(&staging_id),
        vec![
            CloneRecoveryAction::CleanupStaging,
            CloneRecoveryAction::RetryClone,
        ]
    );
    assert_eq!(actions(&stale_id), vec![CloneRecoveryAction::Interrupted]);
    assert_eq!(actions(&unsafe_id), vec![CloneRecoveryAction::UnsafePath]);
    assert!(staging.staging.is_dir());
    assert!(!stale.marker.exists());
    assert!(unsafe_paths.staging.is_dir());
    assert_eq!(
        std::fs::read_to_string(&unsafe_paths.marker).unwrap(),
        "foreign owner\n"
    );
    let stale_job = JobService::new(fixture.database.clone())
        .list()
        .unwrap()
        .into_iter()
        .find(|job| job.id == stale_id)
        .unwrap();
    assert_eq!(stale_job.status, JobStatus::Failed);
    assert_eq!(stale_job.error.unwrap().code, "git.transport.interrupted");
    assert!(
        fixture
            .transport
            .classify_clone_recovery()
            .unwrap()
            .iter()
            .all(|classification| classification.operation_id != stale_id)
    );
}
