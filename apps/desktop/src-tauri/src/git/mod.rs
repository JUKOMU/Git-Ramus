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
        snapshot.refresh_error_summary = Some("old".into());
        snapshots.upsert(&snapshot).unwrap();
        snapshot.refresh_error_summary = Some("new".into());
        snapshot.untracked_count = 2;
        snapshots.upsert(&snapshot).unwrap();
        let loaded = snapshots.get(&snapshot.id).unwrap();
        assert_eq!(loaded.refresh_error_summary.as_deref(), Some("new"));
        assert_eq!(loaded.untracked_count, 2);
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
}

pub mod model;
pub mod repository;
