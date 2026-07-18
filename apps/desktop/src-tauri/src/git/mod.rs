#[cfg(test)]
mod tests {
    use crate::db::Database;

    #[test]
    fn project_duplicate_root_is_rejected() {
        let db = Database::open_in_memory().unwrap();
        let repo = super::repository::ProjectRepository::new(db);
        let project = super::model::Project::new("/tmp/repo", "Repo");
        repo.create(&project).unwrap();
        let duplicate = super::model::Project::new("/tmp/repo", "Repo copy");
        let error = repo.create(&duplicate).unwrap_err();
        assert!(matches!(error, crate::error::AppError::InvalidInput(_)));
    }

    #[test]
    fn workspace_membership_removal_preserves_projects() {
        let db = Database::open_in_memory().unwrap();
        let projects = super::repository::ProjectRepository::new(db.clone());
        let workspaces = super::repository::WorkspaceRepository::new(db);
        let a = super::model::Project::new("/tmp/a", "A");
        let b = super::model::Project::new("/tmp/b", "B");
        projects.create(&a).unwrap();
        projects.create(&b).unwrap();
        let workspace = super::model::Workspace::new("Main");
        workspaces.create(&workspace).unwrap();
        workspaces
            .set_projects(&workspace.id, &[a.id.clone(), b.id.clone()])
            .unwrap();
        workspaces
            .set_projects(&workspace.id, &[a.id.clone()])
            .unwrap();
        assert_eq!(projects.get(&b.id).unwrap().id, b.id);
    }

    #[test]
    fn duplicate_repository_canonical_path_is_rejected() {
        let db = Database::open_in_memory().unwrap();
        let repos = super::repository::RepositoryRepository::new(db);
        let first = super::model::Repository::new(
            "/tmp/repo",
            "Repo",
            super::model::RepositoryKind::Normal,
        );
        repos.create(&first).unwrap();
        let duplicate =
            super::model::Repository::new("/tmp/repo", "Other", super::model::RepositoryKind::Bare);
        assert!(matches!(
            repos.create(&duplicate),
            Err(crate::error::AppError::InvalidInput(_))
        ));
    }

    #[test]
    fn project_repository_many_to_many_and_snapshot_upsert_round_trip() {
        let db = Database::open_in_memory().unwrap();
        let projects = super::repository::ProjectRepository::new(db.clone());
        let repos = super::repository::RepositoryRepository::new(db.clone());
        let snapshots = super::repository::SnapshotRepository::new(db.clone());
        let project = super::model::Project::new("/tmp/p", "P");
        let repo =
            super::model::Repository::new("/tmp/r", "R", super::model::RepositoryKind::Normal);
        projects.create(&project).unwrap();
        repos.create(&repo).unwrap();
        repos.add_to_project(&project.id, &repo.id, "src").unwrap();
        db.with_connection(|c| {
            c.execute(
                "DELETE FROM project_repositories WHERE project_id=?1 AND repository_id=?2",
                rusqlite::params![project.id, repo.id],
            )
        })
        .unwrap();
        assert_eq!(projects.get(&project.id).unwrap().id, project.id);
        assert_eq!(repos.get(&repo.id).unwrap().id, repo.id);
        repos.add_to_project(&project.id, &repo.id, "src").unwrap();
        let mut snapshot = super::model::RepositorySnapshot::new(&repo.id);
        snapshot.head_oid = Some("abc".into());
        snapshot.branch = Some("main".into());
        snapshot.upstream = Some("origin/main".into());
        snapshot.ahead = 1;
        snapshot.behind = 2;
        snapshot.dirty = true;
        snapshot.staged_count = 3;
        snapshot.unstaged_count = 4;
        snapshot.untracked_count = 5;
        snapshot.conflicted_count = 6;
        snapshot.refresh_error_summary = Some("old".into());
        snapshots.upsert(&snapshot).unwrap();
        snapshot.refresh_error_summary = Some("new".into());
        snapshot.untracked_count = 2;
        snapshots.upsert(&snapshot).unwrap();
        let loaded = snapshots.get(&snapshot.id).unwrap();
        assert_eq!(loaded.refresh_error_summary.as_deref(), Some("new"));
        assert_eq!(loaded.untracked_count, 2);
        assert_eq!(loaded.head_oid.as_deref(), Some("abc"));
        assert_eq!(loaded.branch.as_deref(), Some("main"));
        assert_eq!(loaded.upstream.as_deref(), Some("origin/main"));
        assert_eq!((loaded.ahead, loaded.behind), (1, 2));
        assert!(loaded.dirty);
        assert_eq!(
            (
                loaded.staged_count,
                loaded.unstaged_count,
                loaded.conflicted_count
            ),
            (3, 4, 6)
        );
    }

    #[test]
    fn repository_identity_binding_is_one_per_repository_and_not_cascaded() {
        let db = Database::open_in_memory().unwrap();
        let repos = super::repository::RepositoryRepository::new(db.clone());
        let identities = crate::identity::IdentityProfileRepository::new(db.clone());
        let bindings = super::repository::IdentityBindingRepository::new(db.clone());
        let repo =
            super::model::Repository::new("/tmp/b", "B", super::model::RepositoryKind::Normal);
        let identity = crate::identity::IdentityProfile::new("A", "a", "a@example.com");
        repos.create(&repo).unwrap();
        identities.create(&identity).unwrap();
        bindings.bind(&repo.id, &identity.id).unwrap();
        assert_eq!(
            bindings.get(&repo.id).unwrap().identity_profile_id,
            identity.id
        );
        assert!(
            db.with_connection(|c| c.execute("DELETE FROM repositories WHERE id=?1", [&repo.id]))
                .is_err()
        );
    }

    #[test]
    fn scan_depth_defaults_to_three_and_relationship_lists_round_trip() {
        let db = Database::open_in_memory().unwrap();
        let projects = super::repository::ProjectRepository::new(db.clone());
        let repos = super::repository::RepositoryRepository::new(db.clone());
        let p1 = super::model::Project::new("/tmp/p1", "P1");
        let p2 = super::model::Project::new("/tmp/p2", "P2");
        let r1 =
            super::model::Repository::new("/tmp/r1", "R1", super::model::RepositoryKind::Normal);
        let r2 =
            super::model::Repository::new("/tmp/r2", "R2", super::model::RepositoryKind::Normal);
        projects.create(&p1).unwrap();
        projects.create(&p2).unwrap();
        repos.create(&r1).unwrap();
        repos.create(&r2).unwrap();
        assert_eq!(p1.scan_depth, 3);
        repos.add_to_project(&p1.id, &r1.id, "one").unwrap();
        repos.add_to_project(&p1.id, &r2.id, "two").unwrap();
        repos.add_to_project(&p2.id, &r1.id, "shared").unwrap();
        assert_eq!(repos.list_for_project(&p1.id).unwrap().len(), 2);
        assert_eq!(repos.list_for_project(&p2.id).unwrap().len(), 1);
        db.with_connection(|c| {
            c.execute(
                "DELETE FROM project_repositories WHERE project_id=?1 AND repository_id=?2",
                rusqlite::params![p1.id, r1.id],
            )
        })
        .unwrap();
        assert_eq!(repos.get(&r1.id).unwrap().id, r1.id);
    }

    #[test]
    fn invalid_relationship_and_check_errors_are_validation_errors() {
        let db = Database::open_in_memory().unwrap();
        let repos = super::repository::RepositoryRepository::new(db.clone());
        let missing = super::model::Repository::new(
            "/tmp/missing",
            "M",
            super::model::RepositoryKind::Normal,
        );
        assert!(
            matches!(repos.add_to_project("no-project", &missing.id, ""), Err(crate::error::AppError::InvalidInput(message)) if message.contains("project"))
        );
        let projects = super::repository::ProjectRepository::new(db);
        let mut project = super::model::Project::new("/tmp/check", "C");
        project.scan_depth = -1;
        assert!(matches!(
            projects.create(&project),
            Err(crate::error::AppError::InvalidInput(_))
        ));
    }

    #[test]
    fn remote_and_trust_round_trip() {
        let db = Database::open_in_memory().unwrap();
        let repos = super::repository::RepositoryRepository::new(db.clone());
        let repo =
            super::model::Repository::new("/tmp/remote", "R", super::model::RepositoryKind::Normal);
        repos.create(&repo).unwrap();
        let remote = super::model::Remote {
            repository_id: repo.id.clone(),
            name: "origin".into(),
            fetch_url: Some("fetch".into()),
            push_url: Some("push".into()),
        };
        repos.add_remote(&remote).unwrap();
        assert_eq!(
            repos
                .get_remote(&repo.id, "origin")
                .unwrap()
                .fetch_url
                .as_deref(),
            Some("fetch")
        );
        assert_eq!(repos.list_remotes(&repo.id).unwrap().len(), 1);
        let trust = super::model::Trust {
            repository_id: repo.id.clone(),
            trusted_at: chrono::Utc::now(),
            trust_version: 2,
        };
        let trusts = super::repository::TrustRepository::new(db);
        trusts.set(&trust).unwrap();
        assert!(trusts.is_trusted(&repo.id).unwrap());
    }
}

pub mod model;
pub mod repository;
