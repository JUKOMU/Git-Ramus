use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};

use crate::app_state::AppState;
use crate::error::{AppError, ErrorEnvelope};
use crate::git::model::{IdentityBinding, Project, Trust, Workspace};
use crate::git::service::{
    ChangesResult, DiffResult, Overview, ProjectCreateInput, QueryContext, RepositoryScanRecord,
    ScanProjectResult, WorkspaceCreateInput, WorkspaceMembershipInput, WriteResult,
};
use crate::identity::{EffectiveIdentity, IdentityProfile, IdentityProfileInput};
use crate::jobs::model::Job;
use crate::plugins::PluginDescriptor;
use crate::themes::{ThemeMetadata, ThemeState};

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
pub struct GitProjectUpdateRequest {
    pub project_id: String,
    pub root_path: Option<String>,
    pub name: Option<String>,
    pub scan_depth: Option<i64>,
    pub exclude_patterns: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitProjectDeleteRequest {
    pub project_id: String,
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
pub struct GitWorkspaceRequest {
    pub workspace_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitWorkspaceUpdateRequest {
    pub workspace_id: String,
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
    pub identity_profile_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitRepositoryTrustRequest {
    pub project_id: Option<String>,
    pub workspace_id: Option<String>,
    pub repository_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitIdentityCreateRequest {
    pub display_name: String,
    pub user_name: String,
    pub user_email: String,
    pub gpg_format: Option<String>,
    pub signing_key: Option<String>,
    #[serde(default)]
    pub sign_commits: bool,
    #[serde(default)]
    pub sign_tags: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitIdentityUpdateRequest {
    pub profile_id: String,
    pub display_name: String,
    pub user_name: String,
    pub user_email: String,
    pub gpg_format: Option<String>,
    pub signing_key: Option<String>,
    #[serde(default)]
    pub sign_commits: bool,
    #[serde(default)]
    pub sign_tags: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitIdentityProfileRequest {
    pub profile_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitRepositoryIdentityBindRequest {
    pub project_id: Option<String>,
    pub workspace_id: Option<String>,
    pub repository_id: String,
    pub identity_profile_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitRepositoryIdentityRequest {
    pub project_id: Option<String>,
    pub workspace_id: Option<String>,
    pub repository_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitProjectListResponse {
    pub projects: Vec<Project>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitWorkspaceListResponse {
    pub workspaces: Vec<Workspace>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitTrustResponse {
    pub trust: Trust,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitRepositoryTrustStatusResponse {
    pub trusted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitIdentityListResponse {
    pub identities: Vec<IdentityProfile>,
    pub global_identity_profile_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeActivateRequest {
    pub theme_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ThemeCatalogResponse {
    pub themes: Vec<ThemeMetadata>,
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
pub fn list_themes(state: State<'_, AppState>) -> ThemeCatalogResponse {
    ThemeCatalogResponse {
        themes: state.themes.list(),
    }
}

#[tauri::command]
pub fn current_theme(state: State<'_, AppState>) -> CommandResult<ThemeState> {
    state.themes.current().map_err(command_error)
}

#[tauri::command]
pub fn activate_theme(
    app: AppHandle,
    state: State<'_, AppState>,
    request: ThemeActivateRequest,
) -> CommandResult<ThemeState> {
    let theme = state
        .themes
        .activate(&request.theme_id)
        .map_err(command_error)?;
    let _ = app.emit("theme://changed", theme.clone());
    Ok(theme)
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
pub fn git_project_update(
    state: State<'_, AppState>,
    request: GitProjectUpdateRequest,
) -> CommandResult<Project> {
    state
        .git
        .update_project(crate::git::service::ProjectUpdateInput {
            project_id: request.project_id,
            root_path: request.root_path,
            name: request.name,
            scan_depth: request.scan_depth,
            exclude_patterns: request.exclude_patterns,
        })
        .map_err(command_error)
}

#[tauri::command]
pub fn git_project_delete(
    state: State<'_, AppState>,
    request: GitProjectDeleteRequest,
) -> CommandResult<()> {
    state
        .git
        .delete_project_by_id(&request.project_id)
        .map_err(command_error)
}

#[tauri::command]
pub fn git_project_scan(
    app: AppHandle,
    state: State<'_, AppState>,
    request: GitProjectScanRequest,
) -> CommandResult<ScanProjectResult> {
    let progress_app = app.clone();
    state
        .git
        .scan_project_with_progress(&request.project_id, move |record| {
            let _ = progress_app.emit("git://scan-progress", record);
        })
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
pub fn git_workspace_get_membership(
    state: State<'_, AppState>,
    request: GitWorkspaceRequest,
) -> CommandResult<Vec<String>> {
    state
        .git
        .workspace_projects(&request.workspace_id)
        .map_err(command_error)
}

#[tauri::command]
pub fn git_workspace_update(
    state: State<'_, AppState>,
    request: GitWorkspaceUpdateRequest,
) -> CommandResult<Workspace> {
    state
        .git
        .update_workspace(&request.workspace_id, &request.name)
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
pub fn git_repository_trust_status(
    state: State<'_, AppState>,
    request: GitRepositoryRequest,
) -> CommandResult<GitRepositoryTrustStatusResponse> {
    state
        .git
        .is_repository_trusted_in_context(&context_from_request(&request)?, &request.repository_id)
        .map(|trusted| GitRepositoryTrustStatusResponse { trusted })
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
        .commit_with_identity(
            &context,
            &request.repository_id,
            &request.message,
            &state.identities,
            request.identity_profile_id.as_deref(),
        )
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

#[tauri::command]
pub fn git_identity_list(state: State<'_, AppState>) -> CommandResult<GitIdentityListResponse> {
    let identities = state.identities.list().map_err(command_error)?;
    let global_identity_profile_id = state
        .identities
        .global_profile_id()
        .map_err(command_error)?;
    Ok(GitIdentityListResponse {
        identities,
        global_identity_profile_id,
    })
}

#[tauri::command]
pub fn git_identity_create(
    state: State<'_, AppState>,
    request: GitIdentityCreateRequest,
) -> CommandResult<IdentityProfile> {
    state
        .identities
        .create(IdentityProfileInput {
            display_name: request.display_name,
            user_name: request.user_name,
            user_email: request.user_email,
            gpg_format: request.gpg_format,
            signing_key: request.signing_key,
            sign_commits: request.sign_commits,
            sign_tags: request.sign_tags,
        })
        .map_err(command_error)
}

#[tauri::command]
pub fn git_identity_update(
    state: State<'_, AppState>,
    request: GitIdentityUpdateRequest,
) -> CommandResult<IdentityProfile> {
    state
        .identities
        .update(
            &request.profile_id,
            IdentityProfileInput {
                display_name: request.display_name,
                user_name: request.user_name,
                user_email: request.user_email,
                gpg_format: request.gpg_format,
                signing_key: request.signing_key,
                sign_commits: request.sign_commits,
                sign_tags: request.sign_tags,
            },
        )
        .map_err(command_error)
}

#[tauri::command]
pub fn git_identity_delete(
    state: State<'_, AppState>,
    request: GitIdentityProfileRequest,
) -> CommandResult<()> {
    state
        .identities
        .delete(&request.profile_id)
        .map_err(command_error)
}

#[tauri::command]
pub fn git_identity_set_global(
    state: State<'_, AppState>,
    request: GitIdentityProfileRequest,
) -> CommandResult<IdentityProfile> {
    state
        .identities
        .set_global(&request.profile_id)
        .map_err(command_error)
}

#[tauri::command]
pub fn git_repository_bind_identity(
    state: State<'_, AppState>,
    request: GitRepositoryIdentityBindRequest,
) -> CommandResult<IdentityBinding> {
    let context = QueryContext {
        project_id: request.project_id,
        workspace_id: request.workspace_id,
    };
    state
        .git
        .validate_repository_context(&context, &request.repository_id)
        .map_err(command_error)?;
    state
        .identities
        .bind_repository(&request.repository_id, &request.identity_profile_id)
        .map_err(command_error)
}

#[tauri::command]
pub fn git_repository_unbind_identity(
    state: State<'_, AppState>,
    request: GitRepositoryIdentityRequest,
) -> CommandResult<()> {
    let context = QueryContext {
        project_id: request.project_id,
        workspace_id: request.workspace_id,
    };
    state
        .git
        .validate_repository_context(&context, &request.repository_id)
        .map_err(command_error)?;
    state
        .identities
        .unbind_repository(&request.repository_id)
        .map_err(command_error)
}

#[tauri::command]
pub fn git_repository_effective_identity(
    state: State<'_, AppState>,
    request: GitRepositoryIdentityRequest,
) -> CommandResult<EffectiveIdentity> {
    let context = QueryContext {
        project_id: request.project_id,
        workspace_id: request.workspace_id,
    };
    state
        .git
        .validate_repository_context(&context, &request.repository_id)
        .map_err(command_error)?;
    state
        .identities
        .effective_for_repository(&request.repository_id)
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
    use super::{
        GitIdentityCreateRequest, GitProjectCreateRequest, GitProjectDeleteRequest,
        GitRepositoryCommitRequest, GitRepositoryIdentityBindRequest, GitWorkspaceRequest,
        GitWorkspaceUpdateRequest, ThemeActivateRequest, ThemeCatalogResponse,
    };
    use crate::themes::{ThemeDensity, ThemeMetadata};
    use serde_json::json;

    #[test]
    fn app_info_uses_compile_time_package_metadata() {
        let info = app_info();
        assert_eq!(info.name, "Git-Ramus");
        assert_eq!(info.version, "0.1.0");
    }

    #[test]
    fn git_request_dtos_reject_unknown_fields() {
        assert!(
            serde_json::from_value::<GitProjectCreateRequest>(json!({
                "rootPath": ".",
                "name": "fixture",
                "unexpected": true
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<GitProjectDeleteRequest>(json!({
                "projectId": "p",
                "unexpected": true
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<GitWorkspaceUpdateRequest>(json!({
                "workspaceId": "w",
                "name": "fixture",
                "unexpected": true
            }))
            .is_err()
        );
        let membership: GitWorkspaceRequest = serde_json::from_value(json!({
            "workspaceId": "workspace"
        }))
        .expect("workspace membership request parses");
        assert_eq!(membership.workspace_id, "workspace");
        assert!(
            serde_json::from_value::<GitWorkspaceRequest>(json!({
                "workspaceId": "workspace",
                "rootPath": "C:/must-not-cross-boundary"
            }))
            .is_err()
        );
    }

    #[test]
    fn identity_and_commit_requests_are_typed_camel_case_and_reject_unknown_fields() {
        let commit: GitRepositoryCommitRequest = serde_json::from_value(json!({
            "projectId": "project",
            "workspaceId": null,
            "repositoryId": "repository",
            "message": "message",
            "identityProfileId": "profile"
        }))
        .expect("commit request parses");
        assert_eq!(commit.identity_profile_id.as_deref(), Some("profile"));

        assert!(
            serde_json::from_value::<GitIdentityCreateRequest>(json!({
                "displayName": "Alice",
                "userName": "Alice",
                "userEmail": "alice@example.com",
                "gpgFormat": null,
                "signingKey": null,
                "signCommits": false,
                "signTags": false,
                "unexpected": true
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<GitRepositoryIdentityBindRequest>(json!({
                "projectId": "project",
                "workspaceId": null,
                "repositoryId": "repository",
                "identityProfileId": "profile",
                "path": "C:/must-not-be-accepted"
            }))
            .is_err()
        );
    }

    #[test]
    fn identity_list_response_exposes_the_unique_global_profile_pointer() {
        let response = super::GitIdentityListResponse {
            identities: Vec::new(),
            global_identity_profile_id: Some("profile".to_owned()),
        };
        assert_eq!(
            serde_json::to_value(response).unwrap(),
            json!({
                "identities": [],
                "globalIdentityProfileId": "profile"
            })
        );
    }

    #[test]
    fn theme_commands_use_strict_camel_case_contracts() {
        let request: ThemeActivateRequest = serde_json::from_value(json!({
            "themeId": "git-ramus.theme.compact"
        }))
        .expect("activation request parses");
        assert_eq!(request.theme_id, "git-ramus.theme.compact");
        assert!(
            serde_json::from_value::<ThemeActivateRequest>(json!({
                "themeId": "git-ramus.theme.compact",
                "definitionPath": "C:/must-not-cross-boundary"
            }))
            .is_err()
        );
        let response = ThemeCatalogResponse {
            themes: vec![ThemeMetadata {
                theme_id: "git-ramus.theme.default".to_owned(),
                name: "Git-Ramus Default".to_owned(),
                plugin_id: "git-ramus.host".to_owned(),
                version: "0.1.0".to_owned(),
                density: ThemeDensity::Comfortable,
            }],
        };
        assert_eq!(
            serde_json::to_value(response).expect("catalog serializes"),
            json!({
                "themes": [{
                    "themeId": "git-ramus.theme.default",
                    "name": "Git-Ramus Default",
                    "pluginId": "git-ramus.host",
                    "version": "0.1.0",
                    "density": "comfortable"
                }]
            })
        );
    }
}
