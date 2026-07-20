use std::path::PathBuf;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, State};
use tauri_plugin_dialog::DialogExt;

use crate::app_state::AppState;
use crate::error::{AppError, ErrorEnvelope};
use crate::git::model::{IdentityBinding, Project, Trust, Workspace};
use crate::git::service::{
    ChangesResult, DiffResult, Overview, ProjectCreateInput, QueryContext, RepositoryScanRecord,
    ScanProjectResult, WorkspaceCreateInput, WorkspaceMembershipInput, WriteResult,
};
use crate::git::transport::clone::GIT_CLIENT_PLUGIN_ID;
use crate::git::transport::model::{
    CloneInput, CloneIntent, CloneProjectTarget, CloneResult, CloneSource, EffectiveTransport,
    FetchInput, NetworkOperationResult, NetworkProgress, PullInput, PushInput, PushTarget,
    RepositoryNetworkState, RepositoryTransportBindingSummary, TransportKind,
    TransportProfileSummary,
};
use crate::git::transport::operation::TransportAuthorizationDomain;
use crate::git::transport::profile_service::{DriftResolution, ProfileDeletionResolution};
use crate::git::transport::service::NetworkProgressReporter;
use crate::identity::{EffectiveIdentity, IdentityProfile, IdentityProfileInput};
use crate::jobs::model::Job;
use crate::plugins::manifest::PluginKind;
use crate::plugins::permissions::PermissionGateway;
use crate::plugins::{PluginDescriptor, PluginRegistry};
use crate::providers::model::{
    AccountDeletionImpact, AccountDeletionResolution, ProviderAccountSummary,
    ProviderAuthorizedAccount, ProviderBinding, ProviderBindingSuggestion, ProviderInstanceSummary,
    ProviderKind, ProviderRepositoryPage, ProviderRepositoryQuery,
};
use crate::providers::service::{
    BindRemoteInput, CreateInstanceInput, CustomCaUpdate, DeleteAccountInput,
    ListRepositoriesInput, ProviderService, UpdateInstanceInput,
};
use crate::secrets::SensitiveString;
use crate::themes::{ThemeMetadata, ThemeState};
use uuid::Uuid;

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

const TRANSPORT_PROFILES_RESOURCE: &str = "transport-profiles";
const CLONE_INTENTS_RESOURCE: &str = "clone-intents";
const REPOSITORIES_RESOURCE: &str = "repositories";
const PROVIDER_CENTER_PLUGIN_ID: &str = "git-ramus.provider-center";

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitTransportPluginRequest {
    pub plugin_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "lowercase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum GitTransportProfileCreateNativeRequest {
    Ssh {
        plugin_id: String,
        display_name: String,
        ssh_key_path: PathBuf,
        identities_only: bool,
    },
    Https {
        plugin_id: String,
        display_name: String,
        username: String,
        use_http_path: bool,
    },
}

impl GitTransportProfileCreateNativeRequest {
    fn plugin_id(&self) -> &str {
        match self {
            Self::Ssh { plugin_id, .. } | Self::Https { plugin_id, .. } => plugin_id,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "lowercase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum GitTransportProfileUpdateNativeRequest {
    Ssh {
        plugin_id: String,
        profile_id: String,
        display_name: String,
        ssh_key_path: Option<PathBuf>,
        identities_only: bool,
    },
    Https {
        plugin_id: String,
        profile_id: String,
        display_name: String,
        username: String,
        use_http_path: bool,
    },
}

impl GitTransportProfileUpdateNativeRequest {
    fn plugin_id(&self) -> &str {
        match self {
            Self::Ssh { plugin_id, .. } | Self::Https { plugin_id, .. } => plugin_id,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitTransportProfileNativeRequest {
    pub plugin_id: String,
    pub profile_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitTransportProfileDeleteNativeRequest {
    pub plugin_id: String,
    pub profile_id: String,
    pub resolutions: Vec<ProfileDeletionResolution>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitTransportProfileListResponse {
    pub items: Vec<TransportProfileSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitTransportProfileDeletionImpactResponse {
    pub profile_id: String,
    pub repositories: Vec<GitTransportProfileDeletionRepository>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitTransportProfileDeletionRepository {
    pub repository_id: String,
    pub display_name: String,
    pub transport_kind: TransportKind,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitRepositoryTransportRequest {
    pub plugin_id: String,
    pub project_id: Option<String>,
    pub workspace_id: Option<String>,
    pub repository_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitRepositoryBindTransportNativeRequest {
    pub plugin_id: String,
    pub project_id: Option<String>,
    pub workspace_id: Option<String>,
    pub repository_id: String,
    pub transport_profile_id: String,
    pub replace_existing: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitRepositoryUnbindTransportNativeRequest {
    pub plugin_id: String,
    pub project_id: Option<String>,
    pub workspace_id: Option<String>,
    pub repository_id: String,
    pub drift_resolution: DriftResolution,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitCloneIntentCreateNativeRequest {
    pub plugin_id: String,
    pub account_id: String,
    pub repository_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitCloneIntentNativeRequest {
    pub plugin_id: String,
    pub intent_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitCloneIntentReference {
    pub intent_id: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase",
    deny_unknown_fields
)]
pub enum CloneSourceRequest {
    Intent { intent_id: String },
    Manual { remote_url: String },
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitCloneNativeRequest {
    pub plugin_id: String,
    pub source: CloneSourceRequest,
    pub transport_kind: TransportKind,
    pub profile_id: Option<String>,
    pub destination_parent: PathBuf,
    pub folder_name: String,
    pub project_target: CloneProjectTarget,
    pub operation_id: String,
    pub interactive_confirmed: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitRepositoryFetchNativeRequest {
    pub plugin_id: String,
    pub project_id: Option<String>,
    pub workspace_id: Option<String>,
    pub repository_id: String,
    pub remote_name: String,
    pub operation_id: String,
    pub interactive_confirmed: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitRepositoryPullNativeRequest {
    pub plugin_id: String,
    pub project_id: Option<String>,
    pub workspace_id: Option<String>,
    pub repository_id: String,
    pub operation_id: String,
    pub interactive_confirmed: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitPushTargetRequest {
    pub remote_name: String,
    pub branch_name: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitRepositoryPushNativeRequest {
    pub plugin_id: String,
    pub project_id: Option<String>,
    pub workspace_id: Option<String>,
    pub repository_id: String,
    pub target: Option<GitPushTargetRequest>,
    pub operation_id: String,
    pub interactive_confirmed: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GitTransportOperationCancelNativeRequest {
    pub plugin_id: String,
    pub operation_id: String,
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

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderInstanceCreateCommandRequest {
    pub provider_kind: ProviderKind,
    pub display_name: String,
    pub base_url: String,
    pub custom_ca_path: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderInstanceUpdateCommandRequest {
    pub instance_id: String,
    pub display_name: String,
    pub base_url: String,
    pub custom_ca: CustomCaUpdate,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderInstanceRequest {
    pub instance_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderAccountListRequest {
    pub instance_id: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderAccountConnectRequest {
    pub instance_id: String,
    pub pat: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderAccountRotateRequest {
    pub account_id: String,
    pub pat: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderAccountRequest {
    pub account_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderAccountSetDefaultRequest {
    pub instance_id: String,
    pub account_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderAccountDeleteRequest {
    pub account_id: String,
    pub resolution: AccountDeletionResolution,
    pub new_default_account_id: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderRepositoryListRequest {
    pub plugin_id: String,
    pub account_id: String,
    pub query: ProviderRepositoryQuery,
    pub cursor: Option<String>,
    pub operation_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderOperationCancelRequest {
    pub plugin_id: String,
    pub account_id: String,
    pub operation_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderLocalRemoteMatchCommandRequest {
    pub plugin_id: String,
    pub instance_id: String,
    pub account_id: String,
    pub operation_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderBindingListRequest {
    pub account_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderBindingSetRequest {
    pub repository_id: String,
    pub remote_name: String,
    pub instance_id: String,
    pub account_id: Option<String>,
    pub provider_repository_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderBindingDeleteRequest {
    pub repository_id: String,
    pub remote_name: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderPermissionPluginRequest {
    pub plugin_id: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderPermissionGrantAccountsRequest {
    pub plugin_id: String,
    pub account_ids: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderPermissionAccountRequest {
    pub plugin_id: String,
    pub account_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderInstanceListResponse {
    pub items: Vec<ProviderInstanceSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderAccountListResponse {
    pub items: Vec<ProviderAccountSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderAuthorizedAccountListResponse {
    pub items: Vec<ProviderAuthorizedAccount>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderBindingSuggestionListResponse {
    pub items: Vec<ProviderBindingSuggestion>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderBindingListResponse {
    pub items: Vec<ProviderBinding>,
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

#[tauri::command]
pub fn git_transport_profile_list(
    state: State<'_, AppState>,
    request: GitTransportPluginRequest,
) -> CommandResult<GitTransportProfileListResponse> {
    ensure_builtin_permission(
        &state.plugins,
        &state.permissions,
        &request.plugin_id,
        "git.transport:read",
        TRANSPORT_PROFILES_RESOURCE,
    )
    .map_err(command_error)?;
    state
        .transport
        .profile_service()
        .list_profiles()
        .map(|items| GitTransportProfileListResponse { items })
        .map_err(command_error)
}

#[tauri::command]
pub fn git_transport_profile_create(
    state: State<'_, AppState>,
    request: GitTransportProfileCreateNativeRequest,
) -> CommandResult<TransportProfileSummary> {
    ensure_builtin_permission(
        &state.plugins,
        &state.permissions,
        request.plugin_id(),
        "git.transport:manage",
        TRANSPORT_PROFILES_RESOURCE,
    )
    .map_err(command_error)?;
    let profiles = state.transport.profile_service();
    match request {
        GitTransportProfileCreateNativeRequest::Ssh {
            display_name,
            ssh_key_path,
            identities_only,
            ..
        } => profiles
            .create_ssh_profile(&display_name, &ssh_key_path, identities_only)
            .map_err(command_error),
        GitTransportProfileCreateNativeRequest::Https {
            display_name,
            username,
            use_http_path,
            ..
        } => {
            require_http_path(use_http_path).map_err(command_error)?;
            profiles
                .create_https_profile(&display_name, &username)
                .map_err(command_error)
        }
    }
}

#[tauri::command]
pub fn git_transport_profile_update(
    state: State<'_, AppState>,
    request: GitTransportProfileUpdateNativeRequest,
) -> CommandResult<TransportProfileSummary> {
    ensure_builtin_permission(
        &state.plugins,
        &state.permissions,
        request.plugin_id(),
        "git.transport:manage",
        TRANSPORT_PROFILES_RESOURCE,
    )
    .map_err(command_error)?;
    let profiles = state.transport.profile_service();
    match request {
        GitTransportProfileUpdateNativeRequest::Ssh {
            profile_id,
            display_name,
            ssh_key_path,
            identities_only,
            ..
        } => profiles
            .update_ssh_profile(
                &profile_id,
                &display_name,
                ssh_key_path.as_deref(),
                identities_only,
            )
            .map_err(command_error),
        GitTransportProfileUpdateNativeRequest::Https {
            profile_id,
            display_name,
            username,
            use_http_path,
            ..
        } => {
            require_http_path(use_http_path).map_err(command_error)?;
            profiles
                .update_https_profile(&profile_id, &display_name, &username)
                .map_err(command_error)
        }
    }
}

#[tauri::command]
pub fn git_transport_profile_deletion_impact(
    state: State<'_, AppState>,
    request: GitTransportProfileNativeRequest,
) -> CommandResult<GitTransportProfileDeletionImpactResponse> {
    ensure_builtin_permission(
        &state.plugins,
        &state.permissions,
        &request.plugin_id,
        "git.transport:read",
        TRANSPORT_PROFILES_RESOURCE,
    )
    .map_err(command_error)?;
    let profiles = state.transport.profile_service();
    let impact = profiles
        .profile_deletion_impact(&request.profile_id)
        .map_err(command_error)?;
    let kind = profiles
        .list_profiles()
        .map_err(command_error)?
        .into_iter()
        .find(|profile| profile.id == request.profile_id)
        .map(|profile| profile.kind)
        .ok_or_else(|| command_error(AppError::NotFound("transport profile".to_owned())))?;
    let repositories = state.git.repository_repository();
    impact
        .repository_ids
        .into_iter()
        .map(|repository_id| {
            repositories.get(&repository_id).map(|repository| {
                GitTransportProfileDeletionRepository {
                    repository_id,
                    display_name: repository.display_name,
                    transport_kind: kind,
                }
            })
        })
        .collect::<Result<Vec<_>, _>>()
        .map(|repositories| GitTransportProfileDeletionImpactResponse {
            profile_id: impact.profile_id,
            repositories,
        })
        .map_err(command_error)
}

#[tauri::command]
pub async fn git_transport_profile_delete(
    state: State<'_, AppState>,
    request: GitTransportProfileDeleteNativeRequest,
) -> CommandResult<()> {
    ensure_builtin_permission(
        &state.plugins,
        &state.permissions,
        &request.plugin_id,
        "git.transport:manage",
        TRANSPORT_PROFILES_RESOURCE,
    )
    .map_err(command_error)?;
    let profiles = state.transport.profile_service();
    run_transport_blocking(move || {
        profiles.delete_profile(&request.profile_id, &request.resolutions)
    })
    .await
    .map_err(command_error)
}

#[tauri::command]
pub async fn git_transport_select_destination_parent(
    app: AppHandle,
    state: State<'_, AppState>,
    request: GitTransportPluginRequest,
) -> CommandResult<Option<String>> {
    ensure_exact_builtin_permission(
        &state.plugins,
        &state.permissions,
        &request.plugin_id,
        GIT_CLIENT_PLUGIN_ID,
        "git.network:execute",
        CLONE_INTENTS_RESOURCE,
    )
    .map_err(command_error)?;
    #[cfg(all(feature = "e2e", debug_assertions))]
    if let Some(destination) = state
        .e2e_transport
        .take_destination_parent()
        .map_err(command_error)?
    {
        return Ok(Some(destination));
    }
    select_host_path(app, true).await.map_err(command_error)
}

#[tauri::command]
pub async fn git_transport_select_ssh_key(
    app: AppHandle,
    state: State<'_, AppState>,
    request: GitTransportPluginRequest,
) -> CommandResult<Option<String>> {
    ensure_exact_builtin_permission(
        &state.plugins,
        &state.permissions,
        &request.plugin_id,
        GIT_CLIENT_PLUGIN_ID,
        "git.transport:manage",
        TRANSPORT_PROFILES_RESOURCE,
    )
    .map_err(command_error)?;
    select_host_path(app, false).await.map_err(command_error)
}

#[tauri::command]
pub async fn git_repository_effective_transport(
    state: State<'_, AppState>,
    request: GitRepositoryTransportRequest,
) -> CommandResult<EffectiveTransport> {
    ensure_builtin_permission(
        &state.plugins,
        &state.permissions,
        &request.plugin_id,
        "git.transport:read",
        TRANSPORT_PROFILES_RESOURCE,
    )
    .map_err(command_error)?;
    ensure_repository_permission(
        &state.plugins,
        &state.permissions,
        &request.plugin_id,
        "repositories:read",
    )
    .map_err(command_error)?;
    let context =
        transport_context(request.project_id, request.workspace_id).map_err(command_error)?;
    let git = state.git.clone();
    let profiles = state.transport.profile_service();
    run_transport_blocking(move || {
        git.validate_repository_context(&context, &request.repository_id)?;
        profiles.effective_for_repository(&request.repository_id)
    })
    .await
    .map_err(command_error)
}

#[tauri::command]
pub async fn git_repository_network_state(
    state: State<'_, AppState>,
    request: GitRepositoryTransportRequest,
) -> CommandResult<RepositoryNetworkState> {
    ensure_repository_permission(
        &state.plugins,
        &state.permissions,
        &request.plugin_id,
        "repositories:read",
    )
    .map_err(command_error)?;
    let context =
        transport_context(request.project_id, request.workspace_id).map_err(command_error)?;
    let transport = state.transport.clone();
    run_transport_blocking(move || transport.network_state(&context, &request.repository_id))
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn git_repository_bind_transport(
    state: State<'_, AppState>,
    request: GitRepositoryBindTransportNativeRequest,
) -> CommandResult<RepositoryTransportBindingSummary> {
    ensure_builtin_permission(
        &state.plugins,
        &state.permissions,
        &request.plugin_id,
        "git.transport:manage",
        TRANSPORT_PROFILES_RESOURCE,
    )
    .map_err(command_error)?;
    ensure_repository_permission(
        &state.plugins,
        &state.permissions,
        &request.plugin_id,
        "repositories:write",
    )
    .map_err(command_error)?;
    let context =
        transport_context(request.project_id, request.workspace_id).map_err(command_error)?;
    let git = state.git.clone();
    let profiles = state.transport.profile_service();
    run_transport_blocking(move || {
        git.validate_repository_context(&context, &request.repository_id)?;
        profiles.bind_repository(
            &request.repository_id,
            &request.transport_profile_id,
            request.replace_existing,
        )
    })
    .await
    .map_err(command_error)
}

#[tauri::command]
pub async fn git_repository_unbind_transport(
    state: State<'_, AppState>,
    request: GitRepositoryUnbindTransportNativeRequest,
) -> CommandResult<()> {
    ensure_builtin_permission(
        &state.plugins,
        &state.permissions,
        &request.plugin_id,
        "git.transport:manage",
        TRANSPORT_PROFILES_RESOURCE,
    )
    .map_err(command_error)?;
    ensure_repository_permission(
        &state.plugins,
        &state.permissions,
        &request.plugin_id,
        "repositories:write",
    )
    .map_err(command_error)?;
    let context =
        transport_context(request.project_id, request.workspace_id).map_err(command_error)?;
    let git = state.git.clone();
    let profiles = state.transport.profile_service();
    run_transport_blocking(move || {
        git.validate_repository_context(&context, &request.repository_id)?;
        profiles.unbind_repository(&request.repository_id, request.drift_resolution)
    })
    .await
    .map_err(command_error)
}

#[tauri::command]
pub async fn git_clone_intent_create(
    state: State<'_, AppState>,
    request: GitCloneIntentCreateNativeRequest,
) -> CommandResult<GitCloneIntentReference> {
    ensure_exact_builtin_permission(
        &state.plugins,
        &state.permissions,
        &request.plugin_id,
        PROVIDER_CENTER_PLUGIN_ID,
        "git.network:execute",
        CLONE_INTENTS_RESOURCE,
    )
    .map_err(command_error)?;
    ensure_provider_account_read(&state.permissions, &request.plugin_id, &request.account_id)
        .map_err(command_error)?;
    state
        .clone_intents
        .create(
            &request.plugin_id,
            &request.account_id,
            &request.repository_id,
        )
        .await
        .map(|intent| GitCloneIntentReference {
            intent_id: intent.id,
        })
        .map_err(command_error)
}

#[tauri::command]
pub fn git_clone_intent_open(
    state: State<'_, AppState>,
    request: GitCloneIntentNativeRequest,
) -> CommandResult<GitCloneIntentReference> {
    ensure_exact_builtin_permission(
        &state.plugins,
        &state.permissions,
        &request.plugin_id,
        PROVIDER_CENTER_PLUGIN_ID,
        "git.network:execute",
        CLONE_INTENTS_RESOURCE,
    )
    .map_err(command_error)?;
    validate_uuid(&request.intent_id, "Clone intent ID").map_err(command_error)?;
    state
        .transport
        .clone_intents()
        .get_for_creator(&request.intent_id, &request.plugin_id)
        .map(|intent| GitCloneIntentReference {
            intent_id: intent.id,
        })
        .map_err(command_error)
}

#[tauri::command]
pub fn git_clone_intent_get(
    state: State<'_, AppState>,
    request: GitCloneIntentNativeRequest,
) -> CommandResult<CloneIntent> {
    ensure_exact_builtin_permission(
        &state.plugins,
        &state.permissions,
        &request.plugin_id,
        GIT_CLIENT_PLUGIN_ID,
        "git.network:execute",
        CLONE_INTENTS_RESOURCE,
    )
    .map_err(command_error)?;
    state
        .transport
        .clone_intents()
        .get(&request.intent_id)
        .map_err(command_error)
}

#[tauri::command]
pub async fn git_repository_clone(
    app: AppHandle,
    state: State<'_, AppState>,
    request: GitCloneNativeRequest,
) -> CommandResult<CloneResult> {
    validate_clone_native_request(&request).map_err(command_error)?;
    ensure_permission(
        &state.plugins,
        &state.permissions,
        &request.plugin_id,
        "git.network:execute",
        CLONE_INTENTS_RESOURCE,
    )
    .map_err(command_error)?;
    let operation_guard = state
        .transport
        .reserve_operation(
            request.operation_id.clone(),
            request.plugin_id.clone(),
            TransportAuthorizationDomain::CloneIntents,
        )
        .map_err(command_error)?;
    let input = clone_input(request);
    let reporter = job_event_reporter(app, state.jobs.clone());
    let transport = state.transport.clone();
    run_transport_blocking(move || {
        transport.clone_repository_reserved(input, reporter, operation_guard)
    })
    .await
    .map_err(command_error)
}

#[tauri::command]
pub async fn git_repository_fetch(
    app: AppHandle,
    state: State<'_, AppState>,
    request: GitRepositoryFetchNativeRequest,
) -> CommandResult<NetworkOperationResult> {
    ensure_network_repository_request(
        &state.plugins,
        &state.permissions,
        &request.plugin_id,
        request.interactive_confirmed,
        &request.operation_id,
    )
    .map_err(command_error)?;
    let operation_guard = state
        .transport
        .reserve_operation(
            request.operation_id.clone(),
            request.plugin_id.clone(),
            TransportAuthorizationDomain::Repositories,
        )
        .map_err(command_error)?;
    let input = FetchInput {
        repository_id: request.repository_id,
        context: transport_context(request.project_id, request.workspace_id)
            .map_err(command_error)?,
        remote_name: request.remote_name,
        operation_id: request.operation_id,
        interactive: true,
    };
    let reporter = job_event_reporter(app, state.jobs.clone());
    let transport = state.transport.clone();
    run_transport_blocking(move || transport.fetch_reserved(input, reporter, operation_guard))
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn git_repository_pull(
    app: AppHandle,
    state: State<'_, AppState>,
    request: GitRepositoryPullNativeRequest,
) -> CommandResult<NetworkOperationResult> {
    ensure_network_repository_request(
        &state.plugins,
        &state.permissions,
        &request.plugin_id,
        request.interactive_confirmed,
        &request.operation_id,
    )
    .map_err(command_error)?;
    let operation_guard = state
        .transport
        .reserve_operation(
            request.operation_id.clone(),
            request.plugin_id.clone(),
            TransportAuthorizationDomain::Repositories,
        )
        .map_err(command_error)?;
    let input = PullInput {
        repository_id: request.repository_id,
        context: transport_context(request.project_id, request.workspace_id)
            .map_err(command_error)?,
        operation_id: request.operation_id,
        interactive: true,
    };
    let reporter = job_event_reporter(app, state.jobs.clone());
    let transport = state.transport.clone();
    run_transport_blocking(move || transport.pull_reserved(input, reporter, operation_guard))
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn git_repository_push(
    app: AppHandle,
    state: State<'_, AppState>,
    request: GitRepositoryPushNativeRequest,
) -> CommandResult<NetworkOperationResult> {
    ensure_network_repository_request(
        &state.plugins,
        &state.permissions,
        &request.plugin_id,
        request.interactive_confirmed,
        &request.operation_id,
    )
    .map_err(command_error)?;
    let operation_guard = state
        .transport
        .reserve_operation(
            request.operation_id.clone(),
            request.plugin_id.clone(),
            TransportAuthorizationDomain::Repositories,
        )
        .map_err(command_error)?;
    let input = PushInput {
        repository_id: request.repository_id,
        context: transport_context(request.project_id, request.workspace_id)
            .map_err(command_error)?,
        target: request.target.map(|target| PushTarget {
            remote_name: target.remote_name,
            branch_name: target.branch_name,
        }),
        operation_id: request.operation_id,
        interactive: true,
    };
    let reporter = job_event_reporter(app, state.jobs.clone());
    let transport = state.transport.clone();
    run_transport_blocking(move || transport.push_reserved(input, reporter, operation_guard))
        .await
        .map_err(command_error)
}

#[tauri::command]
pub fn git_transport_operation_cancel(
    app: AppHandle,
    state: State<'_, AppState>,
    request: GitTransportOperationCancelNativeRequest,
) -> CommandResult<()> {
    validate_operation_id(&request.operation_id).map_err(command_error)?;
    validate_plugin_id(&request.plugin_id).map_err(command_error)?;
    let Some(authorization) = state
        .transport
        .operation_authorization(&request.operation_id)
    else {
        return Ok(());
    };
    if authorization.plugin_id != request.plugin_id {
        return Err(command_error(AppError::PermissionDenied));
    }
    let resource = match authorization.domain {
        TransportAuthorizationDomain::CloneIntents => CLONE_INTENTS_RESOURCE,
        TransportAuthorizationDomain::Repositories => REPOSITORIES_RESOURCE,
    };
    ensure_permission(
        &state.plugins,
        &state.permissions,
        &request.plugin_id,
        "git.network:execute",
        resource,
    )
    .map_err(command_error)?;
    if state
        .transport
        .cancel_owned_operation(
            &request.operation_id,
            &request.plugin_id,
            authorization.domain,
        )
        .map_err(command_error)?
    {
        match state.jobs.request_cancel(&request.operation_id) {
            Ok(job) => {
                let _ = app.emit("job://updated", job);
            }
            Err(AppError::NotFound(_)) => {}
            Err(error) => return Err(command_error(error)),
        }
    }
    Ok(())
}

#[tauri::command]
pub fn provider_instance_list(
    state: State<'_, AppState>,
) -> CommandResult<ProviderInstanceListResponse> {
    state
        .providers
        .list_instances()
        .map(|items| ProviderInstanceListResponse { items })
        .map_err(command_error)
}

#[tauri::command]
pub async fn provider_instance_create(
    state: State<'_, AppState>,
    request: ProviderInstanceCreateCommandRequest,
) -> CommandResult<ProviderInstanceSummary> {
    state
        .providers
        .create_instance(CreateInstanceInput {
            provider_kind: request.provider_kind,
            display_name: request.display_name,
            base_url: request.base_url,
            custom_ca_path: request.custom_ca_path,
        })
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn provider_instance_update(
    state: State<'_, AppState>,
    request: ProviderInstanceUpdateCommandRequest,
) -> CommandResult<ProviderInstanceSummary> {
    state
        .providers
        .update_instance(UpdateInstanceInput {
            instance_id: request.instance_id,
            display_name: request.display_name,
            base_url: request.base_url,
            custom_ca: request.custom_ca,
        })
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn provider_instance_validate(
    state: State<'_, AppState>,
    request: ProviderInstanceRequest,
) -> CommandResult<ProviderInstanceSummary> {
    state
        .providers
        .validate_instance(&request.instance_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
pub fn provider_instance_delete(
    state: State<'_, AppState>,
    request: ProviderInstanceRequest,
) -> CommandResult<()> {
    state
        .providers
        .delete_instance(&request.instance_id)
        .map_err(command_error)
}

#[tauri::command]
pub fn provider_account_list(
    state: State<'_, AppState>,
    request: ProviderAccountListRequest,
) -> CommandResult<ProviderAccountListResponse> {
    state
        .providers
        .list_accounts(&request.instance_id)
        .map(|items| ProviderAccountListResponse { items })
        .map_err(command_error)
}

#[tauri::command]
pub async fn provider_account_connect(
    state: State<'_, AppState>,
    request: ProviderAccountConnectRequest,
) -> CommandResult<ProviderAccountSummary> {
    let ProviderAccountConnectRequest { instance_id, pat } = request;
    let pat = SensitiveString::new(pat);
    state
        .providers
        .connect_account(&instance_id, pat)
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn provider_account_rotate(
    state: State<'_, AppState>,
    request: ProviderAccountRotateRequest,
) -> CommandResult<ProviderAccountSummary> {
    let ProviderAccountRotateRequest { account_id, pat } = request;
    let pat = SensitiveString::new(pat);
    state
        .providers
        .rotate_account(&account_id, pat)
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn provider_account_validate(
    state: State<'_, AppState>,
    request: ProviderAccountRequest,
) -> CommandResult<ProviderAccountSummary> {
    state
        .providers
        .validate_account(&request.account_id)
        .await
        .map_err(command_error)
}

#[tauri::command]
pub fn provider_account_set_default(
    state: State<'_, AppState>,
    request: ProviderAccountSetDefaultRequest,
) -> CommandResult<ProviderAccountSummary> {
    state
        .providers
        .set_default_account(&request.instance_id, &request.account_id)
        .map_err(command_error)
}

#[tauri::command]
pub fn provider_account_deletion_impact(
    state: State<'_, AppState>,
    request: ProviderAccountRequest,
) -> CommandResult<AccountDeletionImpact> {
    state
        .providers
        .account_deletion_impact(&request.account_id)
        .map_err(command_error)
}

#[tauri::command]
pub async fn provider_account_delete(
    state: State<'_, AppState>,
    request: ProviderAccountDeleteRequest,
) -> CommandResult<()> {
    state
        .providers
        .delete_account(DeleteAccountInput {
            account_id: request.account_id,
            resolution: request.resolution,
            new_default_account_id: request.new_default_account_id,
        })
        .await
        .map_err(command_error)
}

#[tauri::command]
pub async fn provider_repository_list(
    state: State<'_, AppState>,
    request: ProviderRepositoryListRequest,
) -> CommandResult<ProviderRepositoryPage> {
    provider_repository_list_core(&state.permissions, &state.providers, request)
        .await
        .map_err(command_error)
}

#[tauri::command]
pub fn provider_operation_cancel(
    state: State<'_, AppState>,
    request: ProviderOperationCancelRequest,
) -> CommandResult<()> {
    ensure_provider_account_read(&state.permissions, &request.plugin_id, &request.account_id)
        .map_err(command_error)?;
    state
        .providers
        .cancel_operation(
            &request.plugin_id,
            &request.account_id,
            &request.operation_id,
        )
        .map_err(command_error)
}

#[tauri::command]
pub async fn provider_local_remote_match(
    state: State<'_, AppState>,
    request: ProviderLocalRemoteMatchCommandRequest,
) -> CommandResult<ProviderBindingSuggestionListResponse> {
    ensure_provider_account_read(&state.permissions, &request.plugin_id, &request.account_id)
        .map_err(command_error)?;
    state
        .providers
        .match_local_remotes(
            &request.plugin_id,
            &request.instance_id,
            &request.account_id,
            &request.operation_id,
        )
        .await
        .map(|items| ProviderBindingSuggestionListResponse { items })
        .map_err(command_error)
}

#[tauri::command]
pub fn provider_binding_list(
    state: State<'_, AppState>,
    request: ProviderBindingListRequest,
) -> CommandResult<ProviderBindingListResponse> {
    state
        .providers
        .list_bindings_for_account(&request.account_id)
        .map(|items| ProviderBindingListResponse { items })
        .map_err(command_error)
}

#[tauri::command]
pub async fn provider_binding_set(
    state: State<'_, AppState>,
    request: ProviderBindingSetRequest,
) -> CommandResult<ProviderBinding> {
    state
        .providers
        .bind_remote(BindRemoteInput {
            repository_id: request.repository_id,
            remote_name: request.remote_name,
            instance_id: request.instance_id,
            account_id: request.account_id,
            provider_repository_id: request.provider_repository_id,
        })
        .await
        .map_err(command_error)
}

#[tauri::command]
pub fn provider_binding_delete(
    state: State<'_, AppState>,
    request: ProviderBindingDeleteRequest,
) -> CommandResult<()> {
    state
        .providers
        .unbind_remote(&request.repository_id, &request.remote_name)
        .map_err(command_error)
}

#[tauri::command]
pub fn provider_permission_is_declared(
    state: State<'_, AppState>,
    request: AuthorizationRequest,
) -> AuthorizationDecision {
    AuthorizationDecision {
        allowed: state.plugins.manifest_requests(
            &request.plugin_id,
            &request.capability,
            &request.resource,
        ),
    }
}

#[tauri::command]
pub fn provider_permission_list_authorized_accounts(
    state: State<'_, AppState>,
    request: ProviderPermissionPluginRequest,
) -> CommandResult<ProviderAuthorizedAccountListResponse> {
    list_authorized_provider_accounts(
        &state.plugins,
        &state.permissions,
        &state.providers,
        &request.plugin_id,
    )
    .map(|items| ProviderAuthorizedAccountListResponse { items })
    .map_err(command_error)
}

#[tauri::command]
pub fn provider_permission_grant_accounts(
    state: State<'_, AppState>,
    request: ProviderPermissionGrantAccountsRequest,
) -> CommandResult<ProviderAuthorizedAccountListResponse> {
    if !state
        .plugins
        .manifest_requests(&request.plugin_id, "providers:read", "providers")
    {
        return Err(command_error(AppError::PermissionDenied));
    }
    if request.account_ids.is_empty() {
        return Err(command_error(AppError::InvalidInput(
            "at least one Provider account is required".to_owned(),
        )));
    }

    let mut resources = Vec::with_capacity(request.account_ids.len());
    let mut items = Vec::with_capacity(request.account_ids.len());
    for account_id in request.account_ids {
        let resource = provider_account_resource(&account_id).map_err(command_error)?;
        if resources.iter().any(|current| current == &resource) {
            continue;
        }
        items.push(
            state
                .providers
                .authorized_account(&account_id)
                .map_err(command_error)?,
        );
        resources.push(resource);
    }
    for resource in resources {
        state
            .permissions
            .grant_dynamic(&request.plugin_id, "providers:read", &resource)
            .map_err(command_error)?;
    }
    Ok(ProviderAuthorizedAccountListResponse { items })
}

#[tauri::command]
pub async fn provider_permission_revoke_account(
    state: State<'_, AppState>,
    request: ProviderPermissionAccountRequest,
) -> CommandResult<()> {
    if !state
        .plugins
        .manifest_requests(&request.plugin_id, "providers:read", "providers")
    {
        return Err(command_error(AppError::PermissionDenied));
    }
    revoke_provider_account_permission(
        &state.permissions,
        &state.providers,
        &request.plugin_id,
        &request.account_id,
    )
    .await
    .map_err(command_error)
}

async fn provider_repository_list_core(
    permissions: &PermissionGateway,
    providers: &ProviderService,
    request: ProviderRepositoryListRequest,
) -> Result<ProviderRepositoryPage, AppError> {
    ensure_provider_account_read(permissions, &request.plugin_id, &request.account_id)?;
    providers
        .list_repositories(
            &request.plugin_id,
            ListRepositoriesInput {
                account_id: request.account_id,
                query: request.query,
                cursor: request.cursor,
                operation_id: request.operation_id,
            },
        )
        .await
}

async fn revoke_provider_account_permission(
    permissions: &PermissionGateway,
    providers: &ProviderService,
    plugin_id: &str,
    account_id: &str,
) -> Result<(), AppError> {
    let resource = provider_account_resource(account_id)?;
    permissions.revoke_dynamic(plugin_id, "providers:read", &resource)?;
    providers
        .cancel_plugin_account_operations(plugin_id, account_id)
        .await
}

fn list_authorized_provider_accounts(
    plugins: &crate::plugins::PluginRegistry,
    permissions: &PermissionGateway,
    providers: &ProviderService,
    plugin_id: &str,
) -> Result<Vec<ProviderAuthorizedAccount>, AppError> {
    if !plugins.manifest_requests(plugin_id, "providers:read", "providers") {
        return Err(AppError::PermissionDenied);
    }
    permissions
        .list_active_resources(plugin_id, "providers:read", "provider-account/")?
        .into_iter()
        .map(|resource| {
            let account_id = resource.strip_prefix("provider-account/").ok_or_else(|| {
                AppError::InvalidInput("Provider account resource is invalid".to_owned())
            })?;
            provider_account_resource(account_id)?;
            providers.authorized_account(account_id)
        })
        .collect()
}

fn ensure_provider_account_read(
    permissions: &PermissionGateway,
    plugin_id: &str,
    account_id: &str,
) -> Result<(), AppError> {
    let resource = provider_account_resource(account_id)?;
    let exact = permissions.is_allowed(plugin_id, "providers:read", &resource)?;
    let family = permissions.is_allowed(plugin_id, "providers:read", "providers")?;
    if exact || family {
        Ok(())
    } else {
        Err(AppError::PermissionDenied)
    }
}

fn provider_account_resource(account_id: &str) -> Result<String, AppError> {
    let parsed = Uuid::parse_str(account_id)
        .map_err(|_| AppError::InvalidInput("Provider account ID is invalid".to_owned()))?;
    if parsed.hyphenated().to_string() != account_id {
        return Err(AppError::InvalidInput(
            "Provider account ID is not canonical".to_owned(),
        ));
    }
    Ok(format!("provider-account/{account_id}"))
}

fn require_http_path(use_http_path: bool) -> Result<(), AppError> {
    if !use_http_path {
        return Err(AppError::InvalidInput(
            "HTTPS transport profiles require credential.useHttpPath".to_owned(),
        ));
    }
    Ok(())
}

fn ensure_exact_builtin_permission(
    plugins: &PluginRegistry,
    permissions: &PermissionGateway,
    plugin_id: &str,
    expected_plugin_id: &str,
    capability: &str,
    resource: &str,
) -> Result<(), AppError> {
    if plugin_id != expected_plugin_id {
        return Err(AppError::PermissionDenied);
    }
    ensure_builtin_permission(plugins, permissions, plugin_id, capability, resource)
}

fn ensure_repository_permission(
    plugins: &PluginRegistry,
    permissions: &PermissionGateway,
    plugin_id: &str,
    capability: &str,
) -> Result<(), AppError> {
    ensure_permission(
        plugins,
        permissions,
        plugin_id,
        capability,
        REPOSITORIES_RESOURCE,
    )
}

fn ensure_network_repository_request(
    plugins: &PluginRegistry,
    permissions: &PermissionGateway,
    plugin_id: &str,
    interactive_confirmed: bool,
    operation_id: &str,
) -> Result<(), AppError> {
    ensure_interactive_network_allowed(plugin_id, interactive_confirmed)?;
    validate_operation_id(operation_id)?;
    ensure_permission(
        plugins,
        permissions,
        plugin_id,
        "git.network:execute",
        REPOSITORIES_RESOURCE,
    )?;
    ensure_repository_permission(plugins, permissions, plugin_id, "repositories:write")
}

fn transport_context(
    project_id: Option<String>,
    workspace_id: Option<String>,
) -> Result<QueryContext, AppError> {
    let context = QueryContext {
        project_id,
        workspace_id,
    };
    context.validate_for_command()?;
    Ok(context)
}

fn clone_input(request: GitCloneNativeRequest) -> CloneInput {
    CloneInput {
        source: match request.source {
            CloneSourceRequest::Intent { intent_id } => CloneSource::Intent(intent_id),
            CloneSourceRequest::Manual { remote_url } => CloneSource::Manual(remote_url),
        },
        transport_kind: request.transport_kind,
        profile_id: request.profile_id,
        destination_parent: request.destination_parent,
        folder_name: request.folder_name,
        project_target: request.project_target,
        operation_id: request.operation_id,
        interactive: true,
    }
}

fn job_event_reporter(
    app: AppHandle,
    jobs: crate::jobs::JobService,
) -> Arc<dyn NetworkProgressReporter> {
    Arc::new(move |progress: NetworkProgress| {
        if let Ok(items) = jobs.list()
            && let Some(job) = items
                .into_iter()
                .find(|job| job.id == progress.operation_id)
        {
            let _ = app.emit("job://updated", job);
        }
    })
}

async fn run_transport_blocking<T, F>(operation: F) -> Result<T, AppError>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, AppError> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(operation)
        .await
        .map_err(|_| AppError::Git("Git transport worker failed".to_owned()))?
}

async fn select_host_path(app: AppHandle, directory: bool) -> Result<Option<String>, AppError> {
    let selected = tauri::async_runtime::spawn_blocking(move || {
        if directory {
            app.dialog().file().blocking_pick_folder()
        } else {
            app.dialog().file().blocking_pick_file()
        }
    })
    .await
    .map_err(|_| AppError::Git("native file picker failed".to_owned()))?;
    selected
        .map(|path| {
            path.into_path()
                .map_err(|_| AppError::InvalidInput("selected path is invalid".to_owned()))?
                .into_os_string()
                .into_string()
                .map_err(|_| AppError::NonUtf8Path)
        })
        .transpose()
}

fn ensure_interactive_network_allowed(
    plugin_id: &str,
    interactive_confirmed: bool,
) -> Result<(), AppError> {
    validate_plugin_id(plugin_id)?;
    if !interactive_confirmed {
        return Err(AppError::PermissionDenied);
    }
    Ok(())
}

fn validate_clone_native_request(request: &GitCloneNativeRequest) -> Result<(), AppError> {
    ensure_interactive_network_allowed(&request.plugin_id, request.interactive_confirmed)?;
    if !request.destination_parent.is_absolute() {
        return Err(AppError::InvalidInput(
            "Clone destination parent must be an absolute Host-selected path".to_owned(),
        ));
    }
    validate_operation_id(&request.operation_id)?;
    if let Some(profile_id) = request.profile_id.as_deref() {
        validate_uuid(profile_id, "transport profile ID")?;
    }
    match &request.source {
        CloneSourceRequest::Intent { intent_id } => {
            validate_uuid(intent_id, "Clone intent ID")?;
            if request.plugin_id != GIT_CLIENT_PLUGIN_ID {
                return Err(AppError::PermissionDenied);
            }
        }
        CloneSourceRequest::Manual { remote_url } => {
            if remote_url.is_empty()
                || remote_url.len() > 4096
                || remote_url.chars().any(char::is_control)
            {
                return Err(AppError::InvalidInput(
                    "Clone Remote URL is invalid".to_owned(),
                ));
            }
        }
    }
    if let CloneProjectTarget::Existing { project_id } = &request.project_target {
        validate_uuid(project_id, "Project ID")?;
    }
    Ok(())
}

fn ensure_builtin_permission(
    plugins: &PluginRegistry,
    permissions: &PermissionGateway,
    plugin_id: &str,
    capability: &str,
    resource: &str,
) -> Result<(), AppError> {
    validate_plugin_id(plugin_id)?;
    let descriptor = plugins.get(plugin_id).ok_or(AppError::PermissionDenied)?;
    if descriptor.manifest.kind != PluginKind::Builtin
        || !plugins.manifest_requests(plugin_id, capability, resource)
        || !permissions.is_allowed(plugin_id, capability, resource)?
    {
        return Err(AppError::PermissionDenied);
    }
    Ok(())
}

fn ensure_permission(
    plugins: &PluginRegistry,
    permissions: &PermissionGateway,
    plugin_id: &str,
    capability: &str,
    resource: &str,
) -> Result<(), AppError> {
    validate_plugin_id(plugin_id)?;
    if plugins.get(plugin_id).is_none()
        || !permissions.is_allowed(plugin_id, capability, resource)?
    {
        return Err(AppError::PermissionDenied);
    }
    Ok(())
}

fn validate_plugin_id(plugin_id: &str) -> Result<(), AppError> {
    if plugin_id.is_empty() || plugin_id.len() > 256 || plugin_id.chars().any(char::is_control) {
        return Err(AppError::PermissionDenied);
    }
    Ok(())
}

fn validate_operation_id(operation_id: &str) -> Result<(), AppError> {
    validate_uuid(operation_id, "transport operation ID")
}

fn validate_uuid(value: &str, label: &str) -> Result<(), AppError> {
    Uuid::parse_str(value)
        .map(|_| ())
        .map_err(|_| AppError::InvalidInput(format!("{label} is invalid")))
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
        CloneSourceRequest, GitCloneNativeRequest, GitIdentityCreateRequest,
        GitProjectCreateRequest, GitProjectDeleteRequest, GitRepositoryCommitRequest,
        GitRepositoryFetchNativeRequest, GitRepositoryIdentityBindRequest,
        GitRepositoryPushNativeRequest, GitTransportProfileCreateNativeRequest,
        GitTransportProfileDeletionImpactResponse, GitTransportProfileDeletionRepository,
        GitWorkspaceRequest, GitWorkspaceUpdateRequest, ProviderAccountConnectRequest,
        ProviderInstanceCreateCommandRequest, ProviderRepositoryListRequest, ThemeActivateRequest,
        ThemeCatalogResponse, ensure_builtin_permission, ensure_exact_builtin_permission,
        ensure_interactive_network_allowed, ensure_permission, provider_account_resource,
        provider_repository_list_core, revoke_provider_account_permission,
        validate_clone_native_request,
    };
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use chrono::Utc;
    use futures_util::future::BoxFuture;
    use tokio::sync::Notify;

    use crate::db::Database;
    use crate::error::{AppError, ErrorEnvelope, ProviderFailure};
    use crate::git::transport::model::{CloneProjectTarget, TransportKind};
    use crate::plugins::permissions::PermissionGateway;
    use crate::plugins::registry::PluginRegistry;
    use crate::providers::adapter::{
        AdapterAccountContext, ProviderAdapterRegistry, RepositoryDiscoveryProvider,
    };
    use crate::providers::model::{
        AccountIdentity, AdapterListRequest, AdapterPage, InstanceMetadata, NewProviderAccount,
        ProviderArchivedFilter, ProviderAuthorizedAccount, ProviderBinding, ProviderInstance,
        ProviderInstanceSummary, ProviderKind, ProviderRepositoryDirection, ProviderRepositoryPage,
        ProviderRepositoryQuery, ProviderRepositorySort, RemoteRepository,
        RemoteRepositoryIdentity,
    };
    use crate::providers::service::ProviderService;
    use crate::providers::store::ProviderStore;
    use crate::providers::url::{NormalizedInstance, NormalizedRemoteUrl};
    use crate::secrets::{MemorySecretStore, SecretStore, SensitiveString};
    use crate::themes::{ThemeDensity, ThemeMetadata};
    use serde::Serialize;
    use serde::de::DeserializeOwned;
    use serde_json::{Value, json};

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
    fn plugin_transport_requests_cannot_enable_interaction_without_host_confirmation() {
        assert!(ensure_interactive_network_allowed("git-ramus.git-client", false).is_err());
        assert!(ensure_interactive_network_allowed("external.example", false).is_err());
        assert!(ensure_interactive_network_allowed("git-ramus.git-client", true).is_ok());
    }

    #[test]
    fn clone_native_request_requires_host_injected_absolute_parent() {
        let request = GitCloneNativeRequest {
            plugin_id: "git-ramus.git-client".to_owned(),
            source: CloneSourceRequest::Manual {
                remote_url: "https://git.example.test/acme/repo.git".to_owned(),
            },
            transport_kind: TransportKind::Https,
            profile_id: None,
            destination_parent: "relative/path".into(),
            folder_name: "repo".to_owned(),
            project_target: CloneProjectTarget::New {
                name: "Repo".to_owned(),
            },
            operation_id: uuid::Uuid::new_v4().to_string(),
            interactive_confirmed: true,
        };
        assert!(validate_clone_native_request(&request).is_err());
    }

    #[test]
    fn transport_native_dtos_are_strict_and_reject_plugin_controlled_execution_fields() {
        let destination = std::env::temp_dir().to_string_lossy().into_owned();
        let clone: GitCloneNativeRequest = serde_json::from_value(json!({
            "pluginId": "git-ramus.git-client",
            "source": {
                "kind": "manual",
                "remoteUrl": "https://git.example.test/acme/repo.git"
            },
            "transportKind": "https",
            "profileId": null,
            "destinationParent": destination,
            "folderName": "repo",
            "projectTarget": { "kind": "new", "name": "Repo" },
            "operationId": uuid::Uuid::new_v4().to_string(),
            "interactiveConfirmed": true
        }))
        .expect("Host-injected Clone request parses");
        assert!(validate_clone_native_request(&clone).is_ok());

        assert!(
            serde_json::from_value::<GitCloneNativeRequest>(json!({
                "pluginId": "git-ramus.git-client",
                "source": {
                    "kind": "manual",
                    "remoteUrl": "https://git.example.test/acme/repo.git"
                },
                "transportKind": "https",
                "profileId": null,
                "folderName": "repo",
                "projectTarget": { "kind": "new", "name": "Repo" },
                "operationId": uuid::Uuid::new_v4().to_string(),
                "interactiveConfirmed": true
            }))
            .is_err()
        );
        let clone_with_secret = json!({
            "pluginId": "git-ramus.git-client",
            "source": { "kind": "intent", "intentId": uuid::Uuid::new_v4().to_string() },
            "transportKind": "https",
            "profileId": null,
            "destinationParent": std::env::temp_dir(),
            "folderName": "repo",
            "projectTarget": { "kind": "new", "name": "Repo" },
            "operationId": uuid::Uuid::new_v4().to_string(),
            "interactiveConfirmed": true,
            "pat": "must-not-cross"
        });
        assert!(serde_json::from_value::<GitCloneNativeRequest>(clone_with_secret).is_err());

        assert!(
            serde_json::from_value::<GitTransportProfileCreateNativeRequest>(json!({
                "pluginId": "git-ramus.git-client",
                "kind": "ssh",
                "displayName": "Work SSH",
                "sshKeyAction": "selectFile",
                "identitiesOnly": true
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<GitRepositoryFetchNativeRequest>(json!({
                "pluginId": "git-ramus.git-client",
                "projectId": uuid::Uuid::new_v4().to_string(),
                "workspaceId": null,
                "repositoryId": uuid::Uuid::new_v4().to_string(),
                "remoteName": "origin",
                "operationId": uuid::Uuid::new_v4().to_string(),
                "interactiveConfirmed": true,
                "args": ["--force"]
            }))
            .is_err()
        );
        assert!(
            serde_json::from_value::<GitRepositoryPushNativeRequest>(json!({
                "pluginId": "git-ramus.git-client",
                "projectId": uuid::Uuid::new_v4().to_string(),
                "workspaceId": null,
                "repositoryId": uuid::Uuid::new_v4().to_string(),
                "target": null,
                "operationId": uuid::Uuid::new_v4().to_string(),
                "interactiveConfirmed": true,
                "refspec": "+refs/heads/*:refs/heads/*"
            }))
            .is_err()
        );
    }

    #[test]
    fn transport_deletion_impact_matches_the_public_repository_summary_contract() {
        let response = GitTransportProfileDeletionImpactResponse {
            profile_id: "c19f947f-afb2-4f87-8b44-0d325dfa60a2".to_owned(),
            repositories: vec![GitTransportProfileDeletionRepository {
                repository_id: "e012ea50-4a67-4bb5-977a-ab3234901adf".to_owned(),
                display_name: "skills".to_owned(),
                transport_kind: TransportKind::Ssh,
            }],
        };
        let value = serde_json::to_value(response).unwrap();
        assert_eq!(
            value,
            json!({
                "profileId": "c19f947f-afb2-4f87-8b44-0d325dfa60a2",
                "repositories": [{
                    "repositoryId": "e012ea50-4a67-4bb5-977a-ab3234901adf",
                    "displayName": "skills",
                    "transportKind": "ssh"
                }]
            })
        );
        assert!(value.get("repositoryIds").is_none());
    }

    #[test]
    fn transport_profile_boundaries_require_a_declared_and_granted_builtin_caller() {
        let directory = tempfile::tempdir().expect("temporary plugin root creates");
        write_transport_permission_plugin(directory.path(), "git-ramus.transport-ui", "builtin");
        write_transport_permission_plugin(directory.path(), "example.transport-ui", "external");
        let registry = PluginRegistry::discover(directory.path()).expect("plugins discover");
        let database = Database::open_in_memory().expect("database opens");
        let now = Utc::now().to_rfc3339();
        database
            .with_connection(|connection| {
                for (plugin_id, kind) in [
                    ("git-ramus.transport-ui", "builtin"),
                    ("example.transport-ui", "external"),
                ] {
                    connection.execute(
                        "INSERT INTO plugin_installations(plugin_id,version,kind,root_path,installed_at,updated_at) VALUES(?1,'0.1.0',?2,?1,?3,?3)",
                        rusqlite::params![plugin_id, kind, now],
                    )?;
                }
                Ok(())
            })
            .unwrap();
        let permissions = PermissionGateway::new(database);
        permissions
            .grant_manifest_permissions(&registry.get("git-ramus.transport-ui").unwrap().manifest)
            .unwrap();
        permissions
            .grant_dynamic(
                "example.transport-ui",
                "git.transport:manage",
                "transport-profiles",
            )
            .unwrap();

        assert!(
            ensure_builtin_permission(
                &registry,
                &permissions,
                "git-ramus.transport-ui",
                "git.transport:manage",
                "transport-profiles",
            )
            .is_ok()
        );
        assert!(
            ensure_builtin_permission(
                &registry,
                &permissions,
                "example.transport-ui",
                "git.transport:manage",
                "transport-profiles",
            )
            .is_err()
        );
        assert!(
            ensure_permission(
                &registry,
                &permissions,
                "example.transport-ui",
                "git.transport:manage",
                "transport-profiles",
            )
            .is_ok()
        );
        assert!(
            ensure_exact_builtin_permission(
                &registry,
                &permissions,
                "git-ramus.transport-ui",
                "git-ramus.git-client",
                "git.transport:manage",
                "transport-profiles",
            )
            .is_err()
        );
    }

    fn write_transport_permission_plugin(root: &std::path::Path, id: &str, kind: &str) {
        let plugin = root.join(id);
        std::fs::create_dir_all(&plugin).unwrap();
        std::fs::write(
            plugin.join("plugin.json"),
            format!(
                r#"{{"schemaVersion":1,"id":"{id}","name":"Transport UI","version":"0.1.0","publisher":"test","description":"Transport UI","kind":"{kind}","sdkVersion":"^0.1.0","entrypoints":{{"ui":"ui.html"}},"contributions":{{"navigation":[]}},"permissions":[{{"capability":"git.transport:manage","resources":["transport-profiles"]}}]}}"#
            ),
        )
        .unwrap();
        std::fs::write(plugin.join("ui.html"), "<h1>Transport</h1>").unwrap();
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

    #[test]
    fn provider_secret_and_instance_requests_are_strict_host_only_boundaries() {
        let account: ProviderAccountConnectRequest = serde_json::from_value(json!({
            "instanceId": "6da75ccf-f7df-4bf2-92b7-2c158765726f",
            "pat": "glpat-never-serialize"
        }))
        .expect("secret request parses");
        assert_eq!(account.instance_id, "6da75ccf-f7df-4bf2-92b7-2c158765726f");
        let instance: ProviderInstanceCreateCommandRequest = serde_json::from_value(json!({
            "providerKind": "gitlab",
            "displayName": "GitLab",
            "baseUrl": "https://gitlab.example",
            "customCaPath": null
        }))
        .expect("instance request parses");
        assert!(instance.custom_ca_path.is_none());
        assert!(
            serde_json::from_value::<ProviderInstanceCreateCommandRequest>(json!({
                "providerKind": "gitlab",
                "displayName": "GitLab",
                "baseUrl": "https://gitlab.example",
                "customCaPath": null,
                "pat": "must-not-cross"
            }))
            .is_err()
        );
    }

    #[test]
    fn provider_secret_requests_and_failures_never_expose_the_pat() {
        let request: ProviderAccountConnectRequest = serde_json::from_value(json!({
            "instanceId": "6da75ccf-f7df-4bf2-92b7-2c158765726f",
            "pat": "glpat-never-serialize"
        }))
        .expect("secret request parses");
        let ProviderAccountConnectRequest { pat, .. } = request;
        let secret = SensitiveString::new(pat);
        let error = ErrorEnvelope::from(AppError::Provider(ProviderFailure::authentication()));
        let json = serde_json::to_string(&error).expect("error serializes");
        let diagnostics = format!("{secret:?} {error:?}");

        assert!(!json.contains("glpat-never-serialize"));
        assert!(!diagnostics.contains("glpat-never-serialize"));
        assert!(diagnostics.contains("[REDACTED]"));
    }

    #[test]
    fn provider_canonical_contract_fixture_round_trips_exactly() {
        let fixture: Value = serde_json::from_str(include_str!(
            "../../../../packages/contracts/src/__fixtures__/provider-contracts.json"
        ))
        .expect("canonical fixture parses");

        assert_round_trip::<ProviderInstanceSummary>(&fixture["instance"]);
        assert_round_trip::<ProviderAuthorizedAccount>(&fixture["authorizedAccount"]);
        assert_round_trip::<ProviderRepositoryPage>(&fixture["repositoryPage"]);
        assert_round_trip::<ProviderBinding>(&fixture["binding"]);
        assert_round_trip::<ErrorEnvelope>(&fixture["error"]);

        let account_json = serde_json::to_string(&fixture["authorizedAccount"])
            .expect("authorized account serializes");
        assert!(!account_json.contains("secretRef"));
        assert!(!account_json.contains("customCaPath"));
    }

    fn assert_round_trip<T>(value: &Value)
    where
        T: DeserializeOwned + Serialize,
    {
        let parsed: T = serde_json::from_value(value.clone()).expect("fixture DTO parses");
        assert_eq!(
            serde_json::to_value(parsed).expect("fixture DTO serializes"),
            *value
        );
    }

    struct BlockingCommandProvider {
        calls: AtomicUsize,
        started: Notify,
    }

    impl BlockingCommandProvider {
        fn new() -> Self {
            Self {
                calls: AtomicUsize::new(0),
                started: Notify::new(),
            }
        }
    }

    impl RepositoryDiscoveryProvider for BlockingCommandProvider {
        fn kind(&self) -> ProviderKind {
            ProviderKind::Gitlab
        }

        fn validate_instance<'a>(
            &'a self,
            _client: &'a crate::providers::http::ScopedHttpClient,
        ) -> BoxFuture<'a, Result<InstanceMetadata, AppError>> {
            Box::pin(async {
                Ok(InstanceMetadata {
                    server_version: None,
                })
            })
        }

        fn authenticate_account<'a>(
            &'a self,
            _client: &'a crate::providers::http::ScopedHttpClient,
            _secret: &'a str,
        ) -> BoxFuture<'a, Result<AccountIdentity, AppError>> {
            Box::pin(async {
                Ok(AccountIdentity {
                    provider_user_id: "user-1".to_owned(),
                    username: "creator".to_owned(),
                    display_name: None,
                    avatar_url: None,
                })
            })
        }

        fn list_repositories<'a>(
            &'a self,
            context: AdapterAccountContext<'a>,
            _request: AdapterListRequest,
        ) -> BoxFuture<'a, Result<AdapterPage, AppError>> {
            Box::pin(async move {
                self.calls.fetch_add(1, Ordering::SeqCst);
                self.started.notify_one();
                context.cancellation.cancelled().await;
                Err(AppError::Provider(ProviderFailure::canceled()))
            })
        }

        fn get_repository<'a>(
            &'a self,
            _context: AdapterAccountContext<'a>,
            _identity: RemoteRepositoryIdentity,
        ) -> BoxFuture<'a, Result<RemoteRepository, AppError>> {
            Box::pin(async { Err(AppError::Provider(ProviderFailure::invalid_response())) })
        }

        fn detect_remote(
            &self,
            _instance: &NormalizedInstance,
            _remote: &NormalizedRemoteUrl,
        ) -> Option<RemoteRepositoryIdentity> {
            None
        }
    }

    #[tokio::test]
    async fn provider_revocation_cancels_in_flight_work_and_denies_before_the_next_adapter_call() {
        const PLUGIN_ID: &str = "example.reader";
        const INSTANCE_ID: &str = "6da75ccf-f7df-4bf2-92b7-2c158765726f";
        const ACCOUNT_ID: &str = "7f3c0214-373c-4d43-b0c7-cdaed1cbcc50";
        const SECRET_REF: &str = "provider/account/test";

        let database = Database::open_in_memory().expect("database opens");
        database
            .with_connection(|connection| {
                let now = Utc::now().to_rfc3339();
                connection.execute(
                    "INSERT INTO plugin_installations(plugin_id,version,kind,root_path,enabled,installed_at,updated_at) VALUES(?1,'0.1.0','external','/external/reader',1,?3,?3),('git-ramus.provider.gitlab','0.1.0','builtin','/builtin/gitlab',1,?3,?3)",
                    rusqlite::params![PLUGIN_ID, "unused", now],
                )?;
                Ok(())
            })
            .expect("plugin installations seed");
        let store = ProviderStore::new(database.clone());
        let now = Utc::now();
        store
            .insert_instance(ProviderInstance {
                id: INSTANCE_ID.to_owned(),
                provider_kind: ProviderKind::Gitlab,
                display_name: "GitLab Example".to_owned(),
                base_url: "https://gitlab.example".to_owned(),
                api_base_url: "https://gitlab.example/api/v4".to_owned(),
                custom_ca_path: None,
                last_validated_at: Some(now),
                server_version: None,
                created_at: now,
                updated_at: now,
            })
            .expect("instance seeds");
        store
            .insert_account(NewProviderAccount {
                id: ACCOUNT_ID.to_owned(),
                instance_id: INSTANCE_ID.to_owned(),
                provider_user_id: "user-1".to_owned(),
                username: "creator".to_owned(),
                display_name: None,
                avatar_url: None,
                secret_ref: SECRET_REF.to_owned(),
                last_validated_at: now,
                created_at: now,
                updated_at: now,
            })
            .expect("account seeds");
        let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::default());
        secrets
            .set(SECRET_REF, "glpat-test-only")
            .expect("secret seeds");
        let adapter = Arc::new(BlockingCommandProvider::new());
        let registry = ProviderAdapterRegistry::for_test(
            database.clone(),
            ProviderKind::Gitlab,
            adapter.clone(),
        );
        let providers = Arc::new(ProviderService::new(store, secrets, registry));
        let permissions = PermissionGateway::new(database);
        let resource = provider_account_resource(ACCOUNT_ID).expect("resource is canonical");
        permissions
            .grant_dynamic(PLUGIN_ID, "providers:read", &resource)
            .expect("account grant succeeds");

        let request = provider_list_request(PLUGIN_ID, ACCOUNT_ID, "operation-1");
        let task_permissions = permissions.clone();
        let task_providers = Arc::clone(&providers);
        let started = adapter.started.notified();
        let request_task = tokio::spawn(async move {
            provider_repository_list_core(&task_permissions, &task_providers, request).await
        });
        tokio::time::timeout(std::time::Duration::from_secs(2), started)
            .await
            .expect("adapter request starts");

        revoke_provider_account_permission(&permissions, &providers, PLUGIN_ID, ACCOUNT_ID)
            .await
            .expect("revocation cancels and drains work");
        let canceled = request_task.await.expect("request task joins");
        assert!(matches!(
            canceled,
            Err(AppError::Provider(ref failure))
                if failure.code() == "provider.request-canceled"
        ));

        let denied = provider_repository_list_core(
            &permissions,
            &providers,
            provider_list_request(PLUGIN_ID, ACCOUNT_ID, "operation-2"),
        )
        .await;
        assert!(matches!(denied, Err(AppError::PermissionDenied)));
        assert_eq!(adapter.calls.load(Ordering::SeqCst), 1);
    }

    fn provider_list_request(
        plugin_id: &str,
        account_id: &str,
        operation_id: &str,
    ) -> ProviderRepositoryListRequest {
        ProviderRepositoryListRequest {
            plugin_id: plugin_id.to_owned(),
            account_id: account_id.to_owned(),
            query: ProviderRepositoryQuery {
                search: String::new(),
                visibility: None,
                namespace: None,
                archived: ProviderArchivedFilter::All,
                sort: ProviderRepositorySort::Name,
                direction: ProviderRepositoryDirection::Asc,
                page_size: 30,
            },
            cursor: None,
            operation_id: operation_id.to_owned(),
        }
    }
}
