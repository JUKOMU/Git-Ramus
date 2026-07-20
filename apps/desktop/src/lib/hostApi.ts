import {
  changesResultSchema,
  authorizationDecisionSchema,
  cloneIntentReferenceSchema,
  cloneIntentRequestSchema,
  cloneIntentSummarySchema,
  cloneRequestSchema,
  cloneResultSchema,
  diffResultSchema,
  effectiveTransportSchema,
  effectiveIdentitySchema,
  gitContextRequestSchema,
  identityBindingSchema,
  identityCreateRequestSchema,
  identityListResponseSchema,
  identityProfileRequestSchema,
  identityProfileSchema,
  identityUpdateRequestSchema,
  overviewSchema,
  networkOperationResultSchema,
  providerAccountConnectRequestSchema,
  providerAccountDeleteRequestSchema,
  providerAccountDeletionImpactRequestSchema,
  providerAccountDeletionImpactSchema,
  providerAccountListRequestSchema,
  providerAccountListResponseSchema,
  providerAccountRotateRequestSchema,
  providerAccountSetDefaultRequestSchema,
  providerAccountSummarySchema,
  providerAccountValidateRequestSchema,
  providerAuthorizedAccountListResponseSchema,
  providerBindingDeleteRequestSchema,
  providerBindingListRequestSchema,
  providerBindingListResponseSchema,
  providerBindingSchema,
  providerBindingSetRequestSchema,
  providerBindingSuggestionListResponseSchema,
  providerInstanceCreateRequestSchema,
  providerInstanceListResponseSchema,
  providerInstanceRequestSchema,
  providerInstanceSchema,
  providerInstanceUpdateRequestSchema,
  providerLocalRemoteMatchRequestSchema,
  providerOperationCancelRequestSchema,
  providerReadAccessRevokeRequestSchema,
  providerCloneIntentCreateRequestSchema,
  providerRepositoryListRequestSchema,
  providerRepositoryPageSchema,
  projectListResponseSchema,
  projectScanRequestSchema,
  projectSchema,
  projectUpdateScanRulesRequestSchema,
  repositoryCommitRequestSchema,
  repositoryDiffRequestSchema,
  repositoryIdentityBindRequestSchema,
  repositoryIdentityRequestSchema,
  repositoryFetchRequestSchema,
  repositoryNetworkStateSchema,
  repositoryPullRequestSchema,
  repositoryPushRequestSchema,
  repositoryRequestSchema,
  repositoryScanRecordSchema,
  repositoryStageRequestSchema,
  repositoryUnstageRequestSchema,
  repositoryTransportBindRequestSchema,
  repositoryTransportBindingSchema,
  repositoryTransportUnbindRequestSchema,
  scanProjectResultSchema,
  trustResponseSchema,
  trustStatusResponseSchema,
  themeActivateRequestSchema,
  themeCatalogSchema,
  themeStateSchema,
  transportOperationCancelRequestSchema,
  transportProfileCreateRequestSchema,
  transportProfileDeleteRequestSchema,
  transportProfileDeletionImpactSchema,
  transportProfileListResponseSchema,
  transportProfileRequestSchema,
  transportProfileSummarySchema,
  transportProfileUpdateRequestSchema,
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
  CloneIntentReference,
  CloneIntentRequest,
  CloneIntentSummary,
  CloneRequest,
  CloneResult,
  DiffResult,
  EffectiveIdentity,
  EffectiveTransport,
  GitContextRequest,
  IdentityBinding,
  IdentityCreateRequest,
  IdentityListResponse,
  IdentityProfile,
  IdentityProfileRequest,
  IdentityUpdateRequest,
  Job,
  NetworkOperationResult,
  Overview,
  PluginDescriptor,
  Project,
  ProjectListResponse,
  ProjectScanRequest,
  ProjectUpdateScanRulesRequest,
  ProviderAccountDeleteRequest,
  ProviderAccountDeletionImpact,
  ProviderAccountDeletionImpactRequest,
  ProviderAccountConnectRequest,
  ProviderAccountListRequest,
  ProviderAccountListResponse,
  ProviderAccountRotateRequest,
  ProviderAccountSetDefaultRequest,
  ProviderAccountSummary,
  ProviderAccountValidateRequest,
  ProviderAuthorizedAccount,
  ProviderAuthorizedAccountListResponse,
  ProviderBinding,
  ProviderBindingDeleteRequest,
  ProviderBindingListRequest,
  ProviderBindingListResponse,
  ProviderBindingSetRequest,
  ProviderBindingSuggestion,
  ProviderCloneIntentCreateRequest,
  ProviderInstance,
  ProviderInstanceCreateRequest,
  ProviderInstanceListResponse,
  ProviderInstanceRequest,
  ProviderInstanceUpdateRequest,
  ProviderLocalRemoteMatchRequest,
  ProviderOperationCancelRequest,
  ProviderReadAccessRevokeRequest,
  ProviderRepositoryListRequest,
  ProviderRepositoryPage,
  RepositoryCommitRequest,
  RepositoryDiffRequest,
  RepositoryIdentityBindRequest,
  RepositoryIdentityRequest,
  RepositoryFetchRequest,
  RepositoryNetworkState,
  RepositoryPullRequest,
  RepositoryPushRequest,
  RepositoryRequest,
  RepositoryScanRecord,
  RepositoryStageRequest,
  RepositoryUnstageRequest,
  RepositoryTransportBindRequest,
  RepositoryTransportBinding,
  RepositoryTransportUnbindRequest,
  ScanProjectResult,
  TrustResponse,
  TrustStatusResponse,
  ThemeActivateRequest,
  ThemeCatalog,
  ThemeState,
  TransportOperationCancelRequest,
  TransportProfileCreateRequest,
  TransportProfileDeleteRequest,
  TransportProfileDeletionImpact,
  TransportProfileListResponse,
  TransportProfileRequest,
  TransportProfileSummary,
  TransportProfileUpdateRequest,
  Workspace,
  WorkspaceCreateRequest,
  WorkspaceDeleteRequest,
  WorkspaceListResponse,
  WorkspaceRequest,
  WorkspaceUpdateMembershipRequest,
  WriteResult
} from "@git-ramus/contracts";
import { invoke } from "@tauri-apps/api/core";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  cloneNavigationBroker,
  type CloneNavigationPublisher
} from "../git-transport/cloneNavigationBroker";
import { transportPromptBroker } from "../git-transport/promptBroker";
import type { GitTransportFilePort, GitTransportPromptPort } from "../git-transport/promptPorts";
import {
  nativeGitTransportFilePort,
  unavailableGitTransportPromptPort
} from "../git-transport/promptPorts";
import { providerPromptBrokerPort } from "../providers/promptBroker";
import type { HostFileSelectionPort, ProviderPromptPort } from "../providers/promptPorts";
import { nativeCertificateFileSelectionPort } from "../providers/promptPorts";

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
  authorizePluginPermissionRequest(request: AuthorizationRequest): Promise<AuthorizationDecision>;
  startEchoJob(pluginId: string, message: string): Promise<Job>;
  cancelJob(jobId: string): Promise<void>;
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
  trustRepository(request: RepositoryRequest): Promise<TrustResponse>;
  listIdentities(): Promise<IdentityListResponse>;
  createIdentity(request: IdentityCreateRequest): Promise<IdentityProfile>;
  updateIdentity(request: IdentityUpdateRequest): Promise<IdentityProfile>;
  deleteIdentity(request: IdentityProfileRequest): Promise<void>;
  setGlobalIdentity(request: IdentityProfileRequest): Promise<IdentityProfile>;
  bindRepositoryIdentity(request: RepositoryIdentityBindRequest): Promise<IdentityBinding>;
  unbindRepositoryIdentity(request: RepositoryIdentityRequest): Promise<void>;
  getEffectiveRepositoryIdentity(request: RepositoryIdentityRequest): Promise<EffectiveIdentity>;
  listTransportProfiles(pluginId: string): Promise<TransportProfileListResponse>;
  createTransportProfile(
    pluginId: string,
    request: TransportProfileCreateRequest
  ): Promise<TransportProfileSummary | null>;
  updateTransportProfile(
    pluginId: string,
    request: TransportProfileUpdateRequest
  ): Promise<TransportProfileSummary | null>;
  getTransportProfileDeletionImpact(
    pluginId: string,
    request: TransportProfileRequest
  ): Promise<TransportProfileDeletionImpact>;
  deleteTransportProfile(pluginId: string, request: TransportProfileDeleteRequest): Promise<void>;
  getEffectiveRepositoryTransport(
    pluginId: string,
    request: RepositoryRequest
  ): Promise<EffectiveTransport>;
  getRepositoryNetworkState(
    pluginId: string,
    request: RepositoryRequest
  ): Promise<RepositoryNetworkState>;
  bindRepositoryTransport(
    pluginId: string,
    request: RepositoryTransportBindRequest
  ): Promise<RepositoryTransportBinding | null>;
  unbindRepositoryTransport(
    pluginId: string,
    request: RepositoryTransportUnbindRequest
  ): Promise<void>;
  createCloneIntent(
    pluginId: string,
    request: ProviderCloneIntentCreateRequest
  ): Promise<CloneIntentReference>;
  getCloneIntent(pluginId: string, request: CloneIntentRequest): Promise<CloneIntentSummary>;
  cloneRepository(pluginId: string, request: CloneRequest): Promise<CloneResult | null>;
  fetchRepository(
    pluginId: string,
    request: RepositoryFetchRequest
  ): Promise<NetworkOperationResult | null>;
  pullRepository(
    pluginId: string,
    request: RepositoryPullRequest
  ): Promise<NetworkOperationResult | null>;
  pushRepository(
    pluginId: string,
    request: RepositoryPushRequest
  ): Promise<NetworkOperationResult | null>;
  cancelTransportOperation(
    pluginId: string,
    request: TransportOperationCancelRequest
  ): Promise<void>;
  listProviderInstances(): Promise<ProviderInstanceListResponse>;
  createProviderInstance(request: ProviderInstanceCreateRequest): Promise<ProviderInstance | null>;
  updateProviderInstance(request: ProviderInstanceUpdateRequest): Promise<ProviderInstance | null>;
  validateProviderInstance(request: ProviderInstanceRequest): Promise<ProviderInstance>;
  deleteProviderInstance(request: ProviderInstanceRequest): Promise<void>;
  listProviderAccounts(request: ProviderAccountListRequest): Promise<ProviderAccountListResponse>;
  connectProviderAccount(
    pluginId: string,
    request: ProviderAccountConnectRequest
  ): Promise<ProviderAccountSummary | null>;
  rotateProviderAccount(
    pluginId: string,
    request: ProviderAccountRotateRequest
  ): Promise<ProviderAccountSummary | null>;
  validateProviderAccount(request: ProviderAccountValidateRequest): Promise<ProviderAccountSummary>;
  setDefaultProviderAccount(
    request: ProviderAccountSetDefaultRequest
  ): Promise<ProviderAccountSummary>;
  getProviderAccountDeletionImpact(
    request: ProviderAccountDeletionImpactRequest
  ): Promise<ProviderAccountDeletionImpact>;
  deleteProviderAccount(request: ProviderAccountDeleteRequest): Promise<void>;
  listAuthorizedProviderAccounts(pluginId: string): Promise<ProviderAuthorizedAccountListResponse>;
  requestProviderReadAccess(
    pluginId: string
  ): Promise<ProviderAuthorizedAccountListResponse | null>;
  revokeProviderReadAccess(
    pluginId: string,
    request: ProviderReadAccessRevokeRequest
  ): Promise<void>;
  listProviderRepositories(
    pluginId: string,
    request: ProviderRepositoryListRequest
  ): Promise<ProviderRepositoryPage>;
  cancelProviderOperation(pluginId: string, request: ProviderOperationCancelRequest): Promise<void>;
  matchLocalProviderRemotes(
    pluginId: string,
    request: ProviderLocalRemoteMatchRequest
  ): Promise<{ items: ProviderBindingSuggestion[] }>;
  listProviderBindings(request: ProviderBindingListRequest): Promise<ProviderBindingListResponse>;
  bindProviderRemote(request: ProviderBindingSetRequest): Promise<ProviderBinding>;
  unbindProviderRemote(request: ProviderBindingDeleteRequest): Promise<void>;
}

export function createTauriHostApi(dependencies: {
  prompts: ProviderPromptPort;
  files: HostFileSelectionPort;
  transportPrompts?: GitTransportPromptPort;
  transportFiles?: GitTransportFilePort;
  cloneNavigation?: CloneNavigationPublisher;
}): HostApi {
  const { prompts, files } = dependencies;
  const transportPrompts = dependencies.transportPrompts ?? unavailableGitTransportPromptPort;
  const transportFiles = dependencies.transportFiles ?? nativeGitTransportFilePort;
  const cloneNavigation = dependencies.cloneNavigation ?? cloneNavigationBroker;
  const instanceCache = new Map<string, ProviderInstance>();
  const accountCache = new Map<string, ProviderAccountSummary>();
  const cloneIntentCache = new Map<string, CloneIntentSummary>();
  const networkStateCache = new Map<string, RepositoryNetworkState>();
  return {
    getAppInfo: () => invoke<AppInfo>("get_app_info"),
    listPlugins: () => invoke<PluginDescriptor[]>("list_plugins"),
    listJobs: () => invoke<Job[]>("list_jobs"),
    listThemes: () => invokeParsed("list_themes", themeCatalogSchema),
    currentTheme: () => invokeParsed("current_theme", themeStateSchema),
    activateTheme: (request) =>
      invokeRequest("activate_theme", themeActivateRequestSchema, themeStateSchema, request),
    authorizePluginCall: (request) =>
      invoke<unknown>("authorize_plugin_call", { request }).then((value) =>
        authorizationDecisionSchema.parse(value)
      ),
    authorizePluginPermissionRequest: (request) =>
      invoke<unknown>("provider_permission_is_declared", { request }).then((value) =>
        authorizationDecisionSchema.parse(value)
      ),
    startEchoJob: (pluginId, message) =>
      invoke<Job>("start_echo_job", { request: { pluginId, message } }),
    cancelJob: (jobId) => invoke<void>("cancel_job", { jobId }),
    listProjects: () => invokeParsed("git_project_list", projectListResponseSchema),
    createProject: async () => {
      const rootPath = await openDialog({
        directory: true,
        multiple: false,
        title: "Choose project root"
      });
      if (rootPath === null) return null;
      if (Array.isArray(rootPath) || rootPath.length === 0) {
        throw new Error("Native directory selection returned an invalid path");
      }
      const name = projectNameFromRoot(rootPath);
      return projectSchema.parse(
        await invoke<unknown>("git_project_create", {
          request: { rootPath, name, scanDepth: null, excludePatterns: [] }
        })
      );
    },
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
      invokeRequest(
        "git_repository_changes",
        repositoryRequestSchema,
        changesResultSchema,
        request
      ),
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
      invokeRequest(
        "git_repository_stage",
        repositoryStageRequestSchema,
        writeResultSchema,
        request
      ),
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
      ),
    listTransportProfiles: (pluginId) =>
      invoke<unknown>("git_transport_profile_list", {
        request: { pluginId }
      }).then((value) => transportProfileListResponseSchema.parse(value)),
    createTransportProfile: async (pluginId, request) => {
      const parsed = transportProfileCreateRequestSchema.parse(request);
      if (parsed.kind === "ssh") {
        const selected = await transportFiles.selectSshPrivateKey();
        if (selected === null) return null;
        const sshKeyPath = validateSelectedHostPath(selected);
        return transportProfileSummarySchema.parse(
          await invoke<unknown>("git_transport_profile_create", {
            request: {
              pluginId,
              kind: "ssh",
              displayName: parsed.displayName,
              sshKeyPath,
              identitiesOnly: parsed.identitiesOnly
            }
          })
        );
      }
      return transportProfileSummarySchema.parse(
        await invoke<unknown>("git_transport_profile_create", {
          request: {
            pluginId,
            kind: "https",
            displayName: parsed.displayName,
            username: parsed.username,
            useHttpPath: parsed.useHttpPath
          }
        })
      );
    },
    updateTransportProfile: async (pluginId, request) => {
      const parsed = transportProfileUpdateRequestSchema.parse(request);
      if (parsed.kind === "ssh") {
        let sshKeyPath: string | null = null;
        if (parsed.sshKeyAction === "selectFile") {
          const selected = await transportFiles.selectSshPrivateKey();
          if (selected === null) return null;
          sshKeyPath = validateSelectedHostPath(selected);
        }
        return transportProfileSummarySchema.parse(
          await invoke<unknown>("git_transport_profile_update", {
            request: {
              pluginId,
              kind: "ssh",
              profileId: parsed.profileId,
              displayName: parsed.displayName,
              sshKeyPath,
              identitiesOnly: parsed.identitiesOnly
            }
          })
        );
      }
      return transportProfileSummarySchema.parse(
        await invoke<unknown>("git_transport_profile_update", {
          request: {
            pluginId,
            kind: "https",
            profileId: parsed.profileId,
            displayName: parsed.displayName,
            username: parsed.username,
            useHttpPath: parsed.useHttpPath
          }
        })
      );
    },
    getTransportProfileDeletionImpact: (pluginId, request) => {
      const parsed = transportProfileRequestSchema.parse(request);
      return invoke<unknown>("git_transport_profile_deletion_impact", {
        request: { pluginId, ...parsed }
      }).then((value) => transportProfileDeletionImpactSchema.parse(value));
    },
    deleteTransportProfile: async (pluginId, request) => {
      const parsed = transportProfileDeleteRequestSchema.parse(request);
      await invoke<unknown>("git_transport_profile_delete", {
        request: { pluginId, ...parsed }
      });
    },
    getEffectiveRepositoryTransport: (pluginId, request) => {
      const parsed = repositoryRequestSchema.parse(request);
      return invoke<unknown>("git_repository_effective_transport", {
        request: { pluginId, ...parsed }
      }).then((value) => effectiveTransportSchema.parse(value));
    },
    getRepositoryNetworkState: (pluginId, request) => {
      const parsed = repositoryRequestSchema.parse(request);
      return invoke<unknown>("git_repository_network_state", {
        request: { pluginId, ...parsed }
      }).then((value) => {
        const state = repositoryNetworkStateSchema.parse(value);
        networkStateCache.set(state.repositoryId, state);
        return state;
      });
    },
    bindRepositoryTransport: async (pluginId, request) => {
      const parsed = repositoryTransportBindRequestSchema.parse(request);
      if (
        parsed.replaceExisting &&
        !(await transportPrompts.confirm({
          pluginId,
          operationId: null,
          kind: "replaceConfig",
          operation: "bindProfile",
          resourceLabel: `repository ${parsed.repositoryId}`
        }))
      ) {
        return null;
      }
      return repositoryTransportBindingSchema.parse(
        await invoke<unknown>("git_repository_bind_transport", {
          request: { pluginId, ...parsed }
        })
      );
    },
    unbindRepositoryTransport: async (pluginId, request) => {
      const parsed = repositoryTransportUnbindRequestSchema.parse(request);
      await invoke<unknown>("git_repository_unbind_transport", {
        request: { pluginId, ...parsed }
      });
    },
    createCloneIntent: async (pluginId, request) => {
      const parsed = providerCloneIntentCreateRequestSchema.parse(request);
      const reference = cloneIntentReferenceSchema.parse(
        await invoke<unknown>("git_clone_intent_create", {
          request: { pluginId, ...parsed }
        })
      );
      cloneNavigation.publish(`/clone/${reference.intentId}`);
      return reference;
    },
    getCloneIntent: (pluginId, request) => {
      const parsed = cloneIntentRequestSchema.parse(request);
      return invoke<unknown>("git_clone_intent_get", {
        request: { pluginId, ...parsed }
      }).then((value) => {
        const intent = cloneIntentSummarySchema.parse(value);
        cloneIntentCache.set(intent.id, intent);
        return intent;
      });
    },
    cloneRepository: async (pluginId, request) => {
      const parsed = cloneRequestSchema.parse(request);
      if (transportPrompts.isOperationCanceled(pluginId, parsed.operationId)) return null;
      const sourceLabel =
        parsed.source.kind === "manual"
          ? safeManualRemoteSummary(parsed.source.remoteUrl)
          : (cloneIntentCache.get(parsed.source.intentId)?.repository.fullName ??
            `intent ${parsed.source.intentId}`);
      const sourceApproved = await transportPrompts.confirm({
        pluginId,
        operationId: parsed.operationId,
        kind: "sourceTrust",
        operation: "clone",
        resourceLabel: sourceLabel
      });
      if (!sourceApproved || transportPrompts.isOperationCanceled(pluginId, parsed.operationId)) {
        return null;
      }
      const networkApproved = await transportPrompts.confirm({
        pluginId,
        operationId: parsed.operationId,
        kind: "network",
        operation: "clone",
        resourceLabel: `${parsed.folderName} · ${cloneProjectTargetLabel(parsed.projectTarget)}`
      });
      if (!networkApproved || transportPrompts.isOperationCanceled(pluginId, parsed.operationId)) {
        return null;
      }
      const selected = await transportFiles.selectDestinationParent();
      if (selected === null || transportPrompts.isOperationCanceled(pluginId, parsed.operationId)) {
        return null;
      }
      const destinationParent = validateSelectedHostPath(selected);
      return cloneResultSchema.parse(
        await invoke<unknown>("git_repository_clone", {
          request: {
            pluginId,
            ...parsed,
            destinationParent,
            interactiveConfirmed: true
          }
        })
      );
    },
    fetchRepository: async (pluginId, request) => {
      const parsed = repositoryFetchRequestSchema.parse(request);
      if (transportPrompts.isOperationCanceled(pluginId, parsed.operationId)) return null;
      const confirmed = await transportPrompts.confirm({
        pluginId,
        operationId: parsed.operationId,
        kind: "network",
        operation: "fetch",
        resourceLabel: repositoryNetworkLabel(
          networkStateCache.get(parsed.repositoryId),
          parsed.repositoryId,
          parsed.remoteName,
          false
        )
      });
      if (!confirmed || transportPrompts.isOperationCanceled(pluginId, parsed.operationId)) {
        return null;
      }
      return networkOperationResultSchema.parse(
        await invoke<unknown>("git_repository_fetch", {
          request: { pluginId, ...parsed, interactiveConfirmed: true }
        })
      );
    },
    pullRepository: async (pluginId, request) => {
      const parsed = repositoryPullRequestSchema.parse(request);
      if (transportPrompts.isOperationCanceled(pluginId, parsed.operationId)) return null;
      const confirmed = await transportPrompts.confirm({
        pluginId,
        operationId: parsed.operationId,
        kind: "network",
        operation: "pull",
        resourceLabel: repositoryNetworkLabel(
          networkStateCache.get(parsed.repositoryId),
          parsed.repositoryId,
          null,
          false
        )
      });
      if (!confirmed || transportPrompts.isOperationCanceled(pluginId, parsed.operationId)) {
        return null;
      }
      return networkOperationResultSchema.parse(
        await invoke<unknown>("git_repository_pull", {
          request: { pluginId, ...parsed, interactiveConfirmed: true }
        })
      );
    },
    pushRepository: async (pluginId, request) => {
      const parsed = repositoryPushRequestSchema.parse(request);
      if (transportPrompts.isOperationCanceled(pluginId, parsed.operationId)) return null;
      const confirmed = await transportPrompts.confirm({
        pluginId,
        operationId: parsed.operationId,
        kind: "network",
        operation: "push",
        resourceLabel: repositoryNetworkLabel(
          networkStateCache.get(parsed.repositoryId),
          parsed.repositoryId,
          parsed.target?.remoteName ?? null,
          true,
          parsed.target?.branchName ?? null
        )
      });
      if (!confirmed || transportPrompts.isOperationCanceled(pluginId, parsed.operationId)) {
        return null;
      }
      return networkOperationResultSchema.parse(
        await invoke<unknown>("git_repository_push", {
          request: { pluginId, ...parsed, interactiveConfirmed: true }
        })
      );
    },
    cancelTransportOperation: async (pluginId, request) => {
      const parsed = transportOperationCancelRequestSchema.parse(request);
      transportPrompts.cancelOperation(pluginId, parsed.operationId);
      await invoke<unknown>("git_transport_operation_cancel", {
        request: { pluginId, ...parsed }
      });
    },
    listProviderInstances: () =>
      invokeParsed("provider_instance_list", providerInstanceListResponseSchema).then(
        (response) => {
          for (const instance of response.items) instanceCache.set(instance.id, instance);
          return response;
        }
      ),
    createProviderInstance: async (request) => {
      const parsed = providerInstanceCreateRequestSchema.parse(request);
      let customCaPath: string | null = null;
      if (parsed.customCaAction === "selectFile") {
        customCaPath = await files.selectCertificate();
        if (customCaPath === null) return null;
      }
      const instance = providerInstanceSchema.parse(
        await invoke<unknown>("provider_instance_create", {
          request: {
            providerKind: parsed.providerKind,
            displayName: parsed.displayName,
            baseUrl: parsed.baseUrl,
            customCaPath
          }
        })
      );
      instanceCache.set(instance.id, instance);
      return instance;
    },
    updateProviderInstance: async (request) => {
      const parsed = providerInstanceUpdateRequestSchema.parse(request);
      let customCa: { kind: "keep" | "remove" } | { kind: "replace"; path: string };
      if (parsed.customCaAction === "selectFile") {
        let current = instanceCache.get(parsed.instanceId);
        if (current === undefined) {
          const response = await invokeParsed(
            "provider_instance_list",
            providerInstanceListResponseSchema
          );
          for (const instance of response.items) instanceCache.set(instance.id, instance);
          current = instanceCache.get(parsed.instanceId);
        }
        if (current === undefined) throw new Error("Provider instance is unavailable");
        if (current.providerKind === "github") {
          throw new Error("GitHub does not support custom CA files");
        }
        const path = await files.selectCertificate();
        if (path === null) return null;
        customCa = { kind: "replace", path };
      } else {
        customCa = { kind: parsed.customCaAction };
      }
      const instance = providerInstanceSchema.parse(
        await invoke<unknown>("provider_instance_update", {
          request: {
            instanceId: parsed.instanceId,
            displayName: parsed.displayName,
            baseUrl: parsed.baseUrl,
            customCa
          }
        })
      );
      instanceCache.set(instance.id, instance);
      return instance;
    },
    validateProviderInstance: (request) =>
      invokeRequest(
        "provider_instance_validate",
        providerInstanceRequestSchema,
        providerInstanceSchema,
        request
      ).then((instance) => {
        instanceCache.set(instance.id, instance);
        return instance;
      }),
    deleteProviderInstance: (request) =>
      invokeVoidRequest("provider_instance_delete", providerInstanceRequestSchema, request),
    listProviderAccounts: (request) =>
      invokeRequest(
        "provider_account_list",
        providerAccountListRequestSchema,
        providerAccountListResponseSchema,
        request
      ).then((response) => {
        for (const account of response.items) accountCache.set(account.id, account);
        return response;
      }),
    connectProviderAccount: async (_pluginId, request) => {
      const parsed = providerAccountConnectRequestSchema.parse(request);
      let pat = await prompts.requestCredential({
        providerLabel: instanceCache.get(parsed.instanceId)?.displayName ?? "Provider",
        accountLabel: null,
        purpose: "connect"
      });
      if (pat === null) return null;
      try {
        const account = providerAccountSummarySchema.parse(
          await invoke<unknown>("provider_account_connect", {
            request: { instanceId: parsed.instanceId, pat }
          })
        );
        accountCache.set(account.id, account);
        return account;
      } finally {
        pat = "";
        void pat;
      }
    },
    rotateProviderAccount: async (_pluginId, request) => {
      const parsed = providerAccountRotateRequestSchema.parse(request);
      const accountContext = accountCache.get(parsed.accountId);
      let pat = await prompts.requestCredential({
        providerLabel:
          instanceCache.get(accountContext?.instanceId ?? "")?.displayName ?? "Provider",
        accountLabel: accountContext?.displayName ?? accountContext?.username ?? null,
        purpose: "rotate"
      });
      if (pat === null) return null;
      try {
        const account = providerAccountSummarySchema.parse(
          await invoke<unknown>("provider_account_rotate", {
            request: { accountId: parsed.accountId, pat }
          })
        );
        accountCache.set(account.id, account);
        return account;
      } finally {
        pat = "";
        void pat;
      }
    },
    validateProviderAccount: (request) =>
      invokeRequest(
        "provider_account_validate",
        providerAccountValidateRequestSchema,
        providerAccountSummarySchema,
        request
      ),
    setDefaultProviderAccount: (request) =>
      invokeRequest(
        "provider_account_set_default",
        providerAccountSetDefaultRequestSchema,
        providerAccountSummarySchema,
        request
      ),
    getProviderAccountDeletionImpact: (request) =>
      invokeRequest(
        "provider_account_deletion_impact",
        providerAccountDeletionImpactRequestSchema,
        providerAccountDeletionImpactSchema,
        request
      ),
    deleteProviderAccount: (request) =>
      invokeVoidRequest("provider_account_delete", providerAccountDeleteRequestSchema, request),
    listAuthorizedProviderAccounts: (pluginId) =>
      invoke<unknown>("provider_permission_list_authorized_accounts", {
        request: { pluginId }
      }).then((value) => providerAuthorizedAccountListResponseSchema.parse(value)),
    requestProviderReadAccess: async (pluginId) => {
      const instances = providerInstanceListResponseSchema.parse(
        await invoke<unknown>("provider_instance_list")
      );
      const accounts = (
        await Promise.all(
          instances.items.map(async (instance) => {
            const response = providerAccountListResponseSchema.parse(
              await invoke<unknown>("provider_account_list", {
                request: { instanceId: instance.id }
              })
            );
            return response.items.map((account) => ({ instance, account }));
          })
        )
      ).flat();
      const candidateIds = new Set(accounts.map(({ account }) => account.id));
      const promptAccounts = deepFreeze(
        accounts.map(({ instance, account }) => ({
          instance: { ...instance },
          account: { ...account }
        }))
      ) as unknown as ProviderAuthorizedAccount[];
      const selected = await prompts.requestAccountAccess({
        pluginId,
        accounts: promptAccounts
      });
      if (selected === null) return null;
      const accountIds = [...new Set(selected)];
      if (accountIds.length === 0 || accountIds.some((accountId) => !candidateIds.has(accountId))) {
        throw new Error("Provider access prompt returned an invalid account selection");
      }
      const granted = providerAuthorizedAccountListResponseSchema.parse(
        await invoke<unknown>("provider_permission_grant_accounts", {
          request: { pluginId, accountIds }
        })
      );
      const grantedIds = granted.items.map(({ account }) => account.id).sort();
      if (
        grantedIds.length !== accountIds.length ||
        grantedIds.join("\u0000") !== [...accountIds].sort().join("\u0000")
      ) {
        throw new Error("Provider access grant response did not match the selection");
      }
      return granted;
    },
    revokeProviderReadAccess: (pluginId, request) => {
      const parsed = providerReadAccessRevokeRequestSchema.parse(request);
      return invoke<void>("provider_permission_revoke_account", {
        request: { pluginId, accountId: parsed.accountId }
      });
    },
    listProviderRepositories: (pluginId, request) => {
      const parsed = providerRepositoryListRequestSchema.parse(request);
      return invoke<unknown>("provider_repository_list", {
        request: { pluginId, ...parsed }
      }).then((value) => providerRepositoryPageSchema.parse(value));
    },
    cancelProviderOperation: (pluginId, request) => {
      const parsed = providerOperationCancelRequestSchema.parse(request);
      return invoke<void>("provider_operation_cancel", {
        request: { pluginId, ...parsed }
      });
    },
    matchLocalProviderRemotes: (pluginId, request) => {
      const parsed = providerLocalRemoteMatchRequestSchema.parse(request);
      return invoke<unknown>("provider_local_remote_match", {
        request: { pluginId, ...parsed }
      }).then((value) => providerBindingSuggestionListResponseSchema.parse(value));
    },
    listProviderBindings: (request) =>
      invokeRequest(
        "provider_binding_list",
        providerBindingListRequestSchema,
        providerBindingListResponseSchema,
        request
      ),
    bindProviderRemote: (request) =>
      invokeRequest(
        "provider_binding_set",
        providerBindingSetRequestSchema,
        providerBindingSchema,
        request
      ),
    unbindProviderRemote: (request) =>
      invokeVoidRequest("provider_binding_delete", providerBindingDeleteRequestSchema, request)
  };
}

export const tauriHostApi: HostApi = createTauriHostApi({
  prompts: providerPromptBrokerPort,
  files: nativeCertificateFileSelectionPort,
  transportPrompts: transportPromptBroker,
  transportFiles: nativeGitTransportFilePort,
  cloneNavigation: cloneNavigationBroker
});

interface RuntimeSchema<T> {
  parse(value: unknown): T;
}

async function invokeParsed<T>(command: string, responseSchema: RuntimeSchema<T>): Promise<T> {
  return responseSchema.parse(await invoke<unknown>(command));
}

function deepFreeze<T>(value: T): T {
  if (value === null || typeof value !== "object" || Object.isFrozen(value)) return value;
  Object.freeze(value);
  for (const child of Object.values(value as Record<string, unknown>)) deepFreeze(child);
  return value;
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

function projectNameFromRoot(rootPath: string): string {
  const withoutTrailingSeparators = rootPath.replace(/[\\/]+$/u, "");
  const segments = withoutTrailingSeparators.split(/[\\/]/u);
  return segments.at(-1) || withoutTrailingSeparators || "Root";
}

function validateSelectedHostPath(value: string): string {
  const path = value;
  if (
    path !== path.trim() ||
    path.length === 0 ||
    path.length > 32_768 ||
    Array.from(path).some((character) => {
      const code = character.charCodeAt(0);
      return code < 0x20 || code === 0x7f;
    }) ||
    !/^(?:[A-Za-z]:[\\/]|\\\\|\/)/u.test(path)
  ) {
    throw new Error("Trusted Git transport picker returned an invalid path");
  }
  return path;
}

function safeManualRemoteSummary(value: string): string {
  if (
    value.length === 0 ||
    value.length > 4096 ||
    isGitRemoteHelper(value) ||
    Array.from(value).some((character) => {
      const code = character.charCodeAt(0);
      return code < 0x20 || code === 0x7f;
    })
  ) {
    throw new Error("Manual Clone source is an unsafe Git remote");
  }

  if (value.includes("://")) {
    let remote: URL;
    try {
      remote = new URL(value);
    } catch {
      throw new Error("Manual Clone source is an unsafe Git remote");
    }
    const https = remote.protocol === "https:";
    const ssh = remote.protocol === "ssh:";
    const username = remote.username;
    const path = normalizeSafeRepositoryPath(remote.pathname);
    if (
      (!https && !ssh) ||
      remote.hostname.length === 0 ||
      remote.password.length > 0 ||
      (https && remote.username.length > 0) ||
      (ssh && !isSafeSshUsername(username)) ||
      value.includes("?") ||
      value.includes("#")
    ) {
      throw new Error("Manual Clone source is an unsafe Git remote");
    }
    const port = canonicalGitPort(remote.protocol, remote.port);
    const authority = gitRemoteAuthority(remote.hostname.toLowerCase(), port);
    return https
      ? `https://${authority}/${path}.git`
      : `ssh://${username}@${authority}/${path}.git`;
  }

  const at = value.indexOf("@");
  if (at <= 0 || value.indexOf("@", at + 1) !== -1) {
    throw new Error("Manual Clone source is an unsafe Git remote");
  }
  const username = value.slice(0, at);
  const hostAndPath = value.slice(at + 1);
  const separator = hostAndPath.indexOf(":");
  if (separator <= 0 || !isSafeSshUsername(username)) {
    throw new Error("Manual Clone source is an unsafe Git remote");
  }
  const rawHost = hostAndPath.slice(0, separator);
  const rawPath = hostAndPath.slice(separator + 1);
  if (
    rawHost.includes("/") ||
    rawHost.includes("\\") ||
    rawHost.includes(":") ||
    rawPath.startsWith("/") ||
    rawPath.startsWith("\\")
  ) {
    throw new Error("Manual Clone source is an unsafe Git remote");
  }
  let parsedHost: URL;
  try {
    parsedHost = new URL(`ssh://${rawHost}/`);
  } catch {
    throw new Error("Manual Clone source is an unsafe Git remote");
  }
  if (
    parsedHost.hostname.length === 0 ||
    parsedHost.username.length > 0 ||
    parsedHost.password.length > 0 ||
    parsedHost.port.length > 0 ||
    parsedHost.pathname !== "/" ||
    parsedHost.search.length > 0 ||
    parsedHost.hash.length > 0
  ) {
    throw new Error("Manual Clone source is an unsafe Git remote");
  }
  const path = normalizeSafeRepositoryPath(rawPath);
  return `${username}@${parsedHost.hostname.toLowerCase()}:${path}.git`;
}

function normalizeSafeRepositoryPath(value: string): string {
  const trimmed = value.replace(/^\/+|\/+$/gu, "");
  const path = trimmed.endsWith(".git") ? trimmed.slice(0, -4) : trimmed;
  const lowered = path.toLowerCase();
  if (
    path.length === 0 ||
    path.includes("\\") ||
    path.includes("?") ||
    path.includes("#") ||
    lowered.includes("%00") ||
    lowered.includes("%2f") ||
    lowered.includes("%5c") ||
    lowered.includes("%2e") ||
    path
      .split("/")
      .some(
        (component) =>
          component.length === 0 ||
          component === "." ||
          component === ".." ||
          Array.from(component).some((character) => character.charCodeAt(0) < 0x20)
      )
  ) {
    throw new Error("Manual Clone source is an unsafe Git remote");
  }
  return path;
}

function isSafeSshUsername(value: string): boolean {
  return value.length > 0 && value.length <= 256 && /^[A-Za-z0-9._-]+$/u.test(value);
}

function isGitRemoteHelper(value: string): boolean {
  const separator = value.indexOf("::");
  if (separator <= 0) return false;
  return /^[A-Za-z][A-Za-z0-9+.-]*$/u.test(value.slice(0, separator));
}

function canonicalGitPort(protocol: string, value: string): string {
  if ((protocol === "https:" && value === "443") || (protocol === "ssh:" && value === "22")) {
    return "";
  }
  return value;
}

function gitRemoteAuthority(host: string, port: string): string {
  const bracketed = host.includes(":") && !host.startsWith("[") ? `[${host}]` : host;
  return port.length === 0 ? bracketed : `${bracketed}:${port}`;
}

function cloneProjectTargetLabel(target: CloneRequest["projectTarget"]): string {
  return target.kind === "existing" ? `project ${target.projectId}` : `new project ${target.name}`;
}

function repositoryNetworkLabel(
  state: RepositoryNetworkState | undefined,
  repositoryId: string,
  requestedRemoteName: string | null,
  forPush: boolean,
  requestedBranchName: string | null = null
): string {
  const remoteName = requestedRemoteName ?? state?.upstream?.remoteName ?? null;
  const remote = state?.remotes.find((candidate) => candidate.name === remoteName);
  const remoteUrl = forPush ? (remote?.pushUrl ?? remote?.fetchUrl) : remote?.fetchUrl;
  const target =
    remoteName === null
      ? "configured upstream"
      : remoteUrl === undefined
        ? remoteName
        : `${remoteName} (${remoteUrl})`;
  const branchName =
    requestedBranchName ?? (requestedRemoteName === null ? state?.upstream?.branchName : null);
  return [
    target,
    branchName === null || branchName === undefined ? null : `branch ${branchName}`,
    `repository ${repositoryId}`
  ]
    .filter((part): part is string => part !== null)
    .join(" · ");
}
