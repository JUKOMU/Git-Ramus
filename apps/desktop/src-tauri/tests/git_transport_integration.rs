use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use chrono::Utc;
use git_ramus_desktop_lib::db::Database;
use git_ramus_desktop_lib::error::AppError;
use git_ramus_desktop_lib::git::engine::{GitCommand, GitOutput, GitRunner, SystemGitRunner};
use git_ramus_desktop_lib::git::model::{Project, Remote, Repository, RepositoryKind, Trust};
use git_ramus_desktop_lib::git::repository::{
    ProjectRepository, RepositoryRepository, RepositoryWriteLocks, TrustRepository,
};
use git_ramus_desktop_lib::git::service::{GitService, QueryContext};
use git_ramus_desktop_lib::git::transport::model::{
    EffectiveTransportSource, FetchInput, NetworkProgress, NetworkStage,
};
use git_ramus_desktop_lib::git::transport::operation::TransportOperationRegistry;
use git_ramus_desktop_lib::git::transport::profile_service::{
    DriftResolution, ProfileDeletionResolution, TransportProfileService,
};
use git_ramus_desktop_lib::git::transport::service::{
    GitTransportService, NetworkProgressReporter, NoopNetworkProgressReporter,
};
use git_ramus_desktop_lib::git::transport::store::TransportStore;
use git_ramus_desktop_lib::jobs::JobService;
use git_ramus_desktop_lib::jobs::model::JobStatus;
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
}

impl TransportFixture {
    fn new() -> Self {
        Self::with_optional_local_config(None)
    }

    fn with_local_config(key: &str, value: &str) -> Self {
        Self::with_optional_local_config(Some((key, value)))
    }

    fn with_optional_local_config(local_config: Option<(&str, &str)>) -> Self {
        Self::with_options(local_config, None, false)
    }

    fn with_failing_config_writes(fail_after: usize) -> Self {
        Self::with_options(None, Some(fail_after), false)
    }

    fn with_https_bare_remote() -> Self {
        let fixture = Self::with_options(None, None, true);
        fixture.trust();
        fixture
    }

    fn with_options(
        local_config: Option<(&str, &str)>,
        fail_config_writes_after: Option<usize>,
        with_bare_remote: bool,
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
        let runner = SystemGitRunner::new().with_sealed_config(home, xdg, global_config);
        let runner: Arc<dyn GitRunner> = match fail_config_writes_after {
            Some(fail_after) => Arc::new(FailingConfigWriteRunner {
                inner: runner,
                fail_after,
                writes: AtomicUsize::new(0),
            }),
            None => Arc::new(runner),
        };
        let write_locks = RepositoryWriteLocks::default();
        let git = GitService::with_runner_concurrency_and_write_locks(
            database.clone(),
            runner.clone(),
            4,
            write_locks.clone(),
        );
        let service =
            TransportProfileService::new(database.clone(), write_locks.clone(), runner.clone());
        let transport = GitTransportService::new(
            database.clone(),
            git.clone(),
            service.clone(),
            JobService::new(database.clone()),
            TransportOperationRegistry::default(),
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
    assert_eq!(events.first().unwrap().stage, NetworkStage::Transferring);
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
