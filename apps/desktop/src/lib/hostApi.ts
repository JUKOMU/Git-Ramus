import {
  changesResultSchema,
  authorizationDecisionSchema,
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
import { open as openDialog } from "@tauri-apps/plugin-dialog";
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
}): HostApi {
  const { prompts, files } = dependencies;
  const instanceCache = new Map<string, ProviderInstance>();
  const accountCache = new Map<string, ProviderAccountSummary>();
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
  files: nativeCertificateFileSelectionPort
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
