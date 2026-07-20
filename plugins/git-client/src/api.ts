import {
  cloneIntentRequestSchema,
  cloneIntentSummarySchema,
  cloneRequestSchema,
  cloneResultSchema,
  effectiveTransportSchema,
  errorEnvelopeSchema,
  networkOperationResultSchema,
  repositoryFetchRequestSchema,
  repositoryNetworkStateSchema,
  repositoryPullRequestSchema,
  repositoryPushRequestSchema,
  repositoryRequestSchema,
  repositoryTransportBindingSchema,
  repositoryTransportBindRequestSchema,
  repositoryTransportUnbindRequestSchema,
  transportOperationCancelRequestSchema,
  transportProfileCreateRequestSchema,
  transportProfileDeleteRequestSchema,
  transportProfileDeletionImpactSchema,
  transportProfileListResponseSchema,
  transportProfileRequestSchema,
  transportProfileSummarySchema,
  transportProfileUpdateRequestSchema,
  type CloneIntentRequest,
  type CloneIntentSummary,
  type CloneRequest,
  type CloneResult,
  type ChangesResult,
  type DiffResult,
  type EffectiveIdentity,
  type EffectiveTransport,
  type ErrorEnvelope,
  type GitContextRequest,
  type IdentityBinding,
  type IdentityCreateRequest,
  type IdentityListResponse,
  type IdentityProfile,
  type IdentityProfileRequest,
  type IdentityUpdateRequest,
  type NetworkOperationResult,
  type Overview,
  type Project,
  type ProjectListResponse,
  type ProjectScanRequest,
  type ProjectUpdateScanRulesRequest,
  type RepositoryCommitRequest,
  type RepositoryDiffRequest,
  type RepositoryFetchRequest,
  type RepositoryIdentityBindRequest,
  type RepositoryIdentityRequest,
  type RepositoryNetworkState,
  type RepositoryPullRequest,
  type RepositoryPushRequest,
  type RepositoryRequest,
  type RepositoryScanRecord,
  type RepositoryStageRequest,
  type RepositoryUnstageRequest,
  type RepositoryTransportBindRequest,
  type RepositoryTransportBinding,
  type RepositoryTransportUnbindRequest,
  type ScanProjectResult,
  type TrustResponse,
  type TrustStatusResponse,
  type TransportOperationCancelRequest,
  type TransportProfileCreateRequest,
  type TransportProfileDeleteRequest,
  type TransportProfileDeletionImpact,
  type TransportProfileListResponse,
  type TransportProfileRequest,
  type TransportProfileSummary,
  type TransportProfileUpdateRequest,
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
  listTransportProfiles(): Promise<TransportProfileListResponse>;
  createTransportProfile(
    request: TransportProfileCreateRequest
  ): Promise<TransportProfileSummary | null>;
  updateTransportProfile(
    request: TransportProfileUpdateRequest
  ): Promise<TransportProfileSummary | null>;
  getTransportProfileDeletionImpact(
    request: TransportProfileRequest
  ): Promise<TransportProfileDeletionImpact>;
  deleteTransportProfile(request: TransportProfileDeleteRequest): Promise<void>;
  getEffectiveRepositoryTransport(request: RepositoryRequest): Promise<EffectiveTransport>;
  getRepositoryNetworkState(request: RepositoryRequest): Promise<RepositoryNetworkState>;
  bindRepositoryTransport(
    request: RepositoryTransportBindRequest
  ): Promise<RepositoryTransportBinding | null>;
  unbindRepositoryTransport(request: RepositoryTransportUnbindRequest): Promise<void>;
  getCloneIntent(request: CloneIntentRequest): Promise<CloneIntentSummary>;
  cloneRepository(request: CloneRequest, signal: AbortSignal): Promise<CloneResult | null>;
  fetchRepository(
    request: RepositoryFetchRequest,
    signal: AbortSignal
  ): Promise<NetworkOperationResult | null>;
  pullRepository(
    request: RepositoryPullRequest,
    signal: AbortSignal
  ): Promise<NetworkOperationResult | null>;
  pushRepository(
    request: RepositoryPushRequest,
    signal: AbortSignal
  ): Promise<NetworkOperationResult | null>;
  cancelNetworkOperation(request: TransportOperationCancelRequest): Promise<void>;
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
      client.request("repositories.getEffectiveIdentity", request),
    listTransportProfiles: () =>
      requestParsed(client, "transportProfiles.list", {}, transportProfileListResponseSchema),
    createTransportProfile: async (request) =>
      requestParsed(
        client,
        "transportProfiles.create",
        transportProfileCreateRequestSchema.parse(request),
        transportProfileSummarySchema.nullable()
      ),
    updateTransportProfile: async (request) =>
      requestParsed(
        client,
        "transportProfiles.update",
        transportProfileUpdateRequestSchema.parse(request),
        transportProfileSummarySchema.nullable()
      ),
    getTransportProfileDeletionImpact: async (request) =>
      requestParsed(
        client,
        "transportProfiles.getDeletionImpact",
        transportProfileRequestSchema.parse(request),
        transportProfileDeletionImpactSchema
      ),
    deleteTransportProfile: async (request) =>
      requestVoid(
        client,
        "transportProfiles.delete",
        transportProfileDeleteRequestSchema.parse(request)
      ),
    getEffectiveRepositoryTransport: async (request) =>
      requestParsed(
        client,
        "repositories.getEffectiveTransport",
        repositoryRequestSchema.parse(request),
        effectiveTransportSchema
      ),
    getRepositoryNetworkState: async (request) =>
      requestParsed(
        client,
        "repositories.getNetworkState",
        repositoryRequestSchema.parse(request),
        repositoryNetworkStateSchema
      ),
    bindRepositoryTransport: async (request) =>
      requestParsed(
        client,
        "repositories.bindTransport",
        repositoryTransportBindRequestSchema.parse(request),
        repositoryTransportBindingSchema.nullable()
      ),
    unbindRepositoryTransport: async (request) =>
      requestVoid(
        client,
        "repositories.unbindTransport",
        repositoryTransportUnbindRequestSchema.parse(request)
      ),
    getCloneIntent: async (request) =>
      requestParsed(
        client,
        "cloneIntents.get",
        cloneIntentRequestSchema.parse(request),
        cloneIntentSummarySchema
      ),
    cloneRepository: async (request, signal) => {
      const parsed = cloneRequestSchema.parse(request);
      return requestCancellable(
        client,
        "repositories.clone",
        parsed,
        cloneResultSchema.nullable(),
        signal
      );
    },
    fetchRepository: async (request, signal) => {
      const parsed = repositoryFetchRequestSchema.parse(request);
      return requestCancellable(
        client,
        "repositories.fetch",
        parsed,
        networkOperationResultSchema.nullable(),
        signal
      );
    },
    pullRepository: async (request, signal) => {
      const parsed = repositoryPullRequestSchema.parse(request);
      return requestCancellable(
        client,
        "repositories.pull",
        parsed,
        networkOperationResultSchema.nullable(),
        signal
      );
    },
    pushRepository: async (request, signal) => {
      const parsed = repositoryPushRequestSchema.parse(request);
      return requestCancellable(
        client,
        "repositories.push",
        parsed,
        networkOperationResultSchema.nullable(),
        signal
      );
    },
    cancelNetworkOperation: async (request) =>
      requestVoid(
        client,
        "repositories.cancelNetworkOperation",
        transportOperationCancelRequestSchema.parse(request)
      )
  };
}

interface RuntimeSchema<T> {
  parse(value: unknown): T;
}

async function requestParsed<T>(
  client: PluginClient,
  method: string,
  params: unknown,
  schema: RuntimeSchema<T>
): Promise<T> {
  return schema.parse(await client.request<unknown>(method, params));
}

async function requestVoid(client: PluginClient, method: string, params: unknown): Promise<void> {
  const result = await client.request<unknown>(method, params);
  if (result !== undefined) throw new Error(`${method} returned an unexpected response`);
}

function requestCancellable<T>(
  client: PluginClient,
  method: string,
  params: { operationId: string },
  schema: RuntimeSchema<T>,
  signal: AbortSignal
): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    let settled = false;
    const finish = (settle: () => void) => {
      if (settled) return;
      settled = true;
      signal.removeEventListener("abort", abort);
      settle();
    };
    const abort = () => {
      if (settled) return;
      const cancellation = transportOperationCancelRequestSchema.parse({
        operationId: params.operationId
      });
      void requestVoid(client, "repositories.cancelNetworkOperation", cancellation).catch(
        () => undefined
      );
      finish(() => reject(abortError()));
    };

    if (signal.aborted) {
      abort();
      return;
    }
    signal.addEventListener("abort", abort, { once: true });
    void client.request<unknown>(method, params).then(
      (result) => {
        if (settled) return;
        try {
          const parsed = schema.parse(result);
          finish(() => resolve(parsed));
        } catch (error: unknown) {
          finish(() => reject(error));
        }
      },
      (error: unknown) => finish(() => reject(error))
    );
  });
}

function abortError(): DOMException {
  return new DOMException("Git network operation was cancelled", "AbortError");
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
