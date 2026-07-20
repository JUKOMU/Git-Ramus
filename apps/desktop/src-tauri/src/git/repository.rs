use super::model::*;
use crate::{
    db::{Database, map_constraint_error},
    error::AppError,
};
use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, Row, params};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

/// Shared per-repository write serialization. GitService and IdentityService receive the same
/// registry from AppState so config application cannot interleave with Stage/Commit.
#[derive(Clone, Default)]
pub struct RepositoryWriteLocks {
    locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
}

impl RepositoryWriteLocks {
    pub fn lock_for(&self, repository_id: &str) -> Arc<Mutex<()>> {
        let mut locks = self
            .locks
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        locks
            .entry(repository_id.to_owned())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }
}

fn dt(s: String) -> Result<DateTime<Utc>, rusqlite::Error> {
    DateTime::parse_from_rfc3339(&s)
        .map(|x| x.with_timezone(&Utc))
        .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))
}

#[derive(Clone)]
pub struct ProjectRepository {
    db: Database,
}
impl ProjectRepository {
    pub fn new(db: Database) -> Self {
        Self { db }
    }
    pub fn create(&self, p: &Project) -> Result<(), AppError> {
        let x = serde_json::to_string(&p.exclude_patterns)?;
        self.db.with_connection(|c|c.execute("INSERT INTO projects(id,root_path,name,scan_depth,exclude_patterns_json,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![p.id,p.root_path,p.name,p.scan_depth,x,p.created_at.to_rfc3339(),p.updated_at.to_rfc3339()]).map(|_|())).map_err(|e| map_constraint_error(e, "project"))
    }
    pub fn get(&self, id: &str) -> Result<Project, AppError> {
        self.db.with_connection(|c|c.query_row("SELECT id,root_path,name,scan_depth,exclude_patterns_json,created_at,updated_at FROM projects WHERE id=?1",[id],map_project).optional()).and_then(|x|x.ok_or_else(||AppError::NotFound(format!("project {id}"))))
    }

    pub fn list(&self) -> Result<Vec<Project>, AppError> {
        self.db.with_connection(|c| {
            let mut statement = c.prepare(
                "SELECT id,root_path,name,scan_depth,exclude_patterns_json,created_at,updated_at \
                 FROM projects ORDER BY name, id",
            )?;
            statement
                .query_map([], map_project)
                .map(|rows| rows.collect())?
        })
    }

    pub fn update(&self, project: &Project) -> Result<(), AppError> {
        self.update_with_root_change(project, false)
    }

    pub fn update_with_root_change(
        &self,
        project: &Project,
        root_changed: bool,
    ) -> Result<(), AppError> {
        let encoded = serde_json::to_string(&project.exclude_patterns)?;
        let changed = self
            .db
            .with_transaction(|transaction| {
                let changed = transaction.execute(
                "UPDATE projects SET root_path=?2,name=?3,scan_depth=?4,exclude_patterns_json=?5,updated_at=?6 WHERE id=?1",
                params![
                    project.id,
                    project.root_path,
                    project.name,
                    project.scan_depth,
                    encoded,
                    project.updated_at.to_rfc3339()
                ],
                )?;
                if changed != 0 && root_changed {
                    transaction.execute(
                        "DELETE FROM project_repositories WHERE project_id=?1",
                        [&project.id],
                    )?;
                }
                Ok(changed)
            })
            .map_err(|error| map_constraint_error(error, "project"))?;
        if changed == 0 {
            return Err(AppError::NotFound(format!("project {}", project.id)));
        }
        Ok(())
    }

    pub fn delete(&self, id: &str) -> Result<(), AppError> {
        let changed = self
            .db
            .with_transaction(|transaction| {
                transaction.execute(
                    "UPDATE git_clone_operations SET project_id=NULL,updated_at=?2 WHERE project_id=?1 AND current_stage IN ('completed','failed','cancelled')",
                    params![id, Utc::now().to_rfc3339()],
                )?;
                transaction.execute("DELETE FROM projects WHERE id=?1", [id])
            })
            .map_err(|error| map_constraint_error(error, "project"))?;
        if changed == 0 {
            return Err(AppError::NotFound(format!("project {id}")));
        }
        Ok(())
    }
}
fn map_project(r: &Row) -> Result<Project, rusqlite::Error> {
    let ex: String = r.get(4)?;
    Ok(Project {
        id: r.get(0)?,
        root_path: r.get(1)?,
        name: r.get(2)?,
        scan_depth: r.get(3)?,
        exclude_patterns: serde_json::from_str(&ex)
            .map_err(|e| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
        created_at: dt(r.get(5)?)?,
        updated_at: dt(r.get(6)?)?,
    })
}

#[derive(Clone)]
pub struct WorkspaceRepository {
    db: Database,
}
impl WorkspaceRepository {
    pub fn new(db: Database) -> Self {
        Self { db }
    }
    pub fn create(&self, w: &Workspace) -> Result<(), AppError> {
        self.db
            .with_connection(|c| {
                c.execute(
                    "INSERT INTO workspaces(id,name,created_at,updated_at) VALUES(?1,?2,?3,?4)",
                    params![
                        w.id,
                        w.name,
                        w.created_at.to_rfc3339(),
                        w.updated_at.to_rfc3339()
                    ],
                )
                .map(|_| ())
            })
            .map_err(|e| map_constraint_error(e, "workspace"))
    }
    pub fn set_projects(&self, id: &str, projects: &[String]) -> Result<(), AppError> {
        self.db.with_transaction(|tx|{tx.execute("DELETE FROM workspace_projects WHERE workspace_id=?1",[id])?; for (i,p) in projects.iter().enumerate(){tx.execute("INSERT INTO workspace_projects(workspace_id,project_id,position) VALUES(?1,?2,?3)",params![id,p,i as i64])?;} Ok(())}).map_err(|e| map_constraint_error(e, "workspace membership"))
    }
    pub fn add_project(&self, workspace_id: &str, project_id: &str) -> Result<(), AppError> {
        self.db.with_transaction(|tx| {
            let position: i64 = tx.query_row("SELECT COALESCE(MAX(position), -1) + 1 FROM workspace_projects WHERE workspace_id=?1", [workspace_id], |r| r.get(0))?;
            tx.execute("INSERT INTO workspace_projects(workspace_id,project_id,position) VALUES(?1,?2,?3)", params![workspace_id, project_id, position]).map(|_| ())
        }).map_err(|e| map_constraint_error(e, "workspace membership"))
    }
    pub fn remove_project(&self, workspace_id: &str, project_id: &str) -> Result<(), AppError> {
        self.db
            .with_transaction(|tx| {
                tx.execute(
                    "DELETE FROM workspace_projects WHERE workspace_id=?1 AND project_id=?2",
                    params![workspace_id, project_id],
                )
                .map(|_| ())
            })
            .map_err(|e| map_constraint_error(e, "workspace membership removal"))
    }
    pub fn projects(&self, id: &str) -> Result<Vec<String>, AppError> {
        self.db.with_connection(|c| {
            let mut s = c.prepare(
                "SELECT project_id FROM workspace_projects WHERE workspace_id=?1 ORDER BY position",
            )?;
            s.query_map([id], |r| r.get(0))?.collect()
        })
    }

    pub fn get(&self, id: &str) -> Result<Workspace, AppError> {
        self.db
            .with_connection(|c| {
                c.query_row(
                    "SELECT id,name,created_at,updated_at FROM workspaces WHERE id=?1",
                    [id],
                    map_workspace,
                )
                .optional()
            })
            .and_then(|value| value.ok_or_else(|| AppError::NotFound(format!("workspace {id}"))))
    }

    pub fn list(&self) -> Result<Vec<Workspace>, AppError> {
        self.db.with_connection(|c| {
            let mut statement =
                c.prepare("SELECT id,name,created_at,updated_at FROM workspaces ORDER BY name,id")?;
            statement
                .query_map([], map_workspace)
                .map(|rows| rows.collect())?
        })
    }

    pub fn update(&self, workspace: &Workspace) -> Result<(), AppError> {
        let changed = self.db.with_connection(|c| {
            c.execute(
                "UPDATE workspaces SET name=?2,updated_at=?3 WHERE id=?1",
                params![
                    workspace.id,
                    workspace.name,
                    workspace.updated_at.to_rfc3339()
                ],
            )
        })?;
        if changed == 0 {
            return Err(AppError::NotFound(format!("workspace {}", workspace.id)));
        }
        Ok(())
    }

    pub fn delete(&self, id: &str) -> Result<(), AppError> {
        let changed = self
            .db
            .with_connection(|c| c.execute("DELETE FROM workspaces WHERE id=?1", [id]))?;
        if changed == 0 {
            return Err(AppError::NotFound(format!("workspace {id}")));
        }
        Ok(())
    }
}

fn map_workspace(r: &Row) -> Result<Workspace, rusqlite::Error> {
    Ok(Workspace {
        id: r.get(0)?,
        name: r.get(1)?,
        created_at: dt(r.get(2)?)?,
        updated_at: dt(r.get(3)?)?,
    })
}

#[derive(Clone)]
pub struct RepositoryRepository {
    db: Database,
}
impl RepositoryRepository {
    pub fn new(db: Database) -> Self {
        Self { db }
    }
    pub fn create(&self, r: &Repository) -> Result<(), AppError> {
        self.db.with_connection(|c|c.execute("INSERT INTO repositories(id,canonical_path,display_name,kind,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6)",params![r.id,r.canonical_path,r.display_name,r.kind.as_str(),r.created_at.to_rfc3339(),r.updated_at.to_rfc3339()]).map(|_|())).map_err(|e| map_constraint_error(e, "repository"))
    }
    pub fn get_or_create(&self, repository: &Repository) -> Result<Repository, AppError> {
        self.db
            .with_transaction(|transaction| {
                transaction.execute(
                    "INSERT INTO repositories(id,canonical_path,display_name,kind,created_at,updated_at)
                     VALUES(?1,?2,?3,?4,?5,?6)
                     ON CONFLICT(canonical_path) DO NOTHING",
                    params![
                        repository.id,
                        repository.canonical_path,
                        repository.display_name,
                        repository.kind.as_str(),
                        repository.created_at.to_rfc3339(),
                        repository.updated_at.to_rfc3339()
                    ],
                )?;
                transaction.query_row(
                    "SELECT id,canonical_path,display_name,kind,created_at,updated_at
                     FROM repositories WHERE canonical_path=?1",
                    [&repository.canonical_path],
                    map_repo,
                )
            })
            .map_err(|error| map_constraint_error(error, "repository"))
    }
    pub fn get(&self, id: &str) -> Result<Repository, AppError> {
        self.db.with_connection(|c|c.query_row("SELECT id,canonical_path,display_name,kind,created_at,updated_at FROM repositories WHERE id=?1",[id],map_repo).optional()).and_then(|x|x.ok_or_else(||AppError::NotFound(format!("repository {id}"))))
    }

    pub fn get_by_canonical_path(&self, path: &str) -> Result<Option<Repository>, AppError> {
        self.db.with_connection(|c| {
            c.query_row(
                "SELECT id,canonical_path,display_name,kind,created_at,updated_at FROM repositories WHERE canonical_path=?1",
                [path],
                map_repo,
            )
            .optional()
        })
    }
    pub fn add_to_project(
        &self,
        project_id: &str,
        repository_id: &str,
        relative_path: &str,
    ) -> Result<(), AppError> {
        self.db.with_transaction(|tx| tx.execute("INSERT INTO project_repositories(project_id,repository_id,relative_path) VALUES(?1,?2,?3) ON CONFLICT(project_id,repository_id) DO UPDATE SET relative_path=excluded.relative_path", params![project_id, repository_id, relative_path]).map(|_|())).map_err(|e| map_constraint_error(e, "project repository relationship"))
    }
    pub fn remove_from_project(
        &self,
        project_id: &str,
        repository_id: &str,
    ) -> Result<(), AppError> {
        self.db
            .with_transaction(|tx| {
                tx.execute(
                    "DELETE FROM project_repositories WHERE project_id=?1 AND repository_id=?2",
                    params![project_id, repository_id],
                )
                .map(|_| ())
            })
            .map_err(|e| map_constraint_error(e, "project repository relationship removal"))
    }
    pub fn add_remote(&self, remote: &Remote) -> Result<(), AppError> {
        self.db.with_connection(|c| c.execute("INSERT INTO repository_remotes(repository_id,name,fetch_url,push_url) VALUES(?1,?2,?3,?4) ON CONFLICT(repository_id,name) DO UPDATE SET fetch_url=excluded.fetch_url,push_url=excluded.push_url", params![remote.repository_id, remote.name, remote.fetch_url, remote.push_url]).map(|_|())).map_err(|e| map_constraint_error(e, "repository remote"))
    }
    pub fn replace_remotes(&self, repository_id: &str, remotes: &[Remote]) -> Result<(), AppError> {
        if remotes
            .iter()
            .any(|remote| remote.repository_id != repository_id)
        {
            return Err(AppError::InvalidInput(
                "remote belongs to another repository".to_owned(),
            ));
        }
        let mut incoming_names = std::collections::BTreeSet::new();
        if remotes
            .iter()
            .any(|remote| !incoming_names.insert(remote.name.as_str()))
        {
            return Err(AppError::InvalidInput(
                "remote names must be unique".to_owned(),
            ));
        }
        self.db
            .with_transaction(|transaction| {
                let existing_names = {
                    let mut statement = transaction.prepare(
                        "SELECT name FROM repository_remotes WHERE repository_id=?1",
                    )?;
                    statement
                        .query_map([repository_id], |row| row.get::<_, String>(0))?
                        .collect::<Result<Vec<_>, _>>()?
                };
                for remote in remotes {
                    transaction.execute(
                        "INSERT INTO repository_remotes(repository_id,name,fetch_url,push_url) VALUES(?1,?2,?3,?4) ON CONFLICT(repository_id,name) DO UPDATE SET fetch_url=excluded.fetch_url,push_url=excluded.push_url",
                        params![
                            remote.repository_id,
                            remote.name,
                            remote.fetch_url,
                            remote.push_url
                        ],
                    )?;
                }
                for stale_name in existing_names {
                    if !incoming_names.contains(stale_name.as_str()) {
                        transaction.execute(
                            "DELETE FROM repository_remotes WHERE repository_id=?1 AND name=?2",
                            params![repository_id, stale_name],
                        )?;
                    }
                }
                Ok(())
            })
            .map_err(|error| map_constraint_error(error, "repository remotes"))
    }
    pub fn get_remote(&self, repository_id: &str, name: &str) -> Result<Remote, AppError> {
        self.db.with_connection(|c| c.query_row("SELECT repository_id,name,fetch_url,push_url FROM repository_remotes WHERE repository_id=?1 AND name=?2", params![repository_id, name], |r| Ok(Remote { repository_id: r.get(0)?, name: r.get(1)?, fetch_url: r.get(2)?, push_url: r.get(3)? })).optional()).and_then(|v| v.ok_or_else(|| AppError::NotFound(format!("remote {repository_id}/{name}"))))
    }
    pub fn list_remotes(&self, repository_id: &str) -> Result<Vec<Remote>, AppError> {
        self.db.with_connection(|c| { let mut s = c.prepare("SELECT repository_id,name,fetch_url,push_url FROM repository_remotes WHERE repository_id=?1 ORDER BY name")?; s.query_map([repository_id], |r| Ok(Remote { repository_id: r.get(0)?, name: r.get(1)?, fetch_url: r.get(2)?, push_url: r.get(3)? })).map(|rows| rows.collect())? })
    }
    pub fn list_for_project(&self, project_id: &str) -> Result<Vec<Repository>, AppError> {
        self.db.with_connection(|c| { let mut s = c.prepare("SELECT r.id,r.canonical_path,r.display_name,r.kind,r.created_at,r.updated_at FROM repositories r INNER JOIN project_repositories pr ON pr.repository_id=r.id WHERE pr.project_id=?1 ORDER BY pr.relative_path")?; s.query_map([project_id], map_repo).map(|rows| rows.collect())? })
    }

    pub fn relative_path(&self, project_id: &str, repository_id: &str) -> Result<String, AppError> {
        self.db
            .with_connection(|c| {
                c.query_row(
                    "SELECT relative_path FROM project_repositories WHERE project_id=?1 AND repository_id=?2",
                    params![project_id, repository_id],
                    |row| row.get(0),
                )
                .optional()
            })
            .and_then(|value| {
                value.ok_or_else(|| {
                    AppError::NotFound(format!(
                        "repository {repository_id} in project {project_id}"
                    ))
                })
            })
    }

    pub fn is_in_project(&self, project_id: &str, repository_id: &str) -> Result<bool, AppError> {
        self.db.with_connection(|c| {
            c.query_row(
                "SELECT 1 FROM project_repositories WHERE project_id=?1 AND repository_id=?2",
                params![project_id, repository_id],
                |_| Ok(true),
            )
            .optional()
            .map(|value| value.unwrap_or(false))
        })
    }

    pub fn list_for_workspace(&self, workspace_id: &str) -> Result<Vec<Repository>, AppError> {
        self.db.with_connection(|c| {
            let mut statement = c.prepare(
                "SELECT DISTINCT r.id,r.canonical_path,r.display_name,r.kind,r.created_at,r.updated_at
                 FROM repositories r
                 INNER JOIN project_repositories pr ON pr.repository_id=r.id
                 INNER JOIN workspace_projects wp ON wp.project_id=pr.project_id
                 WHERE wp.workspace_id=?1 ORDER BY r.display_name,r.id",
            )?;
            statement
                .query_map([workspace_id], map_repo)
                .map(|rows| rows.collect())?
        })
    }

    pub fn is_in_workspace(
        &self,
        workspace_id: &str,
        repository_id: &str,
    ) -> Result<bool, AppError> {
        self.db.with_connection(|c| {
            c.query_row(
                "SELECT 1 FROM project_repositories pr INNER JOIN workspace_projects wp ON wp.project_id=pr.project_id WHERE wp.workspace_id=?1 AND pr.repository_id=?2 LIMIT 1",
                params![workspace_id, repository_id],
                |_| Ok(true),
            )
            .optional()
            .map(|value| value.unwrap_or(false))
        })
    }

    pub fn list_all(&self) -> Result<Vec<Repository>, AppError> {
        self.db.with_connection(|c| {
            let mut statement = c.prepare(
                "SELECT id,canonical_path,display_name,kind,created_at,updated_at FROM repositories ORDER BY display_name,id",
            )?;
            statement
                .query_map([], map_repo)
                .map(|rows| rows.collect())?
        })
    }
}
fn map_repo(r: &Row) -> Result<Repository, rusqlite::Error> {
    let k: String = r.get(3)?;
    Ok(Repository {
        id: r.get(0)?,
        canonical_path: r.get(1)?,
        display_name: r.get(2)?,
        kind: k
            .parse()
            .map_err(|e: AppError| rusqlite::Error::ToSqlConversionFailure(Box::new(e)))?,
        created_at: dt(r.get(4)?)?,
        updated_at: dt(r.get(5)?)?,
    })
}

#[derive(Clone)]
pub struct SnapshotRepository {
    db: Database,
}
impl SnapshotRepository {
    pub fn new(db: Database) -> Self {
        Self { db }
    }
    pub fn upsert(&self, s: &RepositorySnapshot) -> Result<(), AppError> {
        self.db.with_connection(|c|c.execute("INSERT INTO repository_snapshots(id,repository_id,captured_at,head_oid,branch,upstream,ahead,behind,dirty,staged_count,unstaged_count,untracked_count,conflicted_count,refresh_error_summary) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14) ON CONFLICT(id) DO UPDATE SET repository_id=excluded.repository_id,captured_at=excluded.captured_at,head_oid=excluded.head_oid,branch=excluded.branch,upstream=excluded.upstream,ahead=excluded.ahead,behind=excluded.behind,dirty=excluded.dirty,staged_count=excluded.staged_count,unstaged_count=excluded.unstaged_count,untracked_count=excluded.untracked_count,conflicted_count=excluded.conflicted_count,refresh_error_summary=excluded.refresh_error_summary",params![s.id,s.repository_id,s.captured_at.to_rfc3339(),s.head_oid,s.branch,s.upstream,s.ahead,s.behind,s.dirty,s.staged_count,s.unstaged_count,s.untracked_count,s.conflicted_count,s.refresh_error_summary]).map(|_|())).map_err(|e| map_constraint_error(e, "repository snapshot"))
    }
    pub fn get(&self, id: &str) -> Result<RepositorySnapshot, AppError> {
        self.db.with_connection(|c|c.query_row("SELECT id,repository_id,captured_at,head_oid,branch,upstream,ahead,behind,dirty,staged_count,unstaged_count,untracked_count,conflicted_count,refresh_error_summary FROM repository_snapshots WHERE id=?1",[id],map_snapshot).optional()).and_then(|x|x.ok_or_else(||AppError::NotFound(format!("snapshot {id}"))))
    }

    pub fn latest_for_repository(
        &self,
        repository_id: &str,
    ) -> Result<Option<RepositorySnapshot>, AppError> {
        self.db.with_connection(|c| {
            c.query_row(
                "SELECT id,repository_id,captured_at,head_oid,branch,upstream,ahead,behind,dirty,staged_count,unstaged_count,untracked_count,conflicted_count,refresh_error_summary
                 FROM repository_snapshots WHERE repository_id=?1 ORDER BY captured_at DESC, rowid DESC LIMIT 1",
                [repository_id],
                map_snapshot,
            )
            .optional()
        })
    }

    pub fn latest_successful_for_repository(
        &self,
        repository_id: &str,
    ) -> Result<Option<RepositorySnapshot>, AppError> {
        self.db.with_connection(|c| {
            c.query_row(
                "SELECT id,repository_id,captured_at,head_oid,branch,upstream,ahead,behind,dirty,staged_count,unstaged_count,untracked_count,conflicted_count,refresh_error_summary
                 FROM repository_snapshots WHERE repository_id=?1 AND refresh_error_summary IS NULL ORDER BY captured_at DESC, rowid DESC LIMIT 1",
                [repository_id],
                map_snapshot,
            )
            .optional()
        })
    }

    pub fn list_for_repository(
        &self,
        repository_id: &str,
    ) -> Result<Vec<RepositorySnapshot>, AppError> {
        self.db.with_connection(|c| {
            let mut statement = c.prepare(
                "SELECT id,repository_id,captured_at,head_oid,branch,upstream,ahead,behind,dirty,staged_count,unstaged_count,untracked_count,conflicted_count,refresh_error_summary
                 FROM repository_snapshots WHERE repository_id=?1 ORDER BY captured_at DESC, rowid DESC",
            )?;
            statement
                .query_map([repository_id], map_snapshot)
                .map(|rows| rows.collect())?
        })
    }
}
fn map_snapshot(r: &Row) -> Result<RepositorySnapshot, rusqlite::Error> {
    Ok(RepositorySnapshot {
        id: r.get(0)?,
        repository_id: r.get(1)?,
        captured_at: dt(r.get(2)?)?,
        head_oid: r.get(3)?,
        branch: r.get(4)?,
        upstream: r.get(5)?,
        ahead: r.get(6)?,
        behind: r.get(7)?,
        dirty: r.get(8)?,
        staged_count: r.get(9)?,
        unstaged_count: r.get(10)?,
        untracked_count: r.get(11)?,
        conflicted_count: r.get(12)?,
        refresh_error_summary: r.get(13)?,
    })
}

#[derive(Clone)]
pub struct TrustRepository {
    db: Database,
}

#[derive(Clone)]
pub struct IdentityBindingRepository {
    db: Database,
}
impl IdentityBindingRepository {
    pub fn new(db: Database) -> Self {
        Self { db }
    }
    pub fn bind(&self, repository_id: &str, identity_profile_id: &str) -> Result<(), AppError> {
        self.db.with_connection(|c| c.execute("INSERT INTO repository_identity_bindings(repository_id,identity_profile_id,managed,bound_at) VALUES(?1,?2,1,?3) ON CONFLICT(repository_id) DO UPDATE SET identity_profile_id=excluded.identity_profile_id,managed=excluded.managed,bound_at=excluded.bound_at", params![repository_id, identity_profile_id, chrono::Utc::now().to_rfc3339()]).map(|_| ())).map_err(|e| map_constraint_error(e, "repository identity binding"))
    }
    pub fn get(&self, repository_id: &str) -> Result<IdentityBinding, AppError> {
        self.get_optional(repository_id)?
            .ok_or_else(|| AppError::NotFound(format!("identity binding {repository_id}")))
    }
    pub fn get_optional(&self, repository_id: &str) -> Result<Option<IdentityBinding>, AppError> {
        self.db.with_connection(|c| c.query_row("SELECT repository_id,identity_profile_id,managed,bound_at FROM repository_identity_bindings WHERE repository_id=?1", [repository_id], |r| Ok(IdentityBinding { repository_id: r.get(0)?, identity_profile_id: r.get(1)?, managed: r.get(2)?, bound_at: dt(r.get(3)?)? })).optional())
    }
    pub fn unbind(&self, repository_id: &str) -> Result<(), AppError> {
        self.db.with_connection(|connection| {
            connection
                .execute(
                    "DELETE FROM repository_identity_bindings WHERE repository_id=?1",
                    [repository_id],
                )
                .map(|_| ())
        })
    }
    pub fn count_for_profile(&self, identity_profile_id: &str) -> Result<i64, AppError> {
        self.db.with_connection(|connection| {
            connection.query_row(
                "SELECT COUNT(*) FROM repository_identity_bindings WHERE identity_profile_id=?1",
                [identity_profile_id],
                |row| row.get(0),
            )
        })
    }
}
impl TrustRepository {
    pub fn new(db: Database) -> Self {
        Self { db }
    }
    pub fn set(&self, trust: &Trust) -> Result<(), AppError> {
        self.db.with_connection(|c| c.execute("INSERT INTO trusted_repositories(repository_id,trusted_at,trust_version) VALUES(?1,?2,?3) ON CONFLICT(repository_id) DO UPDATE SET trusted_at=excluded.trusted_at,trust_version=excluded.trust_version", params![trust.repository_id, trust.trusted_at.to_rfc3339(), trust.trust_version]).map(|_| ())).map_err(|e| map_constraint_error(e, "repository trust"))
    }
    pub fn is_trusted(&self, repository_id: &str) -> Result<bool, AppError> {
        self.db
            .with_connection(|c| {
                c.query_row(
                    "SELECT 1 FROM trusted_repositories WHERE repository_id=?1",
                    [repository_id],
                    |_| Ok(true),
                )
                .optional()
            })
            .map(|x| x.unwrap_or(false))
    }
}
