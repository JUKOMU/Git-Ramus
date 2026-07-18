//! Service-level acceptance tests are intentionally written before the service implementation.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use git_ramus_desktop_lib::db::Database;
use git_ramus_desktop_lib::error::AppError;
use git_ramus_desktop_lib::git::engine::{GitCommand, GitOutput, GitRunner};
use git_ramus_desktop_lib::git::model::RepositoryKind;
use git_ramus_desktop_lib::git::service::{
    GitService, ProjectCreateInput, QueryContext, validate_relative_path,
};
use tempfile::tempdir;

#[test]
fn create_project_canonicalizes_root_and_applies_default_scan_depth() {
    let root = tempdir().expect("temporary root");
    let nested = root.path().join("nested");
    fs::create_dir(&nested).expect("nested directory");
    let service = GitService::new(Database::open_in_memory().expect("database"));

    let project = service
        .create_project(ProjectCreateInput {
            root_path: nested.join("..").to_string_lossy().into_owned(),
            name: "Fixture".to_owned(),
            scan_depth: None,
            exclude_patterns: vec!["vendor".to_owned()],
        })
        .expect("project creates");

    assert_eq!(project.scan_depth, 3);
    assert_eq!(
        project.root_path,
        std::fs::canonicalize(root.path())
            .unwrap()
            .to_string_lossy()
            .into_owned()
    );
    assert_eq!(project.exclude_patterns, vec!["vendor"]);
}

#[test]
fn scan_project_discovers_repositories_and_returns_partial_records() {
    let root = tempdir().expect("temporary root");
    let repo = root.path().join("repo-a");
    fs::create_dir(&repo).expect("repository directory");
    let service = GitService::new(Database::open_in_memory().expect("database"));
    let project = service
        .create_project(ProjectCreateInput {
            root_path: root.path().to_string_lossy().into_owned(),
            name: "Fixture".to_owned(),
            scan_depth: Some(3),
            exclude_patterns: Vec::new(),
        })
        .expect("project creates");

    let result = service.scan_project(&project.id).expect("scan returns");
    assert!(
        result.repositories.is_empty(),
        "ordinary directories are not repositories"
    );
    assert!(
        result.failures.is_empty(),
        "candidate failures are non-fatal"
    );
}

#[test]
fn scan_project_finds_normal_bare_worktree_skips_excluded_and_deduplicates() {
    if !git_available() {
        eprintln!("git executable unavailable; skipping service integration test");
        return;
    }
    let root = tempdir().expect("temporary root");
    let normal = root.path().join("repo-a");
    run_git(root.path(), &["init", "--quiet", normal.to_str().unwrap()]);
    run_git(&normal, &["config", "user.name", "Fixture"]);
    run_git(&normal, &["config", "user.email", "fixture@example.test"]);
    fs::write(normal.join("seed.txt"), "seed\n").unwrap();
    run_git(&normal, &["add", "--", "seed.txt"]);
    run_git(&normal, &["commit", "--quiet", "-m", "seed"]);

    let nested = root.path().join("nested");
    fs::create_dir(&nested).unwrap();
    let worktree = nested.join("worktree");
    run_git(
        &normal,
        &[
            "worktree",
            "add",
            "--detach",
            "--quiet",
            worktree.to_str().unwrap(),
        ],
    );
    let bare = root.path().join("bare.git");
    run_git(
        root.path(),
        &["init", "--bare", "--quiet", bare.to_str().unwrap()],
    );
    let excluded = root.path().join("node_modules").join("repo-c");
    run_git(
        root.path(),
        &["init", "--quiet", excluded.to_str().unwrap()],
    );
    let alias = root.path().join("alias");
    #[cfg(unix)]
    std::os::unix::fs::symlink(&normal, &alias).unwrap();
    #[cfg(windows)]
    if let Err(error) = std::os::windows::fs::symlink_dir(&normal, &alias) {
        eprintln!("symlink privilege unavailable; continuing without alias: {error}");
    }

    let service = GitService::new(Database::open_in_memory().unwrap());
    let project = service
        .create_project(ProjectCreateInput {
            root_path: root.path().to_string_lossy().into_owned(),
            name: "Fixture".to_owned(),
            scan_depth: Some(3),
            exclude_patterns: Vec::new(),
        })
        .unwrap();
    let result = service.scan_project(&project.id).unwrap();
    let kinds = result
        .repositories
        .iter()
        .map(|record| record.repository.kind.clone())
        .collect::<Vec<_>>();
    assert!(kinds.contains(&RepositoryKind::Normal));
    assert!(kinds.contains(&RepositoryKind::Worktree));
    assert!(kinds.contains(&RepositoryKind::Bare));
    assert_eq!(
        result
            .repositories
            .iter()
            .filter(|record| {
                record.repository.canonical_path
                    == std::fs::canonicalize(&normal).unwrap().to_string_lossy()
            })
            .count(),
        1
    );
    assert!(
        !result
            .repositories
            .iter()
            .any(|record| record.repository.canonical_path.contains("node_modules"))
    );
    assert!(
        !result
            .repositories
            .iter()
            .any(|record| record.repository.canonical_path == alias.to_string_lossy())
    );
}

#[test]
fn status_failure_preserves_previous_snapshot_fields_and_redacts_error() {
    if !git_available() {
        return;
    }
    let root = tempdir().unwrap();
    run_git(root.path(), &["init", "--quiet"]);
    run_git(root.path(), &["config", "user.name", "Fixture"]);
    run_git(
        root.path(),
        &["config", "user.email", "fixture@example.test"],
    );
    fs::write(root.path().join("tracked.txt"), "one\n").unwrap();
    run_git(root.path(), &["add", "--", "tracked.txt"]);
    run_git(root.path(), &["commit", "--quiet", "-m", "seed"]);
    let db = Database::open_in_memory().unwrap();
    let service = GitService::new(db.clone());
    let project = service
        .create_project(ProjectCreateInput {
            root_path: root.path().to_string_lossy().into_owned(),
            name: "Fixture".to_owned(),
            scan_depth: Some(0),
            exclude_patterns: Vec::new(),
        })
        .unwrap();
    let first = service.scan_project(&project.id).unwrap();
    let record = first.repositories.first().expect("repository discovered");
    let repository_id = record.repository.id.clone();
    let first_snapshot = record.snapshot.clone().expect("snapshot captured");
    // Point the persisted repository at a missing directory; the previous status must remain.
    db.with_connection(|connection| {
        connection.execute(
            "UPDATE repositories SET canonical_path=?1 WHERE id=?2",
            rusqlite::params![
                root.path().join("missing").to_string_lossy(),
                &repository_id
            ],
        )
    })
    .unwrap();
    let refreshed = service
        .refresh_repository(&project.id, &repository_id)
        .unwrap();
    let snapshot = refreshed.snapshot.expect("old snapshot retained");
    assert_eq!(snapshot.head_oid, first_snapshot.head_oid);
    assert!(snapshot.refresh_error_summary.is_some());
    assert!(
        !snapshot
            .refresh_error_summary
            .as_deref()
            .unwrap()
            .contains(root.path().to_string_lossy().as_ref())
    );
}

#[test]
fn trust_stage_unstage_and_commit_use_safe_paths_and_stdin_message() {
    if !git_available() {
        return;
    }
    let root = tempdir().unwrap();
    run_git(root.path(), &["init", "--quiet"]);
    run_git(root.path(), &["config", "user.name", "Fixture"]);
    run_git(
        root.path(),
        &["config", "user.email", "fixture@example.test"],
    );
    fs::write(root.path().join("tracked.txt"), "one\n").unwrap();
    run_git(root.path(), &["add", "--", "tracked.txt"]);
    run_git(root.path(), &["commit", "--quiet", "-m", "seed"]);
    fs::write(root.path().join("tracked.txt"), "two\n").unwrap();

    let service = GitService::new(Database::open_in_memory().unwrap());
    let project = service
        .create_project(ProjectCreateInput {
            root_path: root.path().to_string_lossy().into_owned(),
            name: "Fixture".to_owned(),
            scan_depth: Some(0),
            exclude_patterns: Vec::new(),
        })
        .unwrap();
    let scan = service.scan_project(&project.id).unwrap();
    let repository_id = scan.repositories[0].repository.id.clone();
    let context = QueryContext::project(&project.id);
    assert!(matches!(
        service.stage(&context, &repository_id, &["tracked.txt".to_owned()], false),
        Err(git_ramus_desktop_lib::error::AppError::TrustRequired)
    ));
    service
        .trust_repository_in_context(&context, &repository_id)
        .unwrap();
    service
        .stage(&context, &repository_id, &["tracked.txt".to_owned()], false)
        .unwrap();
    assert!(run_git_output(root.path(), &["diff", "--cached", "--quiet"]).is_err());
    service
        .unstage(&context, &repository_id, &["tracked.txt".to_owned()])
        .unwrap();
    assert!(run_git_output(root.path(), &["diff", "--cached", "--quiet"]).is_ok());
    assert!(
        service
            .stage(&context, &repository_id, &["../outside".to_owned()], false)
            .is_err()
    );
    service
        .stage(&context, &repository_id, &["tracked.txt".to_owned()], false)
        .unwrap();
    service
        .commit(&context, &repository_id, "message from stdin")
        .unwrap();
    let log = run_git_output(root.path(), &["log", "-1", "--pretty=%s"]).unwrap();
    assert_eq!(String::from_utf8_lossy(&log), "message from stdin\n");
}

#[test]
fn workspace_context_can_query_repositories_from_two_projects() {
    if !git_available() {
        return;
    }
    let root = tempdir().unwrap();
    let a = root.path().join("a");
    let b = root.path().join("b");
    run_git(root.path(), &["init", "--quiet", a.to_str().unwrap()]);
    run_git(root.path(), &["init", "--quiet", b.to_str().unwrap()]);
    let service = GitService::new(Database::open_in_memory().unwrap());
    let p1 = service
        .create_project(ProjectCreateInput {
            root_path: a.to_string_lossy().into_owned(),
            name: "A".to_owned(),
            scan_depth: Some(0),
            exclude_patterns: Vec::new(),
        })
        .unwrap();
    let p2 = service
        .create_project(ProjectCreateInput {
            root_path: b.to_string_lossy().into_owned(),
            name: "B".to_owned(),
            scan_depth: Some(0),
            exclude_patterns: Vec::new(),
        })
        .unwrap();
    service.scan_project(&p1.id).unwrap();
    service.scan_project(&p2.id).unwrap();
    let workspace = service
        .create_workspace(git_ramus_desktop_lib::git::service::WorkspaceCreateInput {
            name: "Both".to_owned(),
        })
        .unwrap();
    service
        .update_workspace_membership(
            git_ramus_desktop_lib::git::service::WorkspaceMembershipInput {
                workspace_id: workspace.id.clone(),
                project_ids: vec![p1.id.clone(), p2.id.clone()],
            },
        )
        .unwrap();
    let overview = service.get_overview_for_workspace(&workspace.id).unwrap();
    assert_eq!(overview.repository_count, 2);
}

#[test]
fn relative_path_validator_rejects_windows_prefix_and_dot_segments() {
    assert!(validate_relative_path("src\\main.rs").is_ok());
    assert!(validate_relative_path("C:\\Windows\\system.ini").is_err());
    assert!(validate_relative_path("foo/../bar").is_err());
    assert!(validate_relative_path("..\\secret").is_err());
    assert!(validate_relative_path("foo/..\\secret").is_err());
    assert!(validate_relative_path("\\\\server\\share").is_err());
}

#[test]
fn scan_refresh_workers_are_bounded_and_failed_count_is_not_double_counted() {
    if !git_available() {
        return;
    }
    let root = tempdir().unwrap();
    for index in 0..6 {
        let path = root.path().join(format!("repo-{index}"));
        run_git(root.path(), &["init", "--quiet", path.to_str().unwrap()]);
    }
    let runner = RecordingRunner::new(Duration::from_millis(25), false);
    let service = GitService::with_runner_and_concurrency(
        Database::open_in_memory().unwrap(),
        Arc::new(runner.clone()),
        2,
    );
    let project = service
        .create_project(ProjectCreateInput {
            root_path: root.path().to_string_lossy().into_owned(),
            name: "many".to_owned(),
            scan_depth: Some(1),
            exclude_patterns: Vec::new(),
        })
        .unwrap();
    let progress = Arc::new(Mutex::new(Vec::new()));
    let progress_sink = Arc::clone(&progress);
    let result = service
        .scan_project_with_progress(&project.id, move |record| {
            progress_sink
                .lock()
                .unwrap()
                .push(record.repository.id.clone());
        })
        .unwrap();
    assert_eq!(result.repositories.len(), 6);
    assert_eq!(progress.lock().unwrap().len(), 6);
    assert_eq!(result.progress.len(), 6);
    assert!(runner.max_active() > 1, "refreshes should overlap");
    assert!(
        runner.max_active() <= 2,
        "refreshes exceed configured bound"
    );
    assert_eq!(result.failed, result.failures.len());
}

#[test]
fn first_refresh_failure_is_returned_as_an_error_snapshot() {
    if !git_available() {
        return;
    }
    let root = tempdir().unwrap();
    let repo = root.path().join("repo");
    run_git(root.path(), &["init", "--quiet", repo.to_str().unwrap()]);
    let runner = RecordingRunner::new(Duration::ZERO, true);
    let service = GitService::with_runner(Database::open_in_memory().unwrap(), Arc::new(runner));
    let project = service
        .create_project(ProjectCreateInput {
            root_path: root.path().to_string_lossy().into_owned(),
            name: "failure".to_owned(),
            scan_depth: Some(1),
            exclude_patterns: Vec::new(),
        })
        .unwrap();
    let result = service.scan_project(&project.id).unwrap();
    assert_eq!(result.repositories.len(), 1);
    let record = &result.repositories[0];
    let snapshot = record
        .snapshot
        .as_ref()
        .expect("failure has an explicit snapshot");
    assert!(snapshot.refresh_error_summary.is_some());
    assert_eq!(result.failed, result.failures.len());
}

#[test]
fn read_commands_include_lock_and_textconv_safety_flags() {
    if !git_available() {
        return;
    }
    let root = tempdir().unwrap();
    let repo = root.path().join("repo");
    run_git(root.path(), &["init", "--quiet", repo.to_str().unwrap()]);
    let runner = RecordingRunner::new(Duration::ZERO, false);
    let service = GitService::with_runner(
        Database::open_in_memory().unwrap(),
        Arc::new(runner.clone()),
    );
    let project = service
        .create_project(ProjectCreateInput {
            root_path: root.path().to_string_lossy().into_owned(),
            name: "flags".to_owned(),
            scan_depth: Some(1),
            exclude_patterns: Vec::new(),
        })
        .unwrap();
    let result = service.scan_project(&project.id).unwrap();
    let repository_id = result.repositories[0].repository.id.clone();
    let context = QueryContext::project(&project.id);
    service
        .get_diff(&context, &repository_id, &[], false)
        .unwrap();
    let calls = runner.calls();
    let status = calls
        .iter()
        .find(|args| args.iter().any(|arg| arg == "status"))
        .expect("status call");
    assert!(status.iter().any(|arg| arg == "--no-optional-locks"));
    let diff = calls
        .iter()
        .find(|args| args.iter().any(|arg| arg == "diff"))
        .expect("diff call");
    assert!(diff.iter().any(|arg| arg == "--no-optional-locks"));
    assert!(diff.iter().any(|arg| arg == "--no-ext-diff"));
    assert!(diff.iter().any(|arg| arg == "--no-textconv"));
}

#[derive(Clone)]
struct RecordingRunner {
    calls: Arc<Mutex<Vec<Vec<String>>>>,
    active: Arc<Mutex<usize>>,
    max_active: Arc<Mutex<usize>>,
    delay: Duration,
    fail: bool,
}

impl RecordingRunner {
    fn new(delay: Duration, fail: bool) -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            active: Arc::new(Mutex::new(0)),
            max_active: Arc::new(Mutex::new(0)),
            delay,
            fail,
        }
    }

    fn calls(&self) -> Vec<Vec<String>> {
        self.calls.lock().unwrap().clone()
    }

    fn max_active(&self) -> usize {
        *self.max_active.lock().unwrap()
    }
}

impl GitRunner for RecordingRunner {
    fn run(&self, command: GitCommand) -> Result<GitOutput, AppError> {
        self.calls.lock().unwrap().push(
            command
                .args
                .iter()
                .map(|arg| arg.to_string_lossy().into_owned())
                .collect(),
        );
        {
            let mut active = self.active.lock().unwrap();
            *active += 1;
            let mut max_active = self.max_active.lock().unwrap();
            *max_active = (*max_active).max(*active);
        }
        if !self.delay.is_zero() {
            thread::sleep(self.delay);
        }
        {
            let mut active = self.active.lock().unwrap();
            *active -= 1;
        }
        let status = if self.fail {
            #[cfg(windows)]
            {
                Command::new("cmd")
                    .args(["/C", "exit", "1"])
                    .status()
                    .unwrap()
            }
            #[cfg(not(windows))]
            {
                Command::new("false").status().unwrap()
            }
        } else {
            #[cfg(windows)]
            {
                Command::new("cmd")
                    .args(["/C", "exit", "0"])
                    .status()
                    .unwrap()
            }
            #[cfg(not(windows))]
            {
                Command::new("true").status().unwrap()
            }
        };
        let is_diff = command.args.iter().any(|arg| arg.as_os_str() == "diff");
        let stdout = if self.fail || is_diff {
            Vec::new()
        } else {
            b"# branch.oid (initial)\0# branch.head (unborn)\0".to_vec()
        };
        Ok(GitOutput {
            status,
            stdout,
            stderr: if self.fail {
                b"fatal: https://user:secret@example.test failed\n".to_vec()
            } else {
                Vec::new()
            },
        })
    }
}

fn git_available() -> bool {
    Command::new("git").arg("--version").output().is_ok()
}

fn run_git(repo: &Path, args: &[&str]) {
    let status = Command::new("git")
        .current_dir(repo)
        .args(args)
        .status()
        .expect("git executable");
    assert!(status.success(), "git command failed: {args:?}");
}

fn run_git_output(repo: &Path, args: &[&str]) -> Result<Vec<u8>, ()> {
    let output = Command::new("git")
        .current_dir(repo)
        .args(args)
        .output()
        .map_err(|_| ())?;
    if output.status.success() {
        Ok(output.stdout)
    } else {
        Err(())
    }
}
