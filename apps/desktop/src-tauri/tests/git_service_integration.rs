//! Service-level acceptance tests are intentionally written before the service implementation.

use std::fs;
use std::path::Path;
use std::process::Command;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;
use std::time::Duration;

use git_ramus_desktop_lib::db::Database;
use git_ramus_desktop_lib::error::AppError;
use git_ramus_desktop_lib::git::engine::{GitCommand, GitOutput, GitRunner};
use git_ramus_desktop_lib::git::model::RepositoryKind;
use git_ramus_desktop_lib::git::service::{
    GitService, ProjectCreateInput, ProjectUpdateInput, QueryContext, validate_relative_path,
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
fn changes_and_diff_refresh_failures_persist_error_snapshots() {
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
    fs::write(root.path().join("tracked.txt"), "seed\n").unwrap();
    run_git(root.path(), &["add", "--", "tracked.txt"]);
    run_git(root.path(), &["commit", "--quiet", "-m", "seed"]);
    let db = Database::open_in_memory().unwrap();
    let service = GitService::new(db.clone());
    let project = service
        .create_project(ProjectCreateInput {
            root_path: root.path().to_string_lossy().into_owned(),
            name: "query failure".to_owned(),
            scan_depth: Some(0),
            exclude_patterns: Vec::new(),
        })
        .unwrap();
    let scan = service.scan_project(&project.id).unwrap();
    let repository_id = scan.repositories[0].repository.id.clone();
    let successful = scan.repositories[0].snapshot.clone().unwrap();
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
    let context = QueryContext::project(&project.id);

    assert!(matches!(
        service.get_changes(&context, &repository_id),
        Err(AppError::Git(_))
    ));
    assert_eq!(snapshot_count(&db, &repository_id), 2);
    let overview = service.get_overview(&context).unwrap();
    let failed_snapshot = overview.repositories[0].snapshot.as_ref().unwrap();
    assert_eq!(failed_snapshot.head_oid, successful.head_oid);
    assert!(failed_snapshot.refresh_error_summary.is_some());

    assert!(matches!(
        service.get_diff(&context, &repository_id, &[], false),
        Err(AppError::Git(_))
    ));
    assert_eq!(snapshot_count(&db, &repository_id), 3);
    assert!(
        service.get_overview(&context).unwrap().repositories[0]
            .snapshot
            .as_ref()
            .unwrap()
            .refresh_error_summary
            .is_some()
    );
}

#[test]
fn scan_surfaces_refresh_snapshot_persistence_failure() {
    if !git_available() {
        return;
    }
    let root = tempdir().unwrap();
    let repo = root.path().join("repo");
    run_git(root.path(), &["init", "--quiet", repo.to_str().unwrap()]);
    let db = Database::open_in_memory().unwrap();
    db.with_connection(|connection| {
        connection.execute_batch(
            "CREATE TRIGGER reject_snapshot_insert BEFORE INSERT ON repository_snapshots
             BEGIN SELECT RAISE(ABORT, 'snapshot insert rejected'); END;",
        )
    })
    .unwrap();
    let runner = RecordingRunner::new(Duration::ZERO, true);
    let service = GitService::with_runner(db, Arc::new(runner));
    let project = service
        .create_project(ProjectCreateInput {
            root_path: root.path().to_string_lossy().into_owned(),
            name: "persistence failure".to_owned(),
            scan_depth: Some(1),
            exclude_patterns: Vec::new(),
        })
        .unwrap();

    let scan = service.scan_project(&project.id).unwrap();
    assert_eq!(scan.repositories.len(), 1);
    let error = scan.repositories[0].error.as_deref().unwrap();
    assert!(
        error.contains("database operation failed"),
        "snapshot persistence failure was hidden: {error}"
    );
    assert!(scan.failures[0].error.contains("database operation failed"));
    assert_eq!((scan.total, scan.completed, scan.failed), (1, 0, 1));
    assert_eq!(scan.discovery_failed, 0);
    assert_eq!(scan.total, scan.completed + scan.failed);
    assert!(scan.progress.iter().all(|entry| entry.total == scan.total));
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
fn changing_project_root_removes_stale_repository_context_only_for_root_changes() {
    if !git_available() {
        return;
    }
    let root = tempdir().unwrap();
    let root_a = root.path().join("root-a");
    let root_b = root.path().join("root-b");
    run_git(root.path(), &["init", "--quiet", root_a.to_str().unwrap()]);
    run_git(root.path(), &["init", "--quiet", root_b.to_str().unwrap()]);
    let service = GitService::new(Database::open_in_memory().unwrap());
    let project = service
        .create_project(ProjectCreateInput {
            root_path: root_a.to_string_lossy().into_owned(),
            name: "movable".to_owned(),
            scan_depth: Some(0),
            exclude_patterns: Vec::new(),
        })
        .unwrap();
    let scan_a = service.scan_project(&project.id).unwrap();
    let repository_a = scan_a.repositories[0].repository.id.clone();

    service
        .update_project(ProjectUpdateInput {
            project_id: project.id.clone(),
            root_path: Some(root_b.to_string_lossy().into_owned()),
            name: None,
            scan_depth: None,
            exclude_patterns: None,
        })
        .unwrap();
    assert!(
        service
            .get_overview_for_project(&project.id)
            .unwrap()
            .repositories
            .is_empty()
    );
    let context = QueryContext::project(&project.id);
    assert!(
        service
            .trust_repository_in_context(&context, &repository_a)
            .is_err()
    );
    assert!(service.stage(&context, &repository_a, &[], true).is_err());
    assert!(
        service
            .commit(&context, &repository_a, "must not commit stale repository")
            .is_err()
    );

    let scan_b = service.scan_project(&project.id).unwrap();
    assert_eq!(scan_b.repositories.len(), 1);
    service
        .update_project(ProjectUpdateInput {
            project_id: project.id.clone(),
            root_path: None,
            name: Some("renamed".to_owned()),
            scan_depth: Some(2),
            exclude_patterns: Some(vec!["vendor".to_owned()]),
        })
        .unwrap();
    assert_eq!(
        service
            .get_overview_for_project(&project.id)
            .unwrap()
            .repository_count,
        1,
        "name and scan-rule updates must preserve repository relationships"
    );
}

#[test]
fn project_root_update_waits_for_scan_and_clears_relationships() {
    if !git_available() {
        return;
    }
    let root = tempdir().unwrap();
    let root_a = root.path().join("root-a");
    let root_b = root.path().join("root-b");
    run_git(root.path(), &["init", "--quiet", root_a.to_str().unwrap()]);
    fs::create_dir(&root_b).unwrap();
    let (runner, config_entered, release_config) = BlockingConfigRunner::new();
    let service = GitService::with_runner(Database::open_in_memory().unwrap(), Arc::new(runner));
    let project = service
        .create_project(ProjectCreateInput {
            root_path: root_a.to_string_lossy().into_owned(),
            name: "scan race".to_owned(),
            scan_depth: Some(0),
            exclude_patterns: Vec::new(),
        })
        .unwrap();

    let scan_service = service.clone();
    let scan_project_id = project.id.clone();
    let scan = thread::spawn(move || scan_service.scan_project(&scan_project_id));
    config_entered
        .recv_timeout(Duration::from_secs(2))
        .expect("scan reaches the blocked config query");

    let update_service = service.clone();
    let update_project_id = project.id.clone();
    let update_root = root_b.to_string_lossy().into_owned();
    let (updated, update_observed) = mpsc::channel();
    let update = thread::spawn(move || {
        let result = update_service.update_project(ProjectUpdateInput {
            project_id: update_project_id,
            root_path: Some(update_root),
            name: None,
            scan_depth: None,
            exclude_patterns: None,
        });
        updated.send(()).unwrap();
        result
    });

    let observed_while_scan_blocked = update_observed.recv_timeout(Duration::from_millis(100));
    release_config.send(()).unwrap();
    let scan_result = scan.join().unwrap().unwrap();
    update.join().unwrap().unwrap();

    assert!(
        matches!(
            observed_while_scan_blocked,
            Err(mpsc::RecvTimeoutError::Timeout)
        ),
        "same-project root update completed while scan still held its project guard"
    );
    assert_eq!(scan_result.repositories.len(), 1);
    assert_eq!(
        service
            .get_overview_for_project(&project.id)
            .unwrap()
            .repository_count,
        0,
        "root update must clear the relationship created by the completed old-root scan"
    );
}

#[test]
fn duplicate_project_root_update_rolls_back_project_and_relationships() {
    if !git_available() {
        return;
    }
    let root = tempdir().unwrap();
    let root_a = root.path().join("root-a");
    let root_b = root.path().join("root-b");
    run_git(root.path(), &["init", "--quiet", root_a.to_str().unwrap()]);
    run_git(root.path(), &["init", "--quiet", root_b.to_str().unwrap()]);
    let service = GitService::new(Database::open_in_memory().unwrap());
    let project_a = service
        .create_project(ProjectCreateInput {
            root_path: root_a.to_string_lossy().into_owned(),
            name: "A".to_owned(),
            scan_depth: Some(0),
            exclude_patterns: Vec::new(),
        })
        .unwrap();
    let project_b = service
        .create_project(ProjectCreateInput {
            root_path: root_b.to_string_lossy().into_owned(),
            name: "B".to_owned(),
            scan_depth: Some(0),
            exclude_patterns: Vec::new(),
        })
        .unwrap();
    service.scan_project(&project_a.id).unwrap();

    assert!(
        service
            .update_project(ProjectUpdateInput {
                project_id: project_a.id.clone(),
                root_path: Some(root_b.to_string_lossy().into_owned()),
                name: Some("should roll back".to_owned()),
                scan_depth: None,
                exclude_patterns: None,
            })
            .is_err()
    );
    let loaded = service.get_project(&project_a.id).unwrap();
    assert_eq!(
        loaded.root_path,
        fs::canonicalize(&root_a).unwrap().to_string_lossy()
    );
    assert_eq!(loaded.name, "A");
    assert_eq!(
        service
            .get_overview_for_project(&project_a.id)
            .unwrap()
            .repository_count,
        1
    );
    assert_eq!(service.get_project(&project_b.id).unwrap().name, "B");
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
    assert_eq!((result.total, result.completed, result.failed), (6, 6, 0));
    assert_eq!(result.discovery_failed, 0);
    assert!(result.failures.is_empty());
    assert_eq!(result.total, result.completed + result.failed);
    assert!(
        result
            .progress
            .iter()
            .all(|entry| entry.total == result.total)
    );
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
    let service = GitService::with_runner(
        Database::open_in_memory().unwrap(),
        Arc::new(runner.clone()),
    );
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
    assert_eq!((result.total, result.completed, result.failed), (1, 0, 1));
    assert_eq!(result.discovery_failed, 0);
    assert_eq!(result.failures.len(), 1);
    assert_eq!(result.total, result.completed + result.failed);
    assert!(
        result
            .progress
            .iter()
            .all(|entry| entry.total == result.total)
    );
    let calls = runner.calls();
    assert!(
        calls
            .iter()
            .any(|args| args.iter().any(|arg| arg == "config"))
    );
    assert!(
        !calls
            .iter()
            .any(|args| args.iter().any(|arg| arg == "status")),
        "status must not start after the filter-key query fails"
    );
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
    let untrusted_calls = runner.calls();
    let config = untrusted_calls
        .iter()
        .find(|args| args.iter().any(|arg| arg == "config"))
        .expect("filter-key config call");
    assert!(config.iter().any(|arg| arg == "--no-pager"));
    assert!(config.iter().any(|arg| arg == "--name-only"));
    let status = untrusted_calls
        .iter()
        .find(|args| args.iter().any(|arg| arg == "status"))
        .expect("status call");
    assert!(status.iter().any(|arg| arg == "--no-optional-locks"));
    assert!(has_disabled_fsmonitor_config(status));
    assert!(status.iter().any(|arg| arg == "--ignore-submodules=all"));
    assert!(
        !untrusted_calls
            .iter()
            .any(|args| args.iter().any(|arg| arg == "diff")),
        "untrusted get_diff must not start git diff"
    );

    service
        .trust_repository_in_context(&context, &repository_id)
        .unwrap();
    runner.clear_calls();
    service
        .get_diff(&context, &repository_id, &[], false)
        .unwrap();
    let trusted_calls = runner.calls();
    let diff = trusted_calls
        .iter()
        .find(|args| args.iter().any(|arg| arg == "diff"))
        .expect("trusted diff call");
    assert!(diff.iter().any(|arg| arg == "--no-optional-locks"));
    assert!(has_disabled_fsmonitor_config(diff));
    assert!(diff.iter().any(|arg| arg == "--no-ext-diff"));
    assert!(diff.iter().any(|arg| arg == "--no-textconv"));
}

#[test]
fn untrusted_status_disables_every_discovered_filter_driver() {
    if !git_available() {
        return;
    }
    let root = tempdir().unwrap();
    let repo = root.path().join("repo");
    run_git(root.path(), &["init", "--quiet", repo.to_str().unwrap()]);
    let runner = RecordingRunner::new(Duration::ZERO, false).with_config_output(
        b"filter.evil.clean\0filter.evil.process\0filter.other.smudge\0filter.other.required\0",
    );
    let service = GitService::with_runner(
        Database::open_in_memory().unwrap(),
        Arc::new(runner.clone()),
    );
    let project = service
        .create_project(ProjectCreateInput {
            root_path: root.path().to_string_lossy().into_owned(),
            name: "filter overrides".to_owned(),
            scan_depth: Some(1),
            exclude_patterns: Vec::new(),
        })
        .unwrap();
    let scan = service.scan_project(&project.id).unwrap();
    let calls = runner.calls();
    let status = calls
        .iter()
        .find(|args| args.iter().any(|arg| arg == "status"))
        .expect("status call");
    let status_index = status.iter().position(|arg| arg == "status").unwrap();
    for driver in ["evil", "other"] {
        for field in ["clean=", "smudge=", "process="] {
            let value = format!("filter.{driver}.{field}");
            let index = status.iter().position(|arg| arg == &value).unwrap();
            assert!(index < status_index, "filter override must precede status");
            assert_eq!(status[index - 1], "-c");
        }
        let value = format!("filter.{driver}.required=false");
        let index = status.iter().position(|arg| arg == &value).unwrap();
        assert!(
            index < status_index,
            "required override must precede status"
        );
        assert_eq!(status[index - 1], "-c");
    }

    let repository_id = scan.repositories[0].repository.id.clone();
    service
        .trust_repository_in_context(&QueryContext::project(&project.id), &repository_id)
        .unwrap();
    runner.clear_calls();
    service
        .get_changes(&QueryContext::project(&project.id), &repository_id)
        .unwrap();
    let trusted_calls = runner.calls();
    assert!(
        !trusted_calls
            .iter()
            .any(|args| args.iter().any(|arg| arg == "config")),
        "trusted status should retain repository filter semantics"
    );
    let trusted_status = trusted_calls
        .iter()
        .find(|args| args.iter().any(|arg| arg == "status"))
        .unwrap();
    assert!(
        !trusted_status
            .iter()
            .any(|arg| arg == "--ignore-submodules=all")
    );
    assert!(
        !trusted_status
            .iter()
            .any(|arg| arg.starts_with("filter.evil."))
    );
}

#[test]
fn untrusted_status_does_not_execute_repository_fsmonitor_hook() {
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
    fs::write(root.path().join("tracked.txt"), "seed\n").unwrap();
    run_git(root.path(), &["add", "--", "tracked.txt"]);
    run_git(root.path(), &["commit", "--quiet", "-m", "seed"]);

    let hook = root.path().join(".git").join("fsmonitor-test");
    fs::write(
        &hook,
        "#!/bin/sh\nprintf invoked > fsmonitor-marker\nprintf 'test-token\\n'\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(&hook).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&hook, permissions).unwrap();
    }
    run_git(
        root.path(),
        &["config", "core.fsmonitor", ".git/fsmonitor-test"],
    );
    let marker = root.path().join("fsmonitor-marker");
    run_git(root.path(), &["status", "--porcelain=v2"]);
    assert!(marker.exists(), "fsmonitor fixture did not execute");
    fs::remove_file(&marker).unwrap();

    let service = GitService::new(Database::open_in_memory().unwrap());
    let project = service
        .create_project(ProjectCreateInput {
            root_path: root.path().to_string_lossy().into_owned(),
            name: "untrusted fsmonitor".to_owned(),
            scan_depth: Some(0),
            exclude_patterns: Vec::new(),
        })
        .unwrap();
    service.scan_project(&project.id).unwrap();
    assert!(
        !marker.exists(),
        "untrusted status executed repository core.fsmonitor"
    );
}

#[test]
fn untrusted_unstaged_diff_does_not_execute_clean_filter() {
    if !git_available() {
        return;
    }
    let root = filtered_repository("clean");
    let marker = root.path().join("clean-filter-marker");
    let service = GitService::new(Database::open_in_memory().unwrap());
    let project = service
        .create_project(ProjectCreateInput {
            root_path: root.path().to_string_lossy().into_owned(),
            name: "untrusted clean filter".to_owned(),
            scan_depth: Some(0),
            exclude_patterns: Vec::new(),
        })
        .unwrap();
    let scan = service.scan_project(&project.id).unwrap();
    assert!(
        !marker.exists(),
        "status unexpectedly executed clean filter"
    );
    let repository_id = scan.repositories[0].repository.id.clone();

    let diff = service
        .get_diff(
            &QueryContext::project(&project.id),
            &repository_id,
            &["tracked.txt".to_owned()],
            false,
        )
        .expect("untrusted clean-filter diff returns a safe summary");
    assert_eq!(diff.summary.files.len(), 1);
    assert_eq!(diff.summary.files[0].path, "tracked.txt");
    assert!(
        !marker.exists(),
        "untrusted unstaged diff executed repository clean filter"
    );
}

#[test]
fn untrusted_unstaged_diff_does_not_execute_process_filter() {
    if !git_available() {
        return;
    }
    let root = filtered_repository("process");
    let marker = root.path().join("process-filter-marker");
    let service = GitService::new(Database::open_in_memory().unwrap());
    let project = service
        .create_project(ProjectCreateInput {
            root_path: root.path().to_string_lossy().into_owned(),
            name: "untrusted process filter".to_owned(),
            scan_depth: Some(0),
            exclude_patterns: Vec::new(),
        })
        .unwrap();
    let scan = service.scan_project(&project.id).unwrap();
    assert!(
        !marker.exists(),
        "status unexpectedly executed process filter"
    );
    let repository_id = scan.repositories[0].repository.id.clone();

    let diff = service
        .get_diff(
            &QueryContext::project(&project.id),
            &repository_id,
            &["tracked.txt".to_owned()],
            false,
        )
        .expect("untrusted process-filter diff returns a safe summary");
    assert_eq!(diff.summary.files.len(), 1);
    assert_eq!(diff.summary.files[0].path, "tracked.txt");
    assert!(
        !marker.exists(),
        "untrusted unstaged diff executed repository process filter"
    );
}

#[test]
fn untrusted_staged_diff_does_not_execute_clean_filter() {
    if !git_available() {
        return;
    }
    let root = filtered_repository("clean");
    let marker = root.path().join("clean-filter-marker");
    run_git(root.path(), &["add", "--", "tracked.txt"]);
    assert!(
        marker.exists(),
        "clean filter fixture did not execute during add"
    );
    fs::remove_file(&marker).unwrap();
    let service = GitService::new(Database::open_in_memory().unwrap());
    let project = service
        .create_project(ProjectCreateInput {
            root_path: root.path().to_string_lossy().into_owned(),
            name: "untrusted staged filter".to_owned(),
            scan_depth: Some(0),
            exclude_patterns: Vec::new(),
        })
        .unwrap();
    let scan = service.scan_project(&project.id).unwrap();
    assert!(
        !marker.exists(),
        "status unexpectedly executed clean filter"
    );
    let repository_id = scan.repositories[0].repository.id.clone();

    let diff = service
        .get_diff(
            &QueryContext::project(&project.id),
            &repository_id,
            &["tracked.txt".to_owned()],
            true,
        )
        .unwrap();
    assert_eq!(diff.summary.files.len(), 1);
    assert_eq!(diff.summary.files[0].path, "tracked.txt");
    assert!(
        !marker.exists(),
        "untrusted staged diff executed repository clean filter"
    );
}

#[test]
fn concurrent_diff_processes_share_the_global_read_limit() {
    if !git_available() {
        return;
    }
    let root = tempdir().unwrap();
    let repo = root.path().join("repo");
    run_git(root.path(), &["init", "--quiet", repo.to_str().unwrap()]);
    let runner = RecordingRunner::new(Duration::from_millis(30), false);
    let service = GitService::with_runner_and_concurrency(
        Database::open_in_memory().unwrap(),
        Arc::new(runner.clone()),
        2,
    );
    let project = service
        .create_project(ProjectCreateInput {
            root_path: root.path().to_string_lossy().into_owned(),
            name: "diff concurrency".to_owned(),
            scan_depth: Some(1),
            exclude_patterns: Vec::new(),
        })
        .unwrap();
    let scan = service.scan_project(&project.id).unwrap();
    let repository_id = scan.repositories[0].repository.id.clone();
    service
        .trust_repository_in_context(&QueryContext::project(&project.id), &repository_id)
        .unwrap();
    runner.reset_activity();

    let mut workers = Vec::new();
    for _ in 0..6 {
        let service = service.clone();
        let repository_id = repository_id.clone();
        let context = QueryContext::project(&project.id);
        workers.push(thread::spawn(move || {
            service
                .get_diff(&context, &repository_id, &[], false)
                .unwrap();
        }));
    }
    for worker in workers {
        worker.join().unwrap();
    }
    assert!(runner.max_active() > 1, "read processes should overlap");
    assert!(
        runner.max_active() <= 2,
        "diff process escaped the configured read limit"
    );
}

#[test]
fn project_inside_an_outer_repository_returns_a_partial_scan() {
    if !git_available() {
        return;
    }
    let outer = tempdir().unwrap();
    run_git(outer.path(), &["init", "--quiet"]);
    let project_root = outer.path().join("child-project");
    fs::create_dir(&project_root).unwrap();
    let nested_repository = project_root.join("nested-repository");
    run_git(
        &project_root,
        &["init", "--quiet", nested_repository.to_str().unwrap()],
    );
    let service = GitService::new(Database::open_in_memory().unwrap());
    let project = service
        .create_project(ProjectCreateInput {
            root_path: project_root.to_string_lossy().into_owned(),
            name: "nested root".to_owned(),
            scan_depth: Some(1),
            exclude_patterns: Vec::new(),
        })
        .unwrap();

    let scan = service
        .scan_project(&project.id)
        .expect("out-of-scope detected root is a partial result");
    assert_eq!(scan.repositories.len(), 1);
    assert_eq!(
        scan.repositories[0].repository.canonical_path,
        fs::canonicalize(nested_repository)
            .unwrap()
            .to_string_lossy()
    );
    assert_eq!(scan.failures.len(), 1);
    assert_eq!((scan.total, scan.completed, scan.failed), (1, 1, 0));
    assert_eq!(scan.discovery_failed, 1);
    assert_eq!(scan.total, scan.completed + scan.failed);
    assert!(scan.progress.iter().all(|entry| entry.total == scan.total));
    assert!(scan.failures[0].error.contains("escapes project root"));
}

#[test]
fn repository_kind_serializes_to_contract_values() {
    assert_eq!(
        serde_json::to_string(&RepositoryKind::Normal).unwrap(),
        "\"normal\""
    );
    assert_eq!(
        serde_json::to_string(&RepositoryKind::Bare).unwrap(),
        "\"bare\""
    );
    assert_eq!(
        serde_json::to_string(&RepositoryKind::Worktree).unwrap(),
        "\"worktree\""
    );
}

#[derive(Clone)]
struct RecordingRunner {
    calls: Arc<Mutex<Vec<Vec<String>>>>,
    active: Arc<Mutex<usize>>,
    max_active: Arc<Mutex<usize>>,
    delay: Duration,
    fail: bool,
    config_output: Arc<Vec<u8>>,
}

#[derive(Clone)]
struct BlockingConfigRunner {
    inner: RecordingRunner,
    config_entered: Arc<Mutex<Option<mpsc::Sender<()>>>>,
    release_config: Arc<Mutex<mpsc::Receiver<()>>>,
}

impl BlockingConfigRunner {
    fn new() -> (Self, mpsc::Receiver<()>, mpsc::Sender<()>) {
        let (config_entered, entered) = mpsc::channel();
        let (release, release_config) = mpsc::channel();
        (
            Self {
                inner: RecordingRunner::new(Duration::ZERO, false),
                config_entered: Arc::new(Mutex::new(Some(config_entered))),
                release_config: Arc::new(Mutex::new(release_config)),
            },
            entered,
            release,
        )
    }
}

impl GitRunner for BlockingConfigRunner {
    fn run(&self, command: GitCommand) -> Result<GitOutput, AppError> {
        let is_config = command
            .args
            .iter()
            .any(|argument| argument.to_string_lossy() == "config");
        let entered = if is_config {
            self.config_entered.lock().unwrap().take()
        } else {
            None
        };
        if let Some(entered) = entered {
            entered.send(()).unwrap();
            self.release_config.lock().unwrap().recv().unwrap();
        }
        self.inner.run(command)
    }
}

impl RecordingRunner {
    fn new(delay: Duration, fail: bool) -> Self {
        Self {
            calls: Arc::new(Mutex::new(Vec::new())),
            active: Arc::new(Mutex::new(0)),
            max_active: Arc::new(Mutex::new(0)),
            delay,
            fail,
            config_output: Arc::new(Vec::new()),
        }
    }

    fn with_config_output(mut self, output: &[u8]) -> Self {
        self.config_output = Arc::new(output.to_vec());
        self
    }

    fn calls(&self) -> Vec<Vec<String>> {
        self.calls.lock().unwrap().clone()
    }

    fn max_active(&self) -> usize {
        *self.max_active.lock().unwrap()
    }

    fn reset_activity(&self) {
        assert_eq!(*self.active.lock().unwrap(), 0);
        *self.max_active.lock().unwrap() = 0;
    }

    fn clear_calls(&self) {
        self.calls.lock().unwrap().clear();
    }
}

impl GitRunner for RecordingRunner {
    fn run(&self, command: GitCommand) -> Result<GitOutput, AppError> {
        let args = command
            .args
            .iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect::<Vec<_>>();
        self.calls.lock().unwrap().push(args.clone());
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
        let is_config = args.iter().any(|arg| arg == "config");
        let is_diff = args.iter().any(|arg| arg == "diff");
        let stdout = if self.fail || is_diff {
            Vec::new()
        } else if is_config {
            self.config_output.as_ref().clone()
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

fn snapshot_count(db: &Database, repository_id: &str) -> i64 {
    db.with_connection(|connection| {
        connection.query_row(
            "SELECT COUNT(*) FROM repository_snapshots WHERE repository_id=?1",
            [repository_id],
            |row| row.get(0),
        )
    })
    .unwrap()
}

fn filtered_repository(kind: &str) -> tempfile::TempDir {
    let root = tempdir().unwrap();
    run_git(root.path(), &["init", "--quiet"]);
    run_git(root.path(), &["config", "user.name", "Fixture"]);
    run_git(
        root.path(),
        &["config", "user.email", "fixture@example.test"],
    );
    fs::write(root.path().join("tracked.txt"), "before\n").unwrap();
    fs::write(
        root.path().join(".gitattributes"),
        "tracked.txt filter=evil\n",
    )
    .unwrap();
    run_git(root.path(), &["add", "--", "tracked.txt", ".gitattributes"]);
    run_git(root.path(), &["commit", "--quiet", "-m", "seed"]);

    let script = root.path().join(".git").join(format!("filter-{kind}"));
    let contents = match kind {
        "clean" => "#!/bin/sh\nprintf invoked > clean-filter-marker\ncat\n",
        "process" => "#!/bin/sh\nprintf invoked > process-filter-marker\nexit 1\n",
        _ => panic!("unknown filter fixture"),
    };
    fs::write(&script, contents).unwrap();
    make_executable(&script);
    run_git(
        root.path(),
        &[
            "config",
            &format!("filter.evil.{kind}"),
            &format!(".git/filter-{kind}"),
        ],
    );
    run_git(root.path(), &["config", "filter.evil.required", "true"]);
    fs::write(root.path().join("tracked.txt"), "after\n").unwrap();
    root
}

fn make_executable(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(path, permissions).unwrap();
    }
    #[cfg(windows)]
    let _ = path;
}

fn has_disabled_fsmonitor_config(args: &[String]) -> bool {
    args.windows(2)
        .any(|pair| pair == ["-c", "core.fsmonitor=false"])
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
