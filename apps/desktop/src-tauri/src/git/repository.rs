use super::model::*;
use crate::{db::Database, error::AppError};
use chrono::{DateTime, Utc};
use rusqlite::{OptionalExtension, Row, params};

fn uniq(e: AppError, what: &str) -> AppError {
    if matches!(e,AppError::Database(rusqlite::Error::SqliteFailure(ref x,_)) if x.extended_code==2067 || x.extended_code==1555 || x.extended_code==19)
    {
        AppError::InvalidInput(format!("{what} already exists"))
    } else {
        e
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
        self.db.with_connection(|c|c.execute("INSERT INTO projects(id,root_path,name,scan_depth,exclude_patterns_json,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![p.id,p.root_path,p.name,p.scan_depth,x,p.created_at.to_rfc3339(),p.updated_at.to_rfc3339()]).map(|_|())).map_err(|e|uniq(e,"project root path"))
    }
    pub fn get(&self, id: &str) -> Result<Project, AppError> {
        self.db.with_connection(|c|c.query_row("SELECT id,root_path,name,scan_depth,exclude_patterns_json,created_at,updated_at FROM projects WHERE id=?1",[id],map_project).optional()).and_then(|x|x.ok_or_else(||AppError::NotFound(format!("project {id}"))))
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
            .map_err(|e| uniq(e, "workspace name"))
    }
    pub fn set_projects(&self, id: &str, projects: &[String]) -> Result<(), AppError> {
        self.db.with_transaction(|tx|{tx.execute("DELETE FROM workspace_projects WHERE workspace_id=?1",[id])?; for (i,p) in projects.iter().enumerate(){tx.execute("INSERT INTO workspace_projects(workspace_id,project_id,position) VALUES(?1,?2,?3)",params![id,p,i as i64])?;} Ok(())})
    }
    pub fn projects(&self, id: &str) -> Result<Vec<String>, AppError> {
        self.db.with_connection(|c| {
            let mut s = c.prepare(
                "SELECT project_id FROM workspace_projects WHERE workspace_id=?1 ORDER BY position",
            )?;
            s.query_map([id], |r| r.get(0))?.collect()
        })
    }
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
        self.db.with_connection(|c|c.execute("INSERT INTO repositories(id,canonical_path,display_name,kind,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6)",params![r.id,r.canonical_path,r.display_name,r.kind.as_str(),r.created_at.to_rfc3339(),r.updated_at.to_rfc3339()]).map(|_|())).map_err(|e|uniq(e,"repository canonical path"))
    }
    pub fn get(&self, id: &str) -> Result<Repository, AppError> {
        self.db.with_connection(|c|c.query_row("SELECT id,canonical_path,display_name,kind,created_at,updated_at FROM repositories WHERE id=?1",[id],map_repo).optional()).and_then(|x|x.ok_or_else(||AppError::NotFound(format!("repository {id}"))))
    }
    pub fn add_to_project(
        &self,
        project_id: &str,
        repository_id: &str,
        relative_path: &str,
    ) -> Result<(), AppError> {
        self.db.with_connection(|c| c.execute("INSERT INTO project_repositories(project_id,repository_id,relative_path) VALUES(?1,?2,?3) ON CONFLICT(project_id,repository_id) DO UPDATE SET relative_path=excluded.relative_path", params![project_id, repository_id, relative_path]).map(|_|()))
    }
    pub fn add_remote(&self, remote: &Remote) -> Result<(), AppError> {
        self.db.with_connection(|c| c.execute("INSERT INTO repository_remotes(repository_id,name,fetch_url,push_url) VALUES(?1,?2,?3,?4) ON CONFLICT(repository_id,name) DO UPDATE SET fetch_url=excluded.fetch_url,push_url=excluded.push_url", params![remote.repository_id, remote.name, remote.fetch_url, remote.push_url]).map(|_|()))
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
        self.db.with_connection(|c|c.execute("INSERT INTO repository_snapshots(id,repository_id,captured_at,head_oid,branch,upstream,ahead,behind,dirty,staged_count,unstaged_count,untracked_count,conflicted_count,refresh_error_summary) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14) ON CONFLICT(id) DO UPDATE SET repository_id=excluded.repository_id,captured_at=excluded.captured_at,head_oid=excluded.head_oid,branch=excluded.branch,upstream=excluded.upstream,ahead=excluded.ahead,behind=excluded.behind,dirty=excluded.dirty,staged_count=excluded.staged_count,unstaged_count=excluded.unstaged_count,untracked_count=excluded.untracked_count,conflicted_count=excluded.conflicted_count,refresh_error_summary=excluded.refresh_error_summary",params![s.id,s.repository_id,s.captured_at.to_rfc3339(),s.head_oid,s.branch,s.upstream,s.ahead,s.behind,s.dirty,s.staged_count,s.unstaged_count,s.untracked_count,s.conflicted_count,s.refresh_error_summary]).map(|_|()))
    }
    pub fn get(&self, id: &str) -> Result<RepositorySnapshot, AppError> {
        self.db.with_connection(|c|c.query_row("SELECT id,repository_id,captured_at,head_oid,branch,upstream,ahead,behind,dirty,staged_count,unstaged_count,untracked_count,conflicted_count,refresh_error_summary FROM repository_snapshots WHERE id=?1",[id],map_snapshot).optional()).and_then(|x|x.ok_or_else(||AppError::NotFound(format!("snapshot {id}"))))
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
impl TrustRepository {
    pub fn new(db: Database) -> Self {
        Self { db }
    }
    pub fn set(&self, trust: &Trust) -> Result<(), AppError> {
        self.db.with_connection(|c| c.execute("INSERT INTO trusted_repositories(repository_id,trusted_at,trust_version) VALUES(?1,?2,?3) ON CONFLICT(repository_id) DO UPDATE SET trusted_at=excluded.trusted_at,trust_version=excluded.trust_version", params![trust.repository_id, trust.trusted_at.to_rfc3339(), trust.trust_version]).map(|_| ()))
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
