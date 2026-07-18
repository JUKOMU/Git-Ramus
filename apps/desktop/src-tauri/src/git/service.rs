//! High-level Git client orchestration.
//!
//! The service deliberately owns all path validation and Git command construction.  Callers
//! provide database identifiers and (for writes) a set of paths that must already be present in
//! the latest status snapshot; no generic command execution API is exposed.

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::ffi::OsString;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex};
use std::thread;
use std::time::Duration;

use chrono::Utc;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use super::engine::{GitCommand, GitOutput, GitRunner, SystemGitRunner};
use super::model::{Project, Repository, RepositorySnapshot, Trust, Workspace};
use super::parser::{
    ChangeEntry, ChangeKind, DetectedRepository, DiffFile, DiffSummary,
    RepositorySnapshot as ParsedSnapshot, detect_repository, parse_diff_summary, parse_status_v2,
};
use super::repository::{
    ProjectRepository, RepositoryRepository, SnapshotRepository, TrustRepository,
    WorkspaceRepository,
};
use crate::db::Database;
use crate::error::AppError;

const DEFAULT_SCAN_DEPTH: i64 = 3;
const DEFAULT_READ_CONCURRENCY: usize = 4;
const DEFAULT_GIT_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_SCAN_ENTRIES: usize = 100_000;
const MAX_PATHS_PER_OPERATION: usize = 4_096;
const MAX_COMMIT_MESSAGE_BYTES: usize = 128 * 1024;
// Keep worst-case Windows CreateProcess command lines comfortably below 32 KiB after four
// command-scope overrides per driver.
const MAX_FILTER_DRIVERS: usize = 32;
const MAX_FILTER_DRIVER_BYTES: usize = 64;
const DEFAULT_EXCLUDES: &[&str] = &[
    ".git",
    "node_modules",
    "target",
    "dist",
    "build",
    ".next",
    ".cache",
    ".venv",
    "vendor",
];

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectCreateInput {
    pub root_path: String,
    pub name: String,
    pub scan_depth: Option<i64>,
    #[serde(default)]
    pub exclude_patterns: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProjectUpdateInput {
    pub project_id: String,
    pub root_path: Option<String>,
    pub name: Option<String>,
    pub scan_depth: Option<i64>,
    pub exclude_patterns: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceCreateInput {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkspaceMembershipInput {
    pub workspace_id: String,
    pub project_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct QueryContext {
    pub project_id: Option<String>,
    pub workspace_id: Option<String>,
}

impl QueryContext {
    pub fn project(project_id: impl Into<String>) -> Self {
        Self {
            project_id: Some(project_id.into()),
            workspace_id: None,
        }
    }

    pub fn workspace(workspace_id: impl Into<String>) -> Self {
        Self {
            project_id: None,
            workspace_id: Some(workspace_id.into()),
        }
    }

    fn validate(&self) -> Result<(), AppError> {
        if self.project_id.is_some() == self.workspace_id.is_some() {
            return Err(AppError::InvalidInput(
                "exactly one projectId or workspaceId is required".to_owned(),
            ));
        }
        Ok(())
    }

    pub fn validate_for_command(&self) -> Result<(), AppError> {
        self.validate()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RepositoryScanRecord {
    pub repository: Repository,
    pub snapshot: Option<RepositorySnapshot>,
    pub changes: Option<ParsedSnapshot>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RepositoryScanFailure {
    pub path: String,
    pub error: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScanProgressRecord {
    pub index: usize,
    pub total: usize,
    pub repository_id: String,
    pub completed: bool,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ScanProjectResult {
    pub project_id: String,
    pub repositories: Vec<RepositoryScanRecord>,
    pub failures: Vec<RepositoryScanFailure>,
    pub total: usize,
    pub completed: usize,
    pub failed: usize,
    pub discovery_failed: usize,
    pub progress: Vec<ScanProgressRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OverviewRepository {
    pub repository: Repository,
    pub snapshot: Option<RepositorySnapshot>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Overview {
    pub context: QueryContext,
    pub repositories: Vec<OverviewRepository>,
    pub repository_count: usize,
    pub dirty_count: usize,
    pub staged_count: i64,
    pub unstaged_count: i64,
    pub untracked_count: i64,
    pub conflicted_count: i64,
    pub branches: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ChangesResult {
    pub repository_id: String,
    pub snapshot: RepositorySnapshot,
    pub changes: Vec<ChangeEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct DiffResult {
    pub repository_id: String,
    pub staged: bool,
    pub summary: DiffSummary,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WriteResult {
    pub repository_id: String,
    pub snapshot: Option<RepositorySnapshot>,
    pub output: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ContextKind {
    Project,
    Workspace,
}

#[derive(Debug)]
struct ReadGate {
    state: Mutex<usize>,
    wake: Condvar,
    limit: usize,
}

struct ReadPermit<'a> {
    gate: &'a ReadGate,
}

impl ReadGate {
    fn new(limit: usize) -> Self {
        Self {
            state: Mutex::new(0),
            wake: Condvar::new(),
            limit: limit.max(1),
        }
    }

    fn acquire(&self) -> ReadPermit<'_> {
        let mut active = self.state.lock().expect("read gate mutex is not poisoned");
        while *active >= self.limit {
            active = self
                .wake
                .wait(active)
                .expect("read gate mutex is not poisoned");
        }
        *active += 1;
        ReadPermit { gate: self }
    }

    fn limit(&self) -> usize {
        self.limit
    }
}

impl Drop for ReadPermit<'_> {
    fn drop(&mut self) {
        if let Ok(mut active) = self.gate.state.lock() {
            *active = active.saturating_sub(1);
            self.gate.wake.notify_one();
        }
    }
}

#[derive(Clone)]
pub struct GitService {
    db: Database,
    runner: Arc<dyn GitRunner>,
    projects: ProjectRepository,
    workspaces: WorkspaceRepository,
    repositories: RepositoryRepository,
    snapshots: SnapshotRepository,
    trusts: TrustRepository,
    read_gate: Arc<ReadGate>,
    write_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    project_locks: Arc<Mutex<HashMap<String, Arc<Mutex<()>>>>>,
    status_cache: Arc<Mutex<HashMap<String, ParsedSnapshot>>>,
}

impl GitService {
    pub fn new(db: Database) -> Self {
        Self::with_runner(db, Arc::new(SystemGitRunner::default()))
    }

    pub fn with_runner(db: Database, runner: Arc<dyn GitRunner>) -> Self {
        Self::with_runner_and_concurrency(db, runner, DEFAULT_READ_CONCURRENCY)
    }

    pub fn with_runner_and_concurrency(
        db: Database,
        runner: Arc<dyn GitRunner>,
        read_concurrency: usize,
    ) -> Self {
        Self {
            projects: ProjectRepository::new(db.clone()),
            workspaces: WorkspaceRepository::new(db.clone()),
            repositories: RepositoryRepository::new(db.clone()),
            snapshots: SnapshotRepository::new(db.clone()),
            trusts: TrustRepository::new(db.clone()),
            db,
            runner,
            read_gate: Arc::new(ReadGate::new(read_concurrency)),
            write_locks: Arc::new(Mutex::new(HashMap::new())),
            project_locks: Arc::new(Mutex::new(HashMap::new())),
            status_cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn database(&self) -> Database {
        self.db.clone()
    }

    pub fn project_repository(&self) -> ProjectRepository {
        self.projects.clone()
    }

    pub fn repository_repository(&self) -> RepositoryRepository {
        self.repositories.clone()
    }

    pub fn create_project(&self, input: ProjectCreateInput) -> Result<Project, AppError> {
        let root = canonical_directory(&input.root_path)?;
        let name = validate_name(&input.name, "project name")?;
        let scan_depth = validate_scan_depth(input.scan_depth.unwrap_or(DEFAULT_SCAN_DEPTH))?;
        let exclude_patterns = validate_excludes(input.exclude_patterns)?;
        let mut project = Project::new(&root, &name);
        project.scan_depth = scan_depth;
        project.exclude_patterns = exclude_patterns;
        self.projects.create(&project)?;
        Ok(project)
    }

    pub fn update_project(&self, input: ProjectUpdateInput) -> Result<Project, AppError> {
        let project_lock = self.project_lock(&input.project_id);
        let _project_guard = project_lock.lock().expect("project lock is not poisoned");
        let mut project = self.projects.get(&input.project_id)?;
        let original_root = project.root_path.clone();
        if let Some(path) = input.root_path {
            project.root_path = canonical_directory(&path)?;
        }
        if let Some(name) = input.name {
            project.name = validate_name(&name, "project name")?;
        }
        if let Some(depth) = input.scan_depth {
            project.scan_depth = validate_scan_depth(depth)?;
        }
        if let Some(excludes) = input.exclude_patterns {
            project.exclude_patterns = validate_excludes(excludes)?;
        }
        project.updated_at = Utc::now();
        self.projects
            .update_with_root_change(&project, project.root_path != original_root)?;
        Ok(project)
    }

    pub fn update_scan_rules(
        &self,
        project_id: &str,
        scan_depth: Option<i64>,
        exclude_patterns: Option<Vec<String>>,
    ) -> Result<Project, AppError> {
        self.update_project(ProjectUpdateInput {
            project_id: project_id.to_owned(),
            root_path: None,
            name: None,
            scan_depth,
            exclude_patterns,
        })
    }

    pub fn delete_project_by_id(&self, project_id: &str) -> Result<(), AppError> {
        let project_lock = self.project_lock(project_id);
        let _project_guard = project_lock.lock().expect("project lock is not poisoned");
        self.projects.delete(project_id)
    }

    pub fn list_projects(&self) -> Result<Vec<Project>, AppError> {
        self.projects.list()
    }

    pub fn get_project(&self, project_id: &str) -> Result<Project, AppError> {
        self.projects.get(project_id)
    }

    pub fn delete_project(&self, project_id: &str) -> Result<(), AppError> {
        let project_lock = self.project_lock(project_id);
        let _project_guard = project_lock.lock().expect("project lock is not poisoned");
        self.projects.delete(project_id)
    }

    pub fn create_workspace(&self, input: WorkspaceCreateInput) -> Result<Workspace, AppError> {
        let name = validate_name(&input.name, "workspace name")?;
        let workspace = Workspace::new(&name);
        self.workspaces.create(&workspace)?;
        Ok(workspace)
    }

    pub fn update_workspace(&self, workspace_id: &str, name: &str) -> Result<Workspace, AppError> {
        let mut workspace = self.workspaces.get(workspace_id)?;
        workspace.name = validate_name(name, "workspace name")?;
        workspace.updated_at = Utc::now();
        self.workspaces.update(&workspace)?;
        Ok(workspace)
    }

    pub fn list_workspaces(&self) -> Result<Vec<Workspace>, AppError> {
        self.workspaces.list()
    }

    pub fn get_workspace(&self, workspace_id: &str) -> Result<Workspace, AppError> {
        self.workspaces.get(workspace_id)
    }

    pub fn update_workspace_membership(
        &self,
        input: WorkspaceMembershipInput,
    ) -> Result<Vec<String>, AppError> {
        self.workspaces.get(&input.workspace_id)?;
        let mut unique = HashSet::new();
        for project_id in &input.project_ids {
            if !unique.insert(project_id) {
                return Err(AppError::InvalidInput(
                    "workspace project IDs must be unique".to_owned(),
                ));
            }
            self.projects.get(project_id)?;
        }
        self.workspaces
            .set_projects(&input.workspace_id, &input.project_ids)?;
        self.workspaces.projects(&input.workspace_id)
    }

    pub fn add_workspace_project(
        &self,
        workspace_id: &str,
        project_id: &str,
    ) -> Result<(), AppError> {
        self.workspaces.get(workspace_id)?;
        self.projects.get(project_id)?;
        self.workspaces.add_project(workspace_id, project_id)
    }

    pub fn remove_workspace_project(
        &self,
        workspace_id: &str,
        project_id: &str,
    ) -> Result<(), AppError> {
        self.workspaces.remove_project(workspace_id, project_id)
    }

    pub fn workspace_projects(&self, workspace_id: &str) -> Result<Vec<String>, AppError> {
        self.workspaces.projects(workspace_id)
    }

    pub fn delete_workspace(&self, workspace_id: &str) -> Result<(), AppError> {
        self.workspaces.delete(workspace_id)
    }

    /// Scan a project and refresh every discovered repository.  Discovery is deliberately
    /// best-effort: malformed/unreadable candidate directories are recorded in `failures` while
    /// other repositories continue to refresh.
    pub fn scan_project(&self, project_id: &str) -> Result<ScanProjectResult, AppError> {
        self.scan_project_with_progress(project_id, |_| {})
    }

    /// Scan with a completion callback. The callback runs as each bounded worker completes a
    /// repository, allowing a Tauri adapter to emit progressive records while retaining the
    /// synchronous `scan_project` API for callers that only need the final aggregate.
    pub fn scan_project_with_progress<F>(
        &self,
        project_id: &str,
        progress: F,
    ) -> Result<ScanProjectResult, AppError>
    where
        F: Fn(&RepositoryScanRecord) + Send + Sync + 'static,
    {
        self.scan_project_with_progress_limit(project_id, progress, MAX_SCAN_ENTRIES)
    }

    fn scan_project_with_progress_limit<F>(
        &self,
        project_id: &str,
        progress: F,
        entry_limit: usize,
    ) -> Result<ScanProjectResult, AppError>
    where
        F: Fn(&RepositoryScanRecord) + Send + Sync + 'static,
    {
        let project_lock = self.project_lock(project_id);
        let _project_guard = project_lock.lock().expect("project lock is not poisoned");
        let project = self.projects.get(project_id)?;
        let mut candidates = Vec::<(DetectedRepository, String)>::new();
        let mut failures = Vec::new();
        let mut seen = HashSet::<PathBuf>::new();
        let mut visited = HashSet::<PathBuf>::new();
        let mut stack = vec![(PathBuf::from(&project.root_path), 0_i64)];
        let mut entries_seen = 0_usize;

        while let Some((directory, depth)) = stack.pop() {
            if entries_seen >= entry_limit {
                failures.push(RepositoryScanFailure {
                    path: directory.to_string_lossy().into_owned(),
                    error: "scan entry limit exceeded".to_owned(),
                });
                break;
            }
            entries_seen += 1;
            let canonical = match fs::canonicalize(&directory) {
                Ok(path) => path,
                Err(error) => {
                    failures.push(RepositoryScanFailure {
                        path: directory.to_string_lossy().into_owned(),
                        error: sanitize_fs_error(error),
                    });
                    continue;
                }
            };
            if !visited.insert(canonical.clone()) {
                continue;
            }

            // Detection intentionally happens before reading children. `detect_repository`
            // recognizes marker layout; the later bounded `git status` process provides the
            // actual Git verification and captures a per-repository error instead of aborting.
            // It also walks upward, so a Project inside an outer repository must be rejected as
            // an out-of-scope candidate rather than linked to the outer root.
            let mut repository_detected_in_scope = false;
            match detect_repository(&canonical) {
                Ok(detected) => match relative_path(&project.root_path, &detected.canonical_path) {
                    Ok(relative) => {
                        repository_detected_in_scope = true;
                        if seen.insert(detected.canonical_path.clone()) {
                            candidates.push((detected, relative));
                        }
                    }
                    Err(error) if seen.insert(detected.canonical_path.clone()) => {
                        failures.push(RepositoryScanFailure {
                            path: canonical.to_string_lossy().into_owned(),
                            error: stable_error(&error),
                        });
                    }
                    Err(_) => {}
                },
                Err(error) if !is_candidate_error(&error) => {
                    failures.push(RepositoryScanFailure {
                        path: canonical.to_string_lossy().into_owned(),
                        error: stable_error(&error),
                    });
                }
                Err(_) => {}
            }
            if repository_detected_in_scope {
                continue;
            }

            if depth >= project.scan_depth {
                continue;
            }
            let read_dir = match fs::read_dir(&canonical) {
                Ok(read_dir) => read_dir,
                Err(error) => {
                    failures.push(RepositoryScanFailure {
                        path: canonical.to_string_lossy().into_owned(),
                        error: sanitize_fs_error(error),
                    });
                    continue;
                }
            };
            let mut children = Vec::new();
            for entry in read_dir {
                let entry = match entry {
                    Ok(entry) => entry,
                    Err(error) => {
                        failures.push(RepositoryScanFailure {
                            path: canonical.to_string_lossy().into_owned(),
                            error: sanitize_fs_error(error),
                        });
                        continue;
                    }
                };
                let child = entry.path();
                let file_type = match entry.file_type() {
                    Ok(file_type) => file_type,
                    Err(error) => {
                        failures.push(RepositoryScanFailure {
                            path: child.to_string_lossy().into_owned(),
                            error: sanitize_fs_error(error),
                        });
                        continue;
                    }
                };
                if file_type.is_symlink() || !file_type.is_dir() {
                    continue;
                }
                let Some(name) = child.file_name().and_then(|name| name.to_str()) else {
                    failures.push(RepositoryScanFailure {
                        path: child.to_string_lossy().into_owned(),
                        error: "path is not valid UTF-8".to_owned(),
                    });
                    continue;
                };
                let rel = relative_path(&project.root_path, &child)?;
                if is_excluded(name, &rel, &project.exclude_patterns) {
                    continue;
                }
                children.push(child);
            }
            // Stable ordering makes progressive records deterministic and prevents scan result
            // churn when the filesystem returns directory entries in arbitrary order.
            children.sort_by(|left, right| left.as_os_str().cmp(right.as_os_str()));
            for child in children.into_iter().rev() {
                stack.push((child, depth + 1));
            }
        }

        let mut prepared = Vec::with_capacity(candidates.len());
        for (detected, relative) in candidates {
            let repository = match self.ensure_repository(&detected) {
                Ok(repository) => repository,
                Err(error) => {
                    failures.push(RepositoryScanFailure {
                        path: detected.canonical_path.to_string_lossy().into_owned(),
                        error: stable_error(&error),
                    });
                    continue;
                }
            };
            if let Err(error) =
                self.repositories
                    .add_to_project(project_id, &repository.id, &relative)
            {
                failures.push(RepositoryScanFailure {
                    path: detected.canonical_path.to_string_lossy().into_owned(),
                    error: stable_error(&error),
                });
                continue;
            }
            prepared.push((detected.canonical_path, repository));
        }
        let prepared_total = prepared.len();
        let discovery_failed = failures.len();

        // Relationship writes happen on this thread. Snapshot writes are serialized by the
        // Database connection mutex inside each worker; ReadGate bounds the actual Git processes.
        let queue = Arc::new(Mutex::new(
            prepared.into_iter().enumerate().collect::<VecDeque<_>>(),
        ));
        let records = Arc::new(Mutex::new(Vec::<(usize, RepositoryScanRecord)>::new()));
        let refresh_failures = Arc::new(Mutex::new(Vec::<RepositoryScanFailure>::new()));
        let callback: Arc<dyn Fn(&RepositoryScanRecord) + Send + Sync> = Arc::new(progress);
        let worker_count = self.read_gate.limit().min(prepared_total.max(1));
        thread::scope(|scope| {
            for _ in 0..worker_count {
                let queue = Arc::clone(&queue);
                let records = Arc::clone(&records);
                let refresh_failures = Arc::clone(&refresh_failures);
                let callback = Arc::clone(&callback);
                let service = self.clone();
                scope.spawn(move || {
                    loop {
                        let item = queue
                            .lock()
                            .expect("scan queue mutex is not poisoned")
                            .pop_front();
                        let Some((index, (path, repository))) = item else {
                            break;
                        };
                        let (record, failure) = match service.refresh_repository_inner(&repository)
                        {
                            Ok((snapshot, parsed)) => (
                                RepositoryScanRecord {
                                    repository: repository.clone(),
                                    snapshot: Some(snapshot),
                                    changes: Some(parsed),
                                    error: None,
                                },
                                None,
                            ),
                            Err(error) => {
                                let refresh_summary = stable_error(&error);
                                let (snapshot, summary) = match service
                                    .record_refresh_failure(&repository, &error)
                                {
                                    Ok(snapshot) => (snapshot, refresh_summary),
                                    Err(persistence_error) => (
                                        None,
                                        format!(
                                            "{refresh_summary}; snapshot persistence failed: {}",
                                            stable_error(&persistence_error)
                                        ),
                                    ),
                                };
                                (
                                    RepositoryScanRecord {
                                        repository: repository.clone(),
                                        snapshot,
                                        changes: service.cached_status(&repository.id),
                                        error: Some(summary.clone()),
                                    },
                                    Some(RepositoryScanFailure {
                                        path: path.to_string_lossy().into_owned(),
                                        error: summary,
                                    }),
                                )
                            }
                        };
                        callback(&record);
                        records
                            .lock()
                            .expect("scan records mutex is not poisoned")
                            .push((index, record));
                        if let Some(failure) = failure {
                            refresh_failures
                                .lock()
                                .expect("scan failure mutex is not poisoned")
                                .push(failure);
                        }
                    }
                });
            }
        });

        let mut indexed_records = Arc::try_unwrap(records)
            .unwrap_or_else(|_| panic!("scan records still referenced"))
            .into_inner()
            .expect("scan records mutex is not poisoned");
        indexed_records.sort_by_key(|(index, _)| *index);
        let records = indexed_records
            .into_iter()
            .map(|(_, record)| record)
            .collect::<Vec<_>>();
        failures.extend(
            Arc::try_unwrap(refresh_failures)
                .unwrap_or_else(|_| panic!("scan failures still referenced"))
                .into_inner()
                .expect("scan failure mutex is not poisoned"),
        );
        failures.sort_by(|left, right| left.path.cmp(&right.path));

        let total = records.len();
        let completed = records
            .iter()
            .filter(|record| record.error.is_none())
            .count();
        let failed = records
            .iter()
            .filter(|record| record.error.is_some())
            .count();
        debug_assert_eq!(total, completed + failed);
        let progress = records
            .iter()
            .enumerate()
            .map(|(index, record)| ScanProgressRecord {
                index,
                total,
                repository_id: record.repository.id.clone(),
                completed: record.error.is_none(),
                error: record.error.clone(),
            })
            .collect();
        Ok(ScanProjectResult {
            project_id: project_id.to_owned(),
            repositories: records,
            failures,
            total,
            completed,
            failed,
            discovery_failed,
            progress,
        })
    }

    pub fn refresh_repository(
        &self,
        project_id: &str,
        repository_id: &str,
    ) -> Result<RepositoryScanRecord, AppError> {
        self.ensure_project_membership(project_id, repository_id)?;
        let repository = self.repositories.get(repository_id)?;
        match self.refresh_repository_inner(&repository) {
            Ok((snapshot, parsed)) => Ok(RepositoryScanRecord {
                repository,
                snapshot: Some(snapshot),
                changes: Some(parsed),
                error: None,
            }),
            Err(error) => Ok(RepositoryScanRecord {
                repository: repository.clone(),
                snapshot: self.record_refresh_failure(&repository, &error)?,
                changes: self.cached_status(repository_id),
                error: Some(stable_error(&error)),
            }),
        }
    }

    pub fn get_snapshot(
        &self,
        context: &QueryContext,
        repository_id: &str,
    ) -> Result<RepositoryScanRecord, AppError> {
        self.ensure_context_membership(context, repository_id)?;
        let repository = self.repositories.get(repository_id)?;
        let snapshot = self.snapshots.latest_for_repository(repository_id)?;
        Ok(RepositoryScanRecord {
            repository,
            snapshot,
            changes: self.cached_status(repository_id),
            error: None,
        })
    }

    pub fn get_overview(&self, context: &QueryContext) -> Result<Overview, AppError> {
        let repositories = self.repositories_for_context(context)?;
        let mut overview = Overview {
            context: context.clone(),
            repository_count: repositories.len(),
            ..Overview::default()
        };
        overview.context = context.clone();
        for repository in repositories {
            let snapshot = self.snapshots.latest_for_repository(&repository.id)?;
            if let Some(snapshot) = &snapshot {
                if snapshot.dirty {
                    overview.dirty_count += 1;
                }
                overview.staged_count += snapshot.staged_count;
                overview.unstaged_count += snapshot.unstaged_count;
                overview.untracked_count += snapshot.untracked_count;
                overview.conflicted_count += snapshot.conflicted_count;
                if let Some(branch) = &snapshot.branch {
                    if !overview.branches.contains(branch) {
                        overview.branches.push(branch.clone());
                    }
                }
            }
            overview.repositories.push(OverviewRepository {
                repository,
                snapshot,
            });
        }
        Ok(overview)
    }

    pub fn get_overview_for_project(&self, project_id: &str) -> Result<Overview, AppError> {
        self.get_overview(&QueryContext::project(project_id))
    }

    pub fn get_overview_for_workspace(&self, workspace_id: &str) -> Result<Overview, AppError> {
        self.get_overview(&QueryContext::workspace(workspace_id))
    }

    pub fn get_changes(
        &self,
        context: &QueryContext,
        repository_id: &str,
    ) -> Result<ChangesResult, AppError> {
        self.ensure_context_membership(context, repository_id)?;
        let repository = self.repositories.get(repository_id)?;
        let (snapshot, parsed) = self.refresh_repository_recording_failure(&repository)?;
        Ok(ChangesResult {
            repository_id: repository_id.to_owned(),
            snapshot,
            changes: parsed.changes,
        })
    }

    pub fn get_diff(
        &self,
        context: &QueryContext,
        repository_id: &str,
        paths: &[String],
        staged: bool,
    ) -> Result<DiffResult, AppError> {
        self.ensure_context_membership(context, repository_id)?;
        let repository = self.repositories.get(repository_id)?;
        let parsed = self.latest_or_refresh_status(&repository)?;
        let validated = validate_change_paths(paths, &parsed.changes)?;
        if !self.trusts.is_trusted(repository_id)? {
            // Git's worktree diff conversion executes filter.<driver>.clean/process even with
            // --no-textconv. Until the user trusts this repository, return the bounded status-
            // derived summary and never start a repo-context `git diff` process. Content-derived
            // binary detection and line counts remain unknown because computing them would
            // require reading and converting file content.
            return Ok(DiffResult {
                repository_id: repository_id.to_owned(),
                staged,
                summary: safe_diff_summary(&parsed.changes, &validated, staged),
            });
        }
        let mut args = vec![
            OsString::from("--no-optional-locks"),
            OsString::from("-c"),
            OsString::from("core.fsmonitor=false"),
            OsString::from("diff"),
        ];
        if staged {
            args.push(OsString::from("--cached"));
        }
        args.extend([
            OsString::from("--no-ext-diff"),
            OsString::from("--no-textconv"),
            OsString::from("--binary"),
        ]);
        args.push(OsString::from("--"));
        args.extend(validated.iter().map(OsString::from));
        // Status and diff use separate permits, but each actual read-only Git process remains
        // under the same global gate. This prevents concurrent diff calls from escaping the scan
        // read bound after their status refresh has completed.
        let _permit = self.read_gate.acquire();
        let output = self.run_git(&repository, args, None)?;
        ensure_success(&output)?;
        Ok(DiffResult {
            repository_id: repository_id.to_owned(),
            staged,
            summary: parse_diff_summary(output.stdout)?,
        })
    }

    pub fn trust_repository(&self, repository_id: &str) -> Result<Trust, AppError> {
        self.repositories.get(repository_id)?;
        let trust = Trust {
            repository_id: repository_id.to_owned(),
            trusted_at: Utc::now(),
            trust_version: 1,
        };
        self.trusts.set(&trust)?;
        Ok(trust)
    }

    pub fn is_repository_trusted(&self, repository_id: &str) -> Result<bool, AppError> {
        self.repositories.get(repository_id)?;
        self.trusts.is_trusted(repository_id)
    }

    pub fn trust_repository_in_context(
        &self,
        context: &QueryContext,
        repository_id: &str,
    ) -> Result<Trust, AppError> {
        self.ensure_context_membership(context, repository_id)?;
        self.trust_repository(repository_id)
    }

    pub fn stage(
        &self,
        context: &QueryContext,
        repository_id: &str,
        paths: &[String],
        all: bool,
    ) -> Result<WriteResult, AppError> {
        self.write_operation(context, repository_id, paths, all, WriteKind::Stage)
    }

    pub fn unstage(
        &self,
        context: &QueryContext,
        repository_id: &str,
        paths: &[String],
    ) -> Result<WriteResult, AppError> {
        self.write_operation(context, repository_id, paths, false, WriteKind::Unstage)
    }

    pub fn commit(
        &self,
        context: &QueryContext,
        repository_id: &str,
        message: &str,
    ) -> Result<WriteResult, AppError> {
        self.ensure_context_membership(context, repository_id)?;
        if message.trim().is_empty() {
            return Err(AppError::InvalidInput(
                "commit message must not be empty".to_owned(),
            ));
        }
        if message.len() > MAX_COMMIT_MESSAGE_BYTES {
            return Err(AppError::InvalidInput(
                "commit message is too large".to_owned(),
            ));
        }
        let repository = self.repositories.get(repository_id)?;
        self.require_trust(repository_id)?;
        let parsed = self.latest_or_refresh_status(&repository)?;
        if parsed.staged_count == 0 {
            return Err(AppError::InvalidInput(
                "commit requires staged changes".to_owned(),
            ));
        }
        let lock = self.write_lock(repository_id);
        let _guard = lock.lock().expect("repository write lock is not poisoned");
        let mut bytes = message.as_bytes().to_vec();
        if !bytes.ends_with(b"\n") {
            bytes.push(b'\n');
        }
        let result = self.run_git(
            &repository,
            vec![
                OsString::from("commit"),
                OsString::from("-F"),
                OsString::from("-"),
            ],
            Some(bytes),
        );
        let refresh = self.refresh_after_write(&repository);
        match result {
            Ok(output) => {
                let status_result = ensure_success(&output);
                let snapshot_result = refresh;
                status_result?;
                Ok(WriteResult {
                    repository_id: repository_id.to_owned(),
                    snapshot: snapshot_result?,
                    output: Some(sanitize_text(&output.stdout)),
                })
            }
            Err(error) => {
                let _ = refresh;
                Err(error)
            }
        }
    }

    pub fn stage_for_project(
        &self,
        project_id: &str,
        repository_id: &str,
        paths: &[String],
        all: bool,
    ) -> Result<WriteResult, AppError> {
        self.stage(
            &QueryContext::project(project_id),
            repository_id,
            paths,
            all,
        )
    }

    pub fn unstage_for_project(
        &self,
        project_id: &str,
        repository_id: &str,
        paths: &[String],
    ) -> Result<WriteResult, AppError> {
        self.unstage(&QueryContext::project(project_id), repository_id, paths)
    }

    pub fn commit_for_project(
        &self,
        project_id: &str,
        repository_id: &str,
        message: &str,
    ) -> Result<WriteResult, AppError> {
        self.commit(&QueryContext::project(project_id), repository_id, message)
    }

    fn write_operation(
        &self,
        context: &QueryContext,
        repository_id: &str,
        paths: &[String],
        all: bool,
        kind: WriteKind,
    ) -> Result<WriteResult, AppError> {
        self.ensure_context_membership(context, repository_id)?;
        let repository = self.repositories.get(repository_id)?;
        self.require_trust(repository_id)?;
        let parsed = self.latest_or_refresh_status(&repository)?;
        let validated = if all {
            if !paths.is_empty() {
                return Err(AppError::InvalidInput(
                    "paths must be empty when all is true".to_owned(),
                ));
            }
            Vec::new()
        } else {
            validate_change_paths(paths, &parsed.changes)?
        };
        if !all && validated.is_empty() {
            return Err(AppError::InvalidInput(
                "at least one path is required".to_owned(),
            ));
        }
        let lock = self.write_lock(repository_id);
        let _guard = lock.lock().expect("repository write lock is not poisoned");
        let args = match kind {
            WriteKind::Stage if all => vec![
                OsString::from("add"),
                OsString::from("-A"),
                OsString::from("--"),
                OsString::from("."),
            ],
            WriteKind::Stage => {
                let mut args = vec![OsString::from("add"), OsString::from("--")];
                args.extend(validated.iter().map(OsString::from));
                args
            }
            WriteKind::Unstage => {
                let mut args = vec![
                    OsString::from("restore"),
                    OsString::from("--staged"),
                    OsString::from("--"),
                ];
                args.extend(validated.iter().map(OsString::from));
                args
            }
        };
        let result = self.run_git(&repository, args, None);
        let refresh = self.refresh_after_write(&repository);
        match result {
            Ok(output) => {
                let status_result = ensure_success(&output);
                let snapshot_result = refresh;
                status_result?;
                Ok(WriteResult {
                    repository_id: repository_id.to_owned(),
                    snapshot: snapshot_result?,
                    output: None,
                })
            }
            Err(error) => {
                let _ = refresh;
                Err(error)
            }
        }
    }

    fn require_trust(&self, repository_id: &str) -> Result<(), AppError> {
        if self.trusts.is_trusted(repository_id)? {
            Ok(())
        } else {
            Err(AppError::TrustRequired)
        }
    }

    fn ensure_repository(&self, detected: &DetectedRepository) -> Result<Repository, AppError> {
        let canonical = path_to_utf8(&detected.canonical_path)?;
        let display_name = detected
            .canonical_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or(AppError::NonUtf8Path)?;
        let repository = Repository::new(&canonical, display_name, detected.kind.clone());
        self.repositories.get_or_create(&repository)
    }

    fn refresh_repository_inner(
        &self,
        repository: &Repository,
    ) -> Result<(RepositorySnapshot, ParsedSnapshot), AppError> {
        let parsed = self.read_status(repository)?;
        let snapshot = db_snapshot(repository.id.as_str(), &parsed, None);
        self.snapshots.upsert(&snapshot)?;
        if let Ok(mut cache) = self.status_cache.lock() {
            cache.insert(repository.id.clone(), parsed.clone());
        }
        Ok((snapshot, parsed))
    }

    fn refresh_repository_recording_failure(
        &self,
        repository: &Repository,
    ) -> Result<(RepositorySnapshot, ParsedSnapshot), AppError> {
        match self.refresh_repository_inner(repository) {
            Ok(result) => Ok(result),
            Err(refresh_error) => {
                // Persist the redacted failure while preserving the last successful fields. If
                // persistence itself fails, return that storage error rather than hiding it
                // behind the original Git error.
                self.record_refresh_failure(repository, &refresh_error)?;
                Err(refresh_error)
            }
        }
    }

    fn refresh_after_write(
        &self,
        repository: &Repository,
    ) -> Result<Option<RepositorySnapshot>, AppError> {
        match self.refresh_repository_inner(repository) {
            Ok((snapshot, _)) => Ok(Some(snapshot)),
            Err(error) => {
                let _ = self.record_refresh_failure(repository, &error)?;
                Err(error)
            }
        }
    }

    fn record_refresh_failure(
        &self,
        repository: &Repository,
        error: &AppError,
    ) -> Result<Option<RepositorySnapshot>, AppError> {
        // Preserve every successful field when one exists. For a first refresh, persist an
        // explicit error-only snapshot so callers never have to infer failure from a missing row.
        let snapshot = if let Some(previous) = self
            .snapshots
            .latest_successful_for_repository(&repository.id)?
        {
            RepositorySnapshot {
                id: Uuid::new_v4().to_string(),
                refresh_error_summary: Some(stable_error(error)),
                ..previous
            }
        } else {
            let mut initial = RepositorySnapshot::new(&repository.id);
            initial.refresh_error_summary = Some(stable_error(error));
            initial
        };
        self.snapshots.upsert(&snapshot)?;
        Ok(Some(snapshot))
    }

    fn read_status(&self, repository: &Repository) -> Result<ParsedSnapshot, AppError> {
        let trusted = self.trusts.is_trusted(&repository.id)?;
        let _permit = self.read_gate.acquire();
        let mut args = vec![
            OsString::from("--no-optional-locks"),
            OsString::from("-c"),
            OsString::from("core.fsmonitor=false"),
        ];
        if !trusted {
            let filter_drivers = self.read_filter_driver_names(repository)?;
            append_disabled_filter_overrides(&mut args, &filter_drivers);
        }
        args.extend([
            OsString::from("status"),
            OsString::from("--porcelain=v2"),
            OsString::from("-z"),
            OsString::from("--branch"),
            OsString::from("--untracked-files=all"),
        ]);
        if !trusted {
            args.push(OsString::from("--ignore-submodules=all"));
        }
        let output = self.run_git(repository, args, None)?;
        ensure_success(&output)?;
        parse_status_v2(output.stdout)
    }

    fn read_filter_driver_names(&self, repository: &Repository) -> Result<Vec<String>, AppError> {
        // This query returns names only: it never exposes command values or secrets and Git's
        // config reader does not execute hooks, filters, aliases, or a pager. `--includes`
        // intentionally covers effective global/local/worktree include files. Command-scope
        // overrides are applied immediately to status and outrank those sources.
        //
        // A same-user process racing config/.gitattributes changes between these two processes is
        // outside the static repository trust boundary; GitService itself does not mutate filter
        // config on this path. Any malformed/oversized query fails closed before status starts.
        let output = self.run_git(
            repository,
            vec![
                OsString::from("--no-pager"),
                OsString::from("--no-optional-locks"),
                OsString::from("-c"),
                OsString::from("core.fsmonitor=false"),
                OsString::from("config"),
                OsString::from("--null"),
                OsString::from("--name-only"),
                OsString::from("--includes"),
                OsString::from("--get-regexp"),
                OsString::from("^filter\\..*\\.(clean|smudge|process|required)$"),
            ],
            None,
        )?;
        if !output.status.success() {
            if output.status.code() == Some(1)
                && output.stdout.is_empty()
                && output.stderr.is_empty()
            {
                return Ok(Vec::new());
            }
            ensure_success(&output)?;
        }
        parse_filter_driver_names(&output.stdout)
    }

    fn latest_or_refresh_status(
        &self,
        repository: &Repository,
    ) -> Result<ParsedSnapshot, AppError> {
        let (_, parsed) = self.refresh_repository_recording_failure(repository)?;
        Ok(parsed)
    }

    fn cached_status(&self, repository_id: &str) -> Option<ParsedSnapshot> {
        self.status_cache
            .lock()
            .ok()
            .and_then(|cache| cache.get(repository_id).cloned())
    }

    fn run_git(
        &self,
        repository: &Repository,
        args: Vec<OsString>,
        stdin: Option<Vec<u8>>,
    ) -> Result<GitOutput, AppError> {
        self.runner.run(GitCommand {
            repo: PathBuf::from(&repository.canonical_path),
            args,
            stdin,
            timeout: DEFAULT_GIT_TIMEOUT,
        })
    }

    fn write_lock(&self, repository_id: &str) -> Arc<Mutex<()>> {
        let mut locks = self
            .write_locks
            .lock()
            .expect("write lock map is not poisoned");
        locks
            .entry(repository_id.to_owned())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    fn project_lock(&self, project_id: &str) -> Arc<Mutex<()>> {
        let mut locks = self
            .project_locks
            .lock()
            .expect("project lock map is not poisoned");
        locks
            .entry(project_id.to_owned())
            .or_insert_with(|| Arc::new(Mutex::new(())))
            .clone()
    }

    fn ensure_project_membership(
        &self,
        project_id: &str,
        repository_id: &str,
    ) -> Result<(), AppError> {
        self.projects.get(project_id)?;
        if self.repositories.is_in_project(project_id, repository_id)? {
            Ok(())
        } else {
            Err(AppError::NotFound(format!(
                "repository {repository_id} in project {project_id}"
            )))
        }
    }

    fn ensure_context_membership(
        &self,
        context: &QueryContext,
        repository_id: &str,
    ) -> Result<ContextKind, AppError> {
        context.validate()?;
        if let Some(project_id) = &context.project_id {
            self.ensure_project_membership(project_id, repository_id)?;
            return Ok(ContextKind::Project);
        }
        let workspace_id = context.workspace_id.as_deref().expect("validated context");
        self.workspaces.get(workspace_id)?;
        if self
            .repositories
            .is_in_workspace(workspace_id, repository_id)?
        {
            Ok(ContextKind::Workspace)
        } else {
            Err(AppError::NotFound(format!(
                "repository {repository_id} in workspace {workspace_id}"
            )))
        }
    }

    fn repositories_for_context(
        &self,
        context: &QueryContext,
    ) -> Result<Vec<Repository>, AppError> {
        context.validate()?;
        if let Some(project_id) = &context.project_id {
            self.projects.get(project_id)?;
            self.repositories.list_for_project(project_id)
        } else {
            let workspace_id = context.workspace_id.as_deref().expect("validated context");
            self.workspaces.get(workspace_id)?;
            self.repositories.list_for_workspace(workspace_id)
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum WriteKind {
    Stage,
    Unstage,
}

fn canonical_directory(input: &str) -> Result<String, AppError> {
    if input.trim().is_empty() {
        return Err(AppError::InvalidInput(
            "project root path is empty".to_owned(),
        ));
    }
    let path = Path::new(input);
    let metadata = fs::metadata(path).map_err(|error| match error.kind() {
        std::io::ErrorKind::NotFound => AppError::NotFound("project root directory".to_owned()),
        std::io::ErrorKind::PermissionDenied => AppError::PermissionDenied,
        _ => AppError::Io(error),
    })?;
    if !metadata.is_dir() {
        return Err(AppError::InvalidInput(
            "project root path must be a directory".to_owned(),
        ));
    }
    let canonical = fs::canonicalize(path)?;
    path_to_utf8(&canonical)
}

fn path_to_utf8(path: &Path) -> Result<String, AppError> {
    path.to_str()
        .map(str::to_owned)
        .ok_or(AppError::NonUtf8Path)
}

fn validate_name(value: &str, label: &str) -> Result<String, AppError> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 256 {
        return Err(AppError::InvalidInput(format!(
            "{label} must contain 1 to 256 characters"
        )));
    }
    Ok(value.to_owned())
}

fn validate_scan_depth(depth: i64) -> Result<i64, AppError> {
    if !(0..=64).contains(&depth) {
        return Err(AppError::InvalidInput(
            "scan depth must be between 0 and 64".to_owned(),
        ));
    }
    Ok(depth)
}

fn validate_excludes(patterns: Vec<String>) -> Result<Vec<String>, AppError> {
    if patterns.len() > 256 {
        return Err(AppError::InvalidInput(
            "exclude pattern count exceeds the limit".to_owned(),
        ));
    }
    let mut result = Vec::with_capacity(patterns.len());
    for pattern in patterns {
        let pattern = pattern.trim();
        if pattern.is_empty() || pattern.len() > 512 || pattern.contains('\0') {
            return Err(AppError::InvalidInput(
                "exclude pattern is invalid".to_owned(),
            ));
        }
        result.push(pattern.to_owned());
    }
    Ok(result)
}

fn relative_path(root: &str, path: &Path) -> Result<String, AppError> {
    let root = Path::new(root);
    let relative = path
        .strip_prefix(root)
        .map_err(|_| AppError::InvalidInput("repository path escapes project root".to_owned()))?;
    if relative.as_os_str().is_empty() {
        return Ok(".".to_owned());
    }
    let mut value = String::new();
    for component in relative.components() {
        let Component::Normal(part) = component else {
            return Err(AppError::InvalidInput(
                "invalid relative repository path".to_owned(),
            ));
        };
        let part = part.to_str().ok_or(AppError::NonUtf8Path)?;
        if !value.is_empty() {
            value.push('/');
        }
        value.push_str(part);
    }
    Ok(value)
}

fn is_excluded(name: &str, relative: &str, patterns: &[String]) -> bool {
    if DEFAULT_EXCLUDES.contains(&name) {
        return true;
    }
    patterns.iter().any(|pattern| {
        let normalized = pattern.replace('\\', "/");
        glob_match(&normalized, name) || glob_match(&normalized, relative)
    })
}

/// Small, deterministic glob matcher supporting the `*` and `?` forms used by scan rules.  It
/// intentionally does not interpret `**` specially; matching the complete relative path still
/// permits users to write `foo/*` without exposing a filesystem query language.
fn glob_match(pattern: &str, value: &str) -> bool {
    let pattern = pattern.as_bytes();
    let value = value.as_bytes();
    // `state[j]` means that the prefix processed so far matches `value[..j]`.  For `*`, every
    // reachable prefix up to the current position is also reachable after consuming the star;
    // carrying a `seen` flag avoids the subtle in-place propagation bug that is easy to introduce
    // with a two-dimensional table.
    let mut state = vec![false; value.len() + 1];
    state[0] = true;
    for &token in pattern {
        let mut next = vec![false; value.len() + 1];
        if token == b'*' {
            let mut seen = false;
            for index in 0..=value.len() {
                seen |= state[index];
                next[index] = seen;
            }
        } else {
            for index in 0..value.len() {
                if state[index] && (token == b'?' || token == value[index]) {
                    next[index + 1] = true;
                }
            }
        }
        state = next;
    }
    state[value.len()]
}

fn validate_change_paths(
    paths: &[String],
    changes: &[ChangeEntry],
) -> Result<Vec<String>, AppError> {
    if paths.len() > MAX_PATHS_PER_OPERATION {
        return Err(AppError::InvalidInput("too many paths".to_owned()));
    }
    let allowed = changes
        .iter()
        .flat_map(|change| [Some(change.path.as_str()), change.original_path.as_deref()])
        .flatten()
        .collect::<HashSet<_>>();
    let mut result = Vec::with_capacity(paths.len());
    let mut unique = HashSet::new();
    for input in paths {
        let normalized = validate_relative_path(input)?;
        if !allowed.contains(normalized.as_str()) {
            return Err(AppError::InvalidInput(format!(
                "path is not present in the latest change set: {normalized}"
            )));
        }
        if unique.insert(normalized.clone()) {
            result.push(normalized);
        }
    }
    Ok(result)
}

fn safe_diff_summary(
    changes: &[ChangeEntry],
    requested_paths: &[String],
    staged: bool,
) -> DiffSummary {
    let requested = requested_paths
        .iter()
        .map(String::as_str)
        .collect::<HashSet<_>>();
    let files = changes
        .iter()
        .filter(|change| {
            if staged {
                change.staged
            } else {
                change.unstaged && change.kind != ChangeKind::Untracked
            }
        })
        .filter(|change| {
            requested.is_empty()
                || requested.contains(change.path.as_str())
                || change
                    .original_path
                    .as_deref()
                    .is_some_and(|path| requested.contains(path))
        })
        .map(|change| {
            let side_kind = side_change_kind(change, staged);
            let side_path = side_change_path(change, staged);
            let (old_path, new_path) = match side_kind {
                ChangeKind::Added | ChangeKind::Untracked => (None, Some(side_path.clone())),
                ChangeKind::Deleted => (Some(side_path.clone()), None),
                ChangeKind::Renamed | ChangeKind::Copied => {
                    (change.original_path.clone(), Some(change.path.clone()))
                }
                ChangeKind::Modified
                | ChangeKind::TypeChanged
                | ChangeKind::Conflicted
                | ChangeKind::Unknown => (Some(side_path.clone()), Some(side_path.clone())),
            };
            DiffFile {
                path: side_path,
                old_path: old_path.clone(),
                new_path: new_path.clone(),
                binary: change.binary,
                additions: change.additions,
                deletions: change.deletions,
                old: old_path,
                new: new_path,
            }
        })
        .collect::<Vec<_>>();
    let binary = files.iter().any(|file| file.binary);
    let additions = files.iter().filter_map(|file| file.additions).sum();
    let deletions = files.iter().filter_map(|file| file.deletions).sum();
    DiffSummary {
        changes: files.clone(),
        entries: files.clone(),
        files,
        binary,
        additions,
        deletions,
    }
}

fn side_change_kind(change: &ChangeEntry, staged: bool) -> ChangeKind {
    if change.conflicted {
        return ChangeKind::Conflicted;
    }
    let code = if staged {
        change.index_status
    } else {
        change.worktree_status
    };
    match code {
        Some('A') => ChangeKind::Added,
        Some('M') => ChangeKind::Modified,
        Some('D') => ChangeKind::Deleted,
        Some('R') => ChangeKind::Renamed,
        Some('C') => ChangeKind::Copied,
        Some('T') => ChangeKind::TypeChanged,
        Some('U') => ChangeKind::Conflicted,
        _ => change.kind,
    }
}

fn side_change_path(change: &ChangeEntry, staged: bool) -> String {
    // For an unstaged rename (`MR`), the index-side modification still refers to the original
    // path. Once the index itself contains the rename (`RM`/`RD`), worktree-side changes refer to
    // the new path. `original_path` is otherwise reserved for the side whose status is R/C.
    if staged
        && !matches!(change.index_status, Some('R' | 'C'))
        && matches!(change.worktree_status, Some('R' | 'C'))
    {
        change
            .original_path
            .clone()
            .unwrap_or_else(|| change.path.clone())
    } else {
        change.path.clone()
    }
}

fn parse_filter_driver_names(input: &[u8]) -> Result<Vec<String>, AppError> {
    if input.is_empty() {
        return Ok(Vec::new());
    }
    if !input.ends_with(&[0]) {
        return Err(AppError::Git(
            "filter config query was malformed".to_owned(),
        ));
    }
    let mut drivers = BTreeSet::new();
    for record in input[..input.len() - 1].split(|byte| *byte == 0) {
        let key = std::str::from_utf8(record)
            .map_err(|_| AppError::Git("filter config query was malformed".to_owned()))?;
        let Some(without_prefix) = key.strip_prefix("filter.") else {
            return Err(AppError::Git(
                "filter config query was malformed".to_owned(),
            ));
        };
        let Some((driver, field)) = without_prefix.rsplit_once('.') else {
            return Err(AppError::Git(
                "filter config query was malformed".to_owned(),
            ));
        };
        if !matches!(field, "clean" | "smudge" | "process" | "required")
            || driver.is_empty()
            || driver.len() > MAX_FILTER_DRIVER_BYTES
            || !driver
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
        {
            return Err(AppError::Git(
                "filter config query was malformed".to_owned(),
            ));
        }
        drivers.insert(driver.to_owned());
        if drivers.len() > MAX_FILTER_DRIVERS {
            return Err(AppError::OutputLimit);
        }
    }
    Ok(drivers.into_iter().collect())
}

fn append_disabled_filter_overrides(args: &mut Vec<OsString>, drivers: &[String]) {
    for driver in drivers {
        for field in ["clean", "smudge", "process"] {
            args.push(OsString::from("-c"));
            args.push(OsString::from(format!("filter.{driver}.{field}=")));
        }
        args.push(OsString::from("-c"));
        args.push(OsString::from(format!("filter.{driver}.required=false")));
    }
}

pub fn validate_relative_path(input: &str) -> Result<String, AppError> {
    if input.is_empty() || input.len() > 4 * 1024 || input.contains('\0') {
        return Err(AppError::InvalidInput("path is invalid".to_owned()));
    }
    // Validate separators lexically instead of relying only on the host platform's `Path`
    // parser. A Unix host must reject Windows drive/UNC paths and `..\\` escapes just as a
    // Windows host would.
    let normalized = input.replace('\\', "/");
    let bytes = input.as_bytes();
    let drive_prefix = bytes.len() >= 2
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes.get(2).is_none()
            || bytes
                .get(2)
                .is_some_and(|byte| *byte == b'/' || *byte == b'\\'));
    if input.starts_with('/')
        || input.starts_with('\\')
        || drive_prefix
        || normalized.starts_with("//")
    {
        return Err(AppError::InvalidInput("path must be relative".to_owned()));
    }
    let mut result = String::new();
    for component in normalized.split('/') {
        if component.is_empty() || component == "." || component == ".." || component.contains(':')
        {
            return Err(AppError::InvalidInput(
                "path must be relative and confined".to_owned(),
            ));
        }
        if !result.is_empty() {
            result.push('/');
        }
        result.push_str(component);
    }
    if result.is_empty() {
        return Err(AppError::InvalidInput("path is empty".to_owned()));
    }
    Ok(result)
}

fn db_snapshot(
    repository_id: &str,
    parsed: &ParsedSnapshot,
    refresh_error_summary: Option<String>,
) -> RepositorySnapshot {
    RepositorySnapshot {
        id: Uuid::new_v4().to_string(),
        repository_id: repository_id.to_owned(),
        captured_at: Utc::now(),
        head_oid: parsed.head_oid.clone(),
        branch: parsed.branch.clone(),
        upstream: parsed.upstream.clone(),
        ahead: parsed.ahead as i64,
        behind: parsed.behind as i64,
        dirty: parsed.dirty,
        staged_count: parsed.staged_count as i64,
        unstaged_count: parsed.unstaged_count as i64,
        untracked_count: parsed.untracked_count as i64,
        conflicted_count: parsed.conflicted_count as i64,
        refresh_error_summary,
    }
}

fn ensure_success(output: &GitOutput) -> Result<(), AppError> {
    if output.status.success() {
        return Ok(());
    }
    Err(AppError::Git(sanitize_git_stderr(&output.stderr)))
}

fn sanitize_git_stderr(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let first = text
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("git command failed");
    let mut sanitized = first.trim().replace(['\r', '\n'], " ");
    if sanitized.len() > 512 {
        sanitized.truncate(512);
    }
    // Credentials and complete URLs are never returned to a caller.  Keep a stable message for
    // common remote/auth failures without attempting to parse arbitrary shell output.
    if sanitized.contains("://") || sanitized.to_ascii_lowercase().contains("password") {
        return "git command failed".to_owned();
    }
    sanitized
}

fn sanitize_text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .trim()
        .chars()
        .take(512)
        .collect()
}

fn sanitize_fs_error(error: std::io::Error) -> String {
    match error.kind() {
        std::io::ErrorKind::PermissionDenied => "permission denied".to_owned(),
        std::io::ErrorKind::NotFound => "path not found".to_owned(),
        _ => "unable to read directory".to_owned(),
    }
}

fn stable_error(error: &AppError) -> String {
    match error {
        AppError::Git(message) => {
            let bytes = message.as_bytes();
            sanitize_git_stderr(bytes)
        }
        _ => error.to_string(),
    }
}

fn is_candidate_error(error: &AppError) -> bool {
    matches!(error, AppError::InvalidInput(message) if message == "path is not a Git repository")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::parser::ChangeKind;

    #[test]
    fn glob_match_supports_simple_wildcards() {
        assert!(glob_match("node*", "node_modules"));
        assert!(glob_match("foo/?ar", "foo/bar"));
        assert!(!glob_match("foo/*", "bar/foo"));
        assert!(glob_match("*", ""));
        assert!(glob_match("foo*bar", "foo---bar"));
        assert!(!glob_match("foo?bar", "foobar"));
        assert!(!glob_match("foo", "foobar"));
    }

    #[test]
    fn relative_paths_reject_escape_and_absolute_forms() {
        assert!(validate_relative_path("src/main.rs").is_ok());
        assert!(validate_relative_path("../secret").is_err());
        assert!(validate_relative_path("C:\\secret").is_err());
    }

    #[test]
    fn project_locks_are_partitioned_by_project_id() {
        let service = GitService::new(Database::open_in_memory().unwrap());
        let project_a = service.project_lock("project-a");
        let project_a_again = service.project_lock("project-a");
        let project_b = service.project_lock("project-b");
        assert!(Arc::ptr_eq(&project_a, &project_a_again));
        assert!(!Arc::ptr_eq(&project_a, &project_b));
    }

    #[test]
    fn scan_entry_limit_counts_as_discovery_failure_not_record_failure() {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("child")).unwrap();
        let service = GitService::new(Database::open_in_memory().unwrap());
        let project = service
            .create_project(ProjectCreateInput {
                root_path: root.path().to_string_lossy().into_owned(),
                name: "entry limit".to_owned(),
                scan_depth: Some(1),
                exclude_patterns: Vec::new(),
            })
            .unwrap();

        let result = service
            .scan_project_with_progress_limit(&project.id, |_| {}, 1)
            .unwrap();

        assert_eq!((result.total, result.completed, result.failed), (0, 0, 0));
        assert_eq!(result.discovery_failed, 1);
        assert_eq!(result.failures.len(), 1);
        assert_eq!(result.failures[0].error, "scan entry limit exceeded");
        assert!(result.progress.is_empty());
    }

    #[test]
    fn safe_diff_summary_maps_added_deleted_and_renamed_aliases() {
        let changes = vec![
            xy_change("added.txt", None, ChangeKind::Added, "A."),
            xy_change("deleted.txt", None, ChangeKind::Deleted, "D."),
            xy_change(
                "new-name.txt",
                Some("old-name.txt"),
                ChangeKind::Renamed,
                "R.",
            ),
        ];
        let summary = safe_diff_summary(&changes, &[], true);
        assert_eq!(summary.files, summary.changes);
        assert_eq!(summary.files, summary.entries);
        assert_eq!(summary.files.len(), 3);
        assert_eq!(summary.files[0].old_path, None);
        assert_eq!(summary.files[0].new_path.as_deref(), Some("added.txt"));
        assert_eq!(summary.files[1].old_path.as_deref(), Some("deleted.txt"));
        assert_eq!(summary.files[1].new_path, None);
        assert_eq!(summary.files[2].old_path.as_deref(), Some("old-name.txt"));
        assert_eq!(summary.files[2].new_path.as_deref(), Some("new-name.txt"));
    }

    #[test]
    fn safe_diff_summary_maps_each_xy_side_independently() {
        let cases = [
            (
                "AD",
                ChangeKind::Added,
                None,
                (None, Some("path.txt")),
                (Some("path.txt"), None),
            ),
            (
                "AM",
                ChangeKind::Added,
                None,
                (None, Some("path.txt")),
                (Some("path.txt"), Some("path.txt")),
            ),
            (
                "MD",
                ChangeKind::Modified,
                None,
                (Some("path.txt"), Some("path.txt")),
                (Some("path.txt"), None),
            ),
            (
                "RM",
                ChangeKind::Renamed,
                Some("old.txt"),
                (Some("old.txt"), Some("path.txt")),
                (Some("path.txt"), Some("path.txt")),
            ),
            (
                "RD",
                ChangeKind::Renamed,
                Some("old.txt"),
                (Some("old.txt"), Some("path.txt")),
                (Some("path.txt"), None),
            ),
            (
                "MR",
                ChangeKind::Renamed,
                Some("old.txt"),
                (Some("old.txt"), Some("old.txt")),
                (Some("old.txt"), Some("path.txt")),
            ),
        ];
        for (xy, kind, original, staged_paths, unstaged_paths) in cases {
            let change = xy_change("path.txt", original, kind, xy);
            let staged = safe_diff_summary(std::slice::from_ref(&change), &[], true);
            let unstaged = safe_diff_summary(std::slice::from_ref(&change), &[], false);
            assert_eq!(
                (
                    staged.files[0].old_path.as_deref(),
                    staged.files[0].new_path.as_deref()
                ),
                staged_paths,
                "staged side for {xy}"
            );
            assert_eq!(
                (
                    unstaged.files[0].old_path.as_deref(),
                    unstaged.files[0].new_path.as_deref()
                ),
                unstaged_paths,
                "unstaged side for {xy}"
            );
            assert_eq!(staged.files, staged.changes);
            assert_eq!(unstaged.files, unstaged.entries);
        }

        for xy in ["AA", "UU"] {
            let mut change = xy_change("conflict.txt", None, ChangeKind::Conflicted, xy);
            change.conflicted = true;
            let staged = safe_diff_summary(std::slice::from_ref(&change), &[], true);
            let unstaged = safe_diff_summary(std::slice::from_ref(&change), &[], false);
            assert_eq!(
                (
                    staged.files[0].old_path.as_deref(),
                    staged.files[0].new_path.as_deref()
                ),
                (Some("conflict.txt"), Some("conflict.txt")),
                "staged conflict for {xy}"
            );
            assert_eq!(
                (
                    unstaged.files[0].old_path.as_deref(),
                    unstaged.files[0].new_path.as_deref()
                ),
                (Some("conflict.txt"), Some("conflict.txt")),
                "unstaged conflict for {xy}"
            );
        }
    }

    #[test]
    fn filter_driver_name_parser_is_bounded_and_fail_closed() {
        assert!(parse_filter_driver_names(b"").unwrap().is_empty());
        assert_eq!(
            parse_filter_driver_names(
                b"filter.evil.clean\0filter.evil.process\0filter.other.required\0"
            )
            .unwrap(),
            vec!["evil".to_owned(), "other".to_owned()]
        );
        assert!(parse_filter_driver_names(b"filter.bad=name.clean\0").is_err());
        assert!(parse_filter_driver_names(b"filter.evil.clean").is_err());

        let mut excessive = Vec::new();
        for index in 0..=MAX_FILTER_DRIVERS {
            excessive.extend_from_slice(format!("filter.driver{index}.clean\0").as_bytes());
        }
        assert!(matches!(
            parse_filter_driver_names(&excessive),
            Err(AppError::OutputLimit)
        ));
    }

    fn change(
        path: &str,
        original_path: Option<&str>,
        kind: ChangeKind,
        staged: bool,
        unstaged: bool,
    ) -> ChangeEntry {
        ChangeEntry {
            path: path.to_owned(),
            original_path: original_path.map(str::to_owned),
            kind,
            staged,
            unstaged,
            conflicted: false,
            binary: false,
            old: original_path.map(str::to_owned),
            new: original_path.map(|_| path.to_owned()),
            old_path: original_path.map(str::to_owned),
            new_path: original_path.map(|_| path.to_owned()),
            status: "M.".to_owned(),
            index_status: Some('M'),
            worktree_status: Some('.'),
            additions: None,
            deletions: None,
        }
    }

    fn xy_change(
        path: &str,
        original_path: Option<&str>,
        kind: ChangeKind,
        xy: &str,
    ) -> ChangeEntry {
        let mut change = change(path, original_path, kind, true, true);
        let mut chars = xy.chars();
        let index = chars.next().unwrap();
        let worktree = chars.next().unwrap();
        change.status = xy.to_owned();
        change.index_status = Some(index);
        change.worktree_status = Some(worktree);
        change.staged = index != '.' && index != '?';
        change.unstaged = worktree != '.' && worktree != '?';
        change
    }
}
