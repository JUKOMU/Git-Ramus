use std::path::Path;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use chrono::Utc;
use git_ramus_desktop_lib::db::Database;
use git_ramus_desktop_lib::error::AppError;
use git_ramus_desktop_lib::git::engine::{GitCommand, GitOutput, GitRunner, SystemGitRunner};
use git_ramus_desktop_lib::git::model::{Remote, Repository, RepositoryKind, Trust};
use git_ramus_desktop_lib::git::repository::{
    RepositoryRepository, RepositoryWriteLocks, TrustRepository,
};
use git_ramus_desktop_lib::git::transport::model::EffectiveTransportSource;
use git_ramus_desktop_lib::git::transport::profile_service::{
    DriftResolution, ProfileDeletionResolution, TransportProfileService,
};
use git_ramus_desktop_lib::git::transport::store::TransportStore;
use tempfile::TempDir;

struct FailingConfigWriteRunner {
    inner: SystemGitRunner,
    fail_after: usize,
    writes: AtomicUsize,
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
    repository_id: String,
    database: Database,
    service: TransportProfileService,
}

impl TransportFixture {
    fn new() -> Self {
        Self::with_optional_local_config(None)
    }

    fn with_local_config(key: &str, value: &str) -> Self {
        Self::with_optional_local_config(Some((key, value)))
    }

    fn with_optional_local_config(local_config: Option<(&str, &str)>) -> Self {
        Self::with_options(local_config, None)
    }

    fn with_failing_config_writes(fail_after: usize) -> Self {
        Self::with_options(None, Some(fail_after))
    }

    fn with_options(
        local_config: Option<(&str, &str)>,
        fail_config_writes_after: Option<usize>,
    ) -> Self {
        let directory = tempfile::tempdir().expect("temporary transport fixture");
        let repository_path = directory.path().join("repository");
        std::fs::create_dir(&repository_path).expect("repository directory");
        run_git(&repository_path, &["init", "--quiet"]);
        run_git(
            &repository_path,
            &[
                "remote",
                "add",
                "origin",
                "https://gitlab.example/group/repository.git",
            ],
        );
        if let Some((key, value)) = local_config {
            run_git(&repository_path, &["config", "--local", key, value]);
        }

        let database = Database::open_in_memory().expect("database opens");
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
            .add_remote(&Remote {
                repository_id: repository.id.clone(),
                name: "origin".to_owned(),
                fetch_url: Some("https://gitlab.example/group/repository.git".to_owned()),
                push_url: None,
            })
            .expect("remote persists");

        let home = directory.path().join("home");
        let xdg = directory.path().join("xdg");
        std::fs::create_dir_all(&home).expect("sealed home");
        std::fs::create_dir_all(&xdg).expect("sealed XDG home");
        let global_config = directory.path().join("global.gitconfig");
        std::fs::write(&global_config, "").expect("sealed global config");
        let runner = SystemGitRunner::new().with_sealed_config(home, xdg, global_config);
        let runner: Arc<dyn GitRunner> = match fail_config_writes_after {
            Some(fail_after) => Arc::new(FailingConfigWriteRunner {
                inner: runner,
                fail_after,
                writes: AtomicUsize::new(0),
            }),
            None => Arc::new(runner),
        };
        let service =
            TransportProfileService::new(database.clone(), RepositoryWriteLocks::default(), runner);

        Self {
            _directory: directory,
            repository_path,
            repository_id: repository.id,
            database,
            service,
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
