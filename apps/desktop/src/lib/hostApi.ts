import {
  changesResultSchema,
  diffResultSchema,
  effectiveIdentitySchema,
  gitContextRequestSchema,
  identityBindingSchema,
  identityCreateRequestSchema,
  identityListResponseSchema,
  identityProfileRequestSchema,
  identityProfileSchema,
  identityUpdateRequestSchema,
  overviewSchema,
  projectListResponseSchema,
  projectScanRequestSchema,
  projectSchema,
  projectUpdateScanRulesRequestSchema,
  repositoryCommitRequestSchema,
  repositoryDiffRequestSchema,
  repositoryIdentityBindRequestSchema,
  repositoryIdentityRequestSchema,
  repositoryRequestSchema,
  repositoryScanRecordSchema,
  repositoryStageRequestSchema,
  repositoryUnstageRequestSchema,
  scanProjectResultSchema,
  trustResponseSchema,
  trustStatusResponseSchema,
  themeActivateRequestSchema,
  themeCatalogSchema,
  themeStateSchema,
  workspaceCreateRequestSchema,
  workspaceDeleteRequestSchema,
  workspaceListResponseSchema,
  workspaceMembershipResponseSchema,
  workspaceRequestSchema,
  workspaceSchema,
  workspaceUpdateMembershipRequestSchema,
  writeResultSchema
} from "@git-ramus/contracts";
import type {
  ChangesResult,
  DiffResult,
  EffectiveIdentity,
  GitContextRequest,
  IdentityBinding,
  IdentityCreateRequest,
  IdentityListResponse,
  IdentityProfile,
  IdentityProfileRequest,
  IdentityUpdateRequest,
  Job,
  Overview,
  PluginDescriptor,
  Project,
  ProjectListResponse,
  ProjectScanRequest,
  ProjectUpdateScanRulesRequest,
  RepositoryCommitRequest,
  RepositoryDiffRequest,
  RepositoryIdentityBindRequest,
  RepositoryIdentityRequest,
  RepositoryRequest,
  RepositoryScanRecord,
  RepositoryStageRequest,
  RepositoryUnstageRequest,
  ScanProjectResult,
  TrustResponse,
  TrustStatusResponse,
  ThemeActivateRequest,
  ThemeCatalog,
  ThemeState,
  Workspace,
  WorkspaceCreateRequest,
  WorkspaceDeleteRequest,
  WorkspaceListResponse,
  WorkspaceRequest,
  WorkspaceUpdateMembershipRequest,
  WriteResult
} from "@git-ramus/contracts";
import { invoke } from "@tauri-apps/api/core";

export interface AppInfo {
  name: string;
  version: string;
}

export interface AuthorizationRequest {
  pluginId: string;
  capability: string;
  resource: string;
}

export interface AuthorizationDecision {
  allowed: boolean;
}

export interface HostApi {
  getAppInfo(): Promise<AppInfo>;
  listPlugins(): Promise<PluginDescriptor[]>;
  listJobs(): Promise<Job[]>;
  listThemes(): Promise<ThemeCatalog>;
  currentTheme(): Promise<ThemeState>;
  activateTheme(request: ThemeActivateRequest): Promise<ThemeState>;
  authorizePluginCall(request: AuthorizationRequest): Promise<AuthorizationDecision>;
  startEchoJob(pluginId: string, message: string): Promise<Job>;
  cancelJob(jobId: string): Promise<void>;
  listProjects(): Promise<ProjectListResponse>;
  updateProjectScanRules(request: ProjectUpdateScanRulesRequest): Promise<Project>;
  scanProject(request: ProjectScanRequest): Promise<ScanProjectResult>;
  listWorkspaces(): Promise<WorkspaceListResponse>;
  createWorkspace(request: WorkspaceCreateRequest): Promise<Workspace>;
  getWorkspaceMembership(request: WorkspaceRequest): Promise<string[]>;
  updateWorkspaceMembership(request: WorkspaceUpdateMembershipRequest): Promise<string[]>;
  deleteWorkspace(request: WorkspaceDeleteRequest): Promise<void>;
  getOverview(request: GitContextRequest): Promise<Overview>;
  getRepositorySnapshot(request: RepositoryRequest): Promise<RepositoryScanRecord>;
  getRepositoryChanges(request: RepositoryRequest): Promise<ChangesResult>;
  getRepositoryDiff(request: RepositoryDiffRequest): Promise<DiffResult>;
  getRepositoryTrustStatus(request: RepositoryRequest): Promise<TrustStatusResponse>;
  stageRepository(request: RepositoryStageRequest): Promise<WriteResult>;
  unstageRepository(request: RepositoryUnstageRequest): Promise<WriteResult>;
  commitRepository(request: RepositoryCommitRequest): Promise<WriteResult>;
  trustRepository(request: RepositoryRequest): Promise<TrustResponse>;
  listIdentities(): Promise<IdentityListResponse>;
  createIdentity(request: IdentityCreateRequest): Promise<IdentityProfile>;
  updateIdentity(request: IdentityUpdateRequest): Promise<IdentityProfile>;
  deleteIdentity(request: IdentityProfileRequest): Promise<void>;
  setGlobalIdentity(request: IdentityProfileRequest): Promise<IdentityProfile>;
  bindRepositoryIdentity(request: RepositoryIdentityBindRequest): Promise<IdentityBinding>;
  unbindRepositoryIdentity(request: RepositoryIdentityRequest): Promise<void>;
  getEffectiveRepositoryIdentity(request: RepositoryIdentityRequest): Promise<EffectiveIdentity>;
}

export const tauriHostApi: HostApi = {
  getAppInfo: () => invoke<AppInfo>("get_app_info"),
  listPlugins: () => invoke<PluginDescriptor[]>("list_plugins"),
  listJobs: () => invoke<Job[]>("list_jobs"),
  listThemes: () => invokeParsed("list_themes", themeCatalogSchema),
  currentTheme: () => invokeParsed("current_theme", themeStateSchema),
  activateTheme: (request) =>
    invokeRequest("activate_theme", themeActivateRequestSchema, themeStateSchema, request),
  authorizePluginCall: (request) =>
    invoke<AuthorizationDecision>("authorize_plugin_call", { request }),
  startEchoJob: (pluginId, message) =>
    invoke<Job>("start_echo_job", { request: { pluginId, message } }),
  cancelJob: (jobId) => invoke<void>("cancel_job", { jobId }),
  listProjects: () => invokeParsed("git_project_list", projectListResponseSchema),
  updateProjectScanRules: (request) =>
    invokeRequest(
      "git_project_update_scan_rules",
      projectUpdateScanRulesRequestSchema,
      projectSchema,
      request
    ),
  scanProject: (request) =>
    invokeRequest("git_project_scan", projectScanRequestSchema, scanProjectResultSchema, request),
  listWorkspaces: () => invokeParsed("git_workspace_list", workspaceListResponseSchema),
  createWorkspace: (request) =>
    invokeRequest("git_workspace_create", workspaceCreateRequestSchema, workspaceSchema, request),
  getWorkspaceMembership: (request) =>
    invokeRequest(
      "git_workspace_get_membership",
      workspaceRequestSchema,
      workspaceMembershipResponseSchema,
      request
    ),
  updateWorkspaceMembership: (request) =>
    invokeRequest(
      "git_workspace_update_membership",
      workspaceUpdateMembershipRequestSchema,
      workspaceMembershipResponseSchema,
      request
    ),
  deleteWorkspace: (request) =>
    invokeVoidRequest("git_workspace_delete", workspaceDeleteRequestSchema, request),
  getOverview: (request) =>
    invokeRequest("git_overview_get", gitContextRequestSchema, overviewSchema, request),
  getRepositorySnapshot: (request) =>
    invokeRequest(
      "git_repository_snapshot",
      repositoryRequestSchema,
      repositoryScanRecordSchema,
      request
    ),
  getRepositoryChanges: (request) =>
    invokeRequest("git_repository_changes", repositoryRequestSchema, changesResultSchema, request),
  getRepositoryDiff: (request) =>
    invokeRequest("git_repository_diff", repositoryDiffRequestSchema, diffResultSchema, request),
  getRepositoryTrustStatus: (request) =>
    invokeRequest(
      "git_repository_trust_status",
      repositoryRequestSchema,
      trustStatusResponseSchema,
      request
    ),
  stageRepository: (request) =>
    invokeRequest("git_repository_stage", repositoryStageRequestSchema, writeResultSchema, request),
  unstageRepository: (request) =>
    invokeRequest(
      "git_repository_unstage",
      repositoryUnstageRequestSchema,
      writeResultSchema,
      request
    ),
  commitRepository: (request) =>
    invokeRequest(
      "git_repository_commit",
      repositoryCommitRequestSchema,
      writeResultSchema,
      request
    ),
  trustRepository: (request) =>
    invokeRequest("git_repository_trust", repositoryRequestSchema, trustResponseSchema, request),
  listIdentities: () => invokeParsed("git_identity_list", identityListResponseSchema),
  createIdentity: (request) =>
    invokeRequest(
      "git_identity_create",
      identityCreateRequestSchema,
      identityProfileSchema,
      request
    ),
  updateIdentity: (request) =>
    invokeRequest(
      "git_identity_update",
      identityUpdateRequestSchema,
      identityProfileSchema,
      request
    ),
  deleteIdentity: (request) =>
    invokeVoidRequest("git_identity_delete", identityProfileRequestSchema, request),
  setGlobalIdentity: (request) =>
    invokeRequest(
      "git_identity_set_global",
      identityProfileRequestSchema,
      identityProfileSchema,
      request
    ),
  bindRepositoryIdentity: (request) =>
    invokeRequest(
      "git_repository_bind_identity",
      repositoryIdentityBindRequestSchema,
      identityBindingSchema,
      request
    ),
  unbindRepositoryIdentity: (request) =>
    invokeVoidRequest("git_repository_unbind_identity", repositoryIdentityRequestSchema, request),
  getEffectiveRepositoryIdentity: (request) =>
    invokeRequest(
      "git_repository_effective_identity",
      repositoryIdentityRequestSchema,
      effectiveIdentitySchema,
      request
    )
};

interface RuntimeSchema<T> {
  parse(value: unknown): T;
}

async function invokeParsed<T>(command: string, responseSchema: RuntimeSchema<T>): Promise<T> {
  return responseSchema.parse(await invoke<unknown>(command));
}

async function invokeRequest<TRequest, TResponse>(
  command: string,
  requestSchema: RuntimeSchema<TRequest>,
  responseSchema: RuntimeSchema<TResponse>,
  request: TRequest
): Promise<TResponse> {
  const parsedRequest = requestSchema.parse(request);
  return responseSchema.parse(await invoke<unknown>(command, { request: parsedRequest }));
}

async function invokeVoidRequest<TRequest>(
  command: string,
  requestSchema: RuntimeSchema<TRequest>,
  request: TRequest
): Promise<void> {
  const parsedRequest = requestSchema.parse(request);
  await invoke<unknown>(command, { request: parsedRequest });
}
