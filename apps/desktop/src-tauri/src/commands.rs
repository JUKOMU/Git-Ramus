use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::app_state::AppState;
use crate::error::{AppError, ErrorEnvelope};
use crate::git::model::{Project, Trust, Workspace};
use crate::git::service::{
    ChangesResult, DiffResult, Overview, ProjectCreateInput, QueryContext, RepositoryScanRecord,
    ScanProjectResult, WorkspaceCreateInput, WorkspaceMembershipInput, WriteResult,
};
use crate::jobs::model::Job;
use crate::plugins::PluginDescriptor;

pub type CommandResult<T> = Result<T, Box<ErrorEnvelope>>;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AuthorizationRequest {
    pub plugin_id: String,
    pub capability: String,
    pub resource: String,
}

#[derive(Debug, Serialize)]
pub struct AuthorizationDecision {
    pub allowed: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EchoJobRequest {
    pub plugin_id: String,
    pub message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitProjectCreateRequest {
    pub root_path: String,
    pub name: String,
    pub scan_depth: Option<i64>,
    #[serde(default)]
    pub exclude_patterns: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitProjectUpdateScanRulesRequest {
    pub project_id: String,
    pub scan_depth: Option<i64>,
    pub exclude_patterns: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitProjectScanRequest {
    pub project_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitWorkspaceCreateRequest {
    pub name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitWorkspaceUpdateMembershipRequest {
    pub workspace_id: String,
    pub project_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitWorkspaceDeleteRequest {
    pub workspace_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitContextRequest {
    pub project_id: Option<String>,
    pub workspace_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitRepositoryRequest {
    pub project_id: Option<String>,
    pub workspace_id: Option<String>,
    pub repository_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitRepositoryDiffRequest {
    pub project_id: Option<String>,
    pub workspace_id: Option<String>,
    pub repository_id: String,
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub staged: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitRepositoryStageRequest {
    pub project_id: Option<String>,
    pub workspace_id: Option<String>,
    pub repository_id: String,
    #[serde(default)]
    pub paths: Vec<String>,
    #[serde(default)]
    pub all: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitRepositoryUnstageRequest {
    pub project_id: Option<String>,
    pub workspace_id: Option<String>,
    pub repository_id: String,
    pub paths: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitRepositoryCommitRequest {
    pub project_id: Option<String>,
    pub workspace_id: Option<String>,
    pub repository_id: String,
    pub message: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitRepositoryTrustRequest {
    pub project_id: Option<String>,
    pub workspace_id: Option<String>,
    pub repository_id: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitProjectListResponse {
    pub projects: Vec<Project>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitWorkspaceListResponse {
    pub workspaces: Vec<Workspace>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitTrustResponse {
    pub trust: Trust,
}

#[tauri::command]
pub fn get_app_info() -> AppInfo {
    app_info()
}

pub fn app_info() -> AppInfo {
    AppInfo {
        name: "Git-Ramus".to_owned(),
        version: env!("CARGO_PKG_VERSION").to_owned(),
    }
}

#[tauri::command]
pub fn list_plugins(state: State<'_, AppState>) -> Vec<PluginDescriptor> {
    state.plugins.descriptors().to_vec()
}

#[tauri::command]
pub fn list_jobs(state: State<'_, AppState>) -> CommandResult<Vec<Job>> {
    state.jobs.list().map_err(command_error)
}

#[tauri::command]
pub fn authorize_plugin_call(
    state: State<'_, AppState>,
    request: AuthorizationRequest,
) -> CommandResult<AuthorizationDecision> {
    state
        .permissions
        .is_allowed(&request.plugin_id, &request.capability, &request.resource)
        .map(|allowed| AuthorizationDecision { allowed })
        .map_err(command_error)
}

#[tauri::command]
pub async fn start_echo_job(
    app: AppHandle,
    state: State<'_, AppState>,
    request: EchoJobRequest,
) -> CommandResult<Job> {
    if request.message.trim().is_empty() || request.message.chars().count() > 256 {
        return Err(command_error(AppError::InvalidInput(
            "echo message must contain 1 to 256 characters".to_owned(),
        )));
    }
    if !state
        .permissions
        .is_allowed(&request.plugin_id, "tasks:create", "echo")
        .map_err(command_error)?
    {
        return Err(command_error(AppError::PermissionDenied));
    }
    let job = state
        .jobs
        .create("system.echo", &format!("Echo {}", request.message))
        .map_err(command_error)?;
    let runner = state.jobs.clone();
    let job_id = job.id.clone();
    tauri::async_runtime::spawn(async move {
        if let Ok(started) = runner.start(&job_id) {
            let _ = app.emit("job://updated", started);
        }
        for progress in [0.25, 0.5, 0.75] {
            tokio::time::sleep(std::time::Duration::from_millis(150)).await;
            if runner.is_canceled(&job_id).unwrap_or(true) {
                return;
            }
            if let Ok(updated) = runner.set_progress(&job_id, progress) {
                let _ = app.emit("job://updated", updated);
            }
        }
        if let Ok(completed) = runner.succeed(&job_id) {
            let _ = app.emit("job://updated", completed);
        }
    });
    Ok(job)
}

#[tauri::command]
pub fn cancel_job(app: AppHandle, state: State<'_, AppState>, job_id: String) -> CommandResult<()> {
    let canceled = state.jobs.cancel(&job_id).map_err(command_error)?;
    let _ = app.emit("job://updated", canceled);
    Ok(())
}

#[tauri::command]
pub fn git_project_create(
    state: State<'_, AppState>,
    request: GitProjectCreateRequest,
) -> CommandResult<Project> {
    state
        .git
        .create_project(ProjectCreateInput {
            root_path: request.root_path,
            name: request.name,
            scan_depth: request.scan_depth,
            exclude_patterns: request.exclude_patterns,
        })
        .map_err(command_error)
}

#[tauri::command]
pub fn git_project_list(state: State<'_, AppState>) -> CommandResult<GitProjectListResponse> {
    state
        .git
        .list_projects()
        .map(|projects| GitProjectListResponse { projects })
        .map_err(command_error)
}

#[tauri::command]
pub fn git_project_update_scan_rules(
    state: State<'_, AppState>,
    request: GitProjectUpdateScanRulesRequest,
) -> CommandResult<Project> {
    state
        .git
        .update_scan_rules(
            &request.project_id,
            request.scan_depth,
            request.exclude_patterns,
        )
        .map_err(command_error)
}

#[tauri::command]
pub fn git_project_scan(
    state: State<'_, AppState>,
    request: GitProjectScanRequest,
) -> CommandResult<ScanProjectResult> {
    state
        .git
        .scan_project(&request.project_id)
        .map_err(command_error)
}

#[tauri::command]
pub fn git_workspace_create(
    state: State<'_, AppState>,
    request: GitWorkspaceCreateRequest,
) -> CommandResult<Workspace> {
    state
        .git
        .create_workspace(WorkspaceCreateInput { name: request.name })
        .map_err(command_error)
}

#[tauri::command]
pub fn git_workspace_list(state: State<'_, AppState>) -> CommandResult<GitWorkspaceListResponse> {
    state
        .git
        .list_workspaces()
        .map(|workspaces| GitWorkspaceListResponse { workspaces })
        .map_err(command_error)
}

#[tauri::command]
pub fn git_workspace_update_membership(
    state: State<'_, AppState>,
    request: GitWorkspaceUpdateMembershipRequest,
) -> CommandResult<Vec<String>> {
    state
        .git
        .update_workspace_membership(WorkspaceMembershipInput {
            workspace_id: request.workspace_id,
            project_ids: request.project_ids,
        })
        .map_err(command_error)
}

#[tauri::command]
pub fn git_workspace_delete(
    state: State<'_, AppState>,
    request: GitWorkspaceDeleteRequest,
) -> CommandResult<()> {
    state
        .git
        .delete_workspace(&request.workspace_id)
        .map_err(command_error)
}

#[tauri::command]
pub fn git_overview_get(
    state: State<'_, AppState>,
    request: GitContextRequest,
) -> CommandResult<Overview> {
    state
        .git
        .get_overview(&context_from(request)?)
        .map_err(command_error)
}

#[tauri::command]
pub fn git_repository_snapshot(
    state: State<'_, AppState>,
    request: GitRepositoryRequest,
) -> CommandResult<RepositoryScanRecord> {
    state
        .git
        .get_snapshot(&context_from_request(&request)?, &request.repository_id)
        .map_err(command_error)
}

#[tauri::command]
pub fn git_repository_changes(
    state: State<'_, AppState>,
    request: GitRepositoryRequest,
) -> CommandResult<ChangesResult> {
    state
        .git
        .get_changes(&context_from_request(&request)?, &request.repository_id)
        .map_err(command_error)
}

#[tauri::command]
pub fn git_repository_diff(
    state: State<'_, AppState>,
    request: GitRepositoryDiffRequest,
) -> CommandResult<DiffResult> {
    let context = QueryContext {
        project_id: request.project_id,
        workspace_id: request.workspace_id,
    };
    state
        .git
        .get_diff(
            &context,
            &request.repository_id,
            &request.paths,
            request.staged,
        )
        .map_err(command_error)
}

#[tauri::command]
pub fn git_repository_stage(
    state: State<'_, AppState>,
    request: GitRepositoryStageRequest,
) -> CommandResult<WriteResult> {
    let context = QueryContext {
        project_id: request.project_id,
        workspace_id: request.workspace_id,
    };
    state
        .git
        .stage(
            &context,
            &request.repository_id,
            &request.paths,
            request.all,
        )
        .map_err(command_error)
}

#[tauri::command]
pub fn git_repository_unstage(
    state: State<'_, AppState>,
    request: GitRepositoryUnstageRequest,
) -> CommandResult<WriteResult> {
    let context = QueryContext {
        project_id: request.project_id,
        workspace_id: request.workspace_id,
    };
    state
        .git
        .unstage(&context, &request.repository_id, &request.paths)
        .map_err(command_error)
}

#[tauri::command]
pub fn git_repository_commit(
    state: State<'_, AppState>,
    request: GitRepositoryCommitRequest,
) -> CommandResult<WriteResult> {
    let context = QueryContext {
        project_id: request.project_id,
        workspace_id: request.workspace_id,
    };
    state
        .git
        .commit(&context, &request.repository_id, &request.message)
        .map_err(command_error)
}

#[tauri::command]
pub fn git_repository_trust(
    state: State<'_, AppState>,
    request: GitRepositoryTrustRequest,
) -> CommandResult<GitTrustResponse> {
    let context = QueryContext {
        project_id: request.project_id,
        workspace_id: request.workspace_id,
    };
    state
        .git
        .trust_repository_in_context(&context, &request.repository_id)
        .map(|trust| GitTrustResponse { trust })
        .map_err(command_error)
}

fn context_from(request: GitContextRequest) -> Result<QueryContext, Box<ErrorEnvelope>> {
    let context = QueryContext {
        project_id: request.project_id,
        workspace_id: request.workspace_id,
    };
    context
        .validate_for_command()
        .map(|_| context)
        .map_err(command_error)
}

fn context_from_request(
    request: &GitRepositoryRequest,
) -> Result<QueryContext, Box<ErrorEnvelope>> {
    context_from(GitContextRequest {
        project_id: request.project_id.clone(),
        workspace_id: request.workspace_id.clone(),
    })
}

fn command_error(error: AppError) -> Box<ErrorEnvelope> {
    Box::new(ErrorEnvelope::from(error))
}

#[cfg(test)]
mod tests {
    use super::app_info;

    #[test]
    fn app_info_uses_compile_time_package_metadata() {
        let info = app_info();
        assert_eq!(info.name, "Git-Ramus");
        assert_eq!(info.version, "0.1.0");
    }
}
