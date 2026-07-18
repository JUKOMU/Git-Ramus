#[cfg(test)]
mod tests {
    use crate::db::Database;

    #[test]
    fn project_duplicate_root_is_rejected() {
        let db = Database::open_in_memory().unwrap();
        let repo = super::repository::ProjectRepository::new(db);
        let project = super::model::Project::new("/tmp/repo", "Repo");
        repo.create(&project).unwrap();
        let error = repo.create(&project).unwrap_err();
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
}

pub mod model;
pub mod repository;
