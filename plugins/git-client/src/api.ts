import {
  errorEnvelopeSchema,
  type ChangesResult,
  type DiffResult,
  type EffectiveIdentity,
  type ErrorEnvelope,
  type GitContextRequest,
  type IdentityBinding,
  type IdentityCreateRequest,
  type IdentityListResponse,
  type IdentityProfile,
  type IdentityProfileRequest,
  type IdentityUpdateRequest,
  type Overview,
  type Project,
  type ProjectListResponse,
  type ProjectScanRequest,
  type ProjectUpdateScanRulesRequest,
  type RepositoryCommitRequest,
  type RepositoryDiffRequest,
  type RepositoryIdentityBindRequest,
  type RepositoryIdentityRequest,
  type RepositoryRequest,
  type RepositoryScanRecord,
  type RepositoryStageRequest,
  type RepositoryUnstageRequest,
  type ScanProjectResult,
  type TrustResponse,
  type TrustStatusResponse,
  type Workspace,
  type WorkspaceCreateRequest,
  type WorkspaceDeleteRequest,
  type WorkspaceListResponse,
  type WorkspaceRequest,
  type WorkspaceUpdateMembershipRequest,
  type WriteResult
} from "@git-ramus/contracts";
import type { PluginClient } from "@git-ramus/plugin-sdk";

export interface GitClientApi {
  listProjects(): Promise<ProjectListResponse>;
  createProject(): Promise<Project | null>;
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
  trustRepository(request: RepositoryIdentityRequest): Promise<TrustResponse>;
  listIdentities(): Promise<IdentityListResponse>;
  createIdentity(request: IdentityCreateRequest): Promise<IdentityProfile>;
  updateIdentity(request: IdentityUpdateRequest): Promise<IdentityProfile>;
  deleteIdentity(request: IdentityProfileRequest): Promise<void>;
  setGlobalIdentity(request: IdentityProfileRequest): Promise<IdentityProfile>;
  bindRepositoryIdentity(request: RepositoryIdentityBindRequest): Promise<IdentityBinding>;
  unbindRepositoryIdentity(request: RepositoryIdentityRequest): Promise<void>;
  getEffectiveRepositoryIdentity(request: RepositoryIdentityRequest): Promise<EffectiveIdentity>;
}

export function createGitClientApi(client: PluginClient): GitClientApi {
  return {
    listProjects: () => client.request("projects.list", {}),
    createProject: () => client.request("projects.create", {}),
    updateProjectScanRules: (request) => client.request("projects.updateScanRules", request),
    scanProject: (request) => client.request("projects.scan", request),
    listWorkspaces: () => client.request("workspaces.list", {}),
    createWorkspace: (request) => client.request("workspaces.create", request),
    getWorkspaceMembership: (request) => client.request("workspaces.getMembership", request),
    updateWorkspaceMembership: (request) => client.request("workspaces.updateMembership", request),
    deleteWorkspace: (request) => client.request("workspaces.delete", request),
    getOverview: (request) => client.request("overview.get", request),
    getRepositorySnapshot: (request) => client.request("repositories.getSnapshot", request),
    getRepositoryChanges: (request) => client.request("repositories.getChanges", request),
    getRepositoryDiff: (request) => client.request("repositories.getDiff", request),
    getRepositoryTrustStatus: (request) => client.request("repositories.getTrustStatus", request),
    stageRepository: (request) => client.request("repositories.stage", request),
    unstageRepository: (request) => client.request("repositories.unstage", request),
    commitRepository: (request) => client.request("repositories.commit", request),
    trustRepository: (request) => client.request("repositories.trust", request),
    listIdentities: () => client.request("identities.list", {}),
    createIdentity: (request) => client.request("identities.create", request),
    updateIdentity: (request) => client.request("identities.update", request),
    deleteIdentity: (request) => client.request("identities.delete", request),
    setGlobalIdentity: (request) => client.request("identities.setGlobal", request),
    bindRepositoryIdentity: (request) => client.request("repositories.bindIdentity", request),
    unbindRepositoryIdentity: (request) => client.request("repositories.unbindIdentity", request),
    getEffectiveRepositoryIdentity: (request) =>
      client.request("repositories.getEffectiveIdentity", request)
  };
}

export function normalizeError(error: unknown, fallbackMessage: string): ErrorEnvelope {
  const parsed = errorEnvelopeSchema.safeParse(error);
  if (parsed.success) {
    return parsed.data;
  }
  return {
    code: "plugin.unexpected-error",
    category: "retryable",
    message: fallbackMessage,
    operationId: null,
    pluginId: "git-ramus.git-client",
    resourceId: null,
    failedStep: null,
    retryable: true,
    retryAfterMs: null,
    recoveryActions: [{ id: "retry", label: "Try again", kind: "retry" }],
    details: null
  };
}
