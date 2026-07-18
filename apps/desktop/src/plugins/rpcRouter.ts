import {
  gitContextRequestSchema,
  identityCreateRequestSchema,
  identityProfileRequestSchema,
  identityUpdateRequestSchema,
  projectScanRequestSchema,
  projectUpdateScanRulesRequestSchema,
  repositoryCommitRequestSchema,
  repositoryDiffRequestSchema,
  repositoryIdentityBindRequestSchema,
  repositoryIdentityRequestSchema,
  repositoryRequestSchema,
  repositoryStageRequestSchema,
  repositoryUnstageRequestSchema,
  workspaceCreateRequestSchema,
  workspaceDeleteRequestSchema,
  workspaceRequestSchema,
  workspaceUpdateMembershipRequestSchema
} from "@git-ramus/contracts";
import type { ErrorEnvelope, RpcRequest } from "@git-ramus/contracts";
import type { HostApi } from "../lib/hostApi";

export const RPC_RESOURCES = {
  projects: "projects",
  workspaces: "workspaces",
  repositories: "repositories",
  identities: "identities"
} as const;

interface RuntimeSchema<T> {
  parse(value: unknown): T;
}

interface Route {
  capability: string;
  resource: string;
  prepare(pluginId: string, params: unknown, hostApi: HostApi): () => Promise<unknown>;
}

function defineRoute<T>(
  capability: string,
  resource: string,
  schema: RuntimeSchema<T>,
  handle: (params: T, hostApi: HostApi, pluginId: string) => Promise<unknown>
): Route {
  return {
    capability,
    resource,
    prepare(pluginId, params, hostApi) {
      const parsed = schema.parse(params);
      return () => handle(parsed, hostApi, pluginId);
    }
  };
}

const emptyParamsSchema: RuntimeSchema<Record<string, never>> = {
  parse(value) {
    if (!isRecord(value) || Object.keys(value).length !== 0) {
      throw new Error("expected an empty parameter object");
    }
    return {};
  }
};

const echoParamsSchema: RuntimeSchema<{ message: string }> = {
  parse(value) {
    if (
      !isRecord(value) ||
      Object.keys(value).length !== 1 ||
      typeof value.message !== "string" ||
      value.message.trim().length === 0
    ) {
      throw new Error("expected a non-empty message");
    }
    return { message: value.message };
  }
};

// No route accepts a rootPath or singular path parameter. File-selection routes accept only
// repository-relative `paths`, which the Rust host revalidates against the latest change set.
const routes: Readonly<Record<string, Route>> = {
  "app.getInfo": defineRoute("app:read", "info", emptyParamsSchema, (_params, hostApi) =>
    hostApi.getAppInfo()
  ),
  "tasks.startEcho": defineRoute(
    "tasks:create",
    "echo",
    echoParamsSchema,
    (params, hostApi, pluginId) => hostApi.startEchoJob(pluginId, params.message)
  ),
  "projects.list": defineRoute(
    "projects:manage",
    RPC_RESOURCES.projects,
    emptyParamsSchema,
    (_params, hostApi) => hostApi.listProjects()
  ),
  "projects.updateScanRules": defineRoute(
    "projects:manage",
    RPC_RESOURCES.projects,
    projectUpdateScanRulesRequestSchema,
    (params, hostApi) => hostApi.updateProjectScanRules(params)
  ),
  "projects.scan": defineRoute(
    "projects:manage",
    RPC_RESOURCES.projects,
    projectScanRequestSchema,
    (params, hostApi) => hostApi.scanProject(params)
  ),
  "workspaces.list": defineRoute(
    "workspaces:manage",
    RPC_RESOURCES.workspaces,
    emptyParamsSchema,
    (_params, hostApi) => hostApi.listWorkspaces()
  ),
  "workspaces.create": defineRoute(
    "workspaces:manage",
    RPC_RESOURCES.workspaces,
    workspaceCreateRequestSchema,
    (params, hostApi) => hostApi.createWorkspace(params)
  ),
  "workspaces.getMembership": defineRoute(
    "workspaces:manage",
    RPC_RESOURCES.workspaces,
    workspaceRequestSchema,
    (params, hostApi) => hostApi.getWorkspaceMembership(params)
  ),
  "workspaces.updateMembership": defineRoute(
    "workspaces:manage",
    RPC_RESOURCES.workspaces,
    workspaceUpdateMembershipRequestSchema,
    (params, hostApi) => hostApi.updateWorkspaceMembership(params)
  ),
  "workspaces.delete": defineRoute(
    "workspaces:manage",
    RPC_RESOURCES.workspaces,
    workspaceDeleteRequestSchema,
    (params, hostApi) => hostApi.deleteWorkspace(params)
  ),
  "overview.get": defineRoute(
    "repositories:read",
    RPC_RESOURCES.repositories,
    gitContextRequestSchema,
    (params, hostApi) => hostApi.getOverview(params)
  ),
  "repositories.getSnapshot": defineRoute(
    "repositories:read",
    RPC_RESOURCES.repositories,
    repositoryRequestSchema,
    (params, hostApi) => hostApi.getRepositorySnapshot(params)
  ),
  "repositories.getChanges": defineRoute(
    "repositories:read",
    RPC_RESOURCES.repositories,
    repositoryRequestSchema,
    (params, hostApi) => hostApi.getRepositoryChanges(params)
  ),
  "repositories.getDiff": defineRoute(
    "repositories:read",
    RPC_RESOURCES.repositories,
    repositoryDiffRequestSchema,
    (params, hostApi) => hostApi.getRepositoryDiff(params)
  ),
  "repositories.stage": defineRoute(
    "repositories:write",
    RPC_RESOURCES.repositories,
    repositoryStageRequestSchema,
    (params, hostApi) => hostApi.stageRepository(params)
  ),
  "repositories.unstage": defineRoute(
    "repositories:write",
    RPC_RESOURCES.repositories,
    repositoryUnstageRequestSchema,
    (params, hostApi) => hostApi.unstageRepository(params)
  ),
  "repositories.commit": defineRoute(
    "repositories:write",
    RPC_RESOURCES.repositories,
    repositoryCommitRequestSchema,
    (params, hostApi) => hostApi.commitRepository(params)
  ),
  "repositories.trust": defineRoute(
    "repositories:write",
    RPC_RESOURCES.repositories,
    repositoryIdentityRequestSchema,
    (params, hostApi) => hostApi.trustRepository(params)
  ),
  "identities.list": defineRoute(
    "identities:read",
    RPC_RESOURCES.identities,
    emptyParamsSchema,
    (_params, hostApi) => hostApi.listIdentities()
  ),
  "identities.create": defineRoute(
    "identities:write",
    RPC_RESOURCES.identities,
    identityCreateRequestSchema,
    (params, hostApi) => hostApi.createIdentity(params)
  ),
  "identities.update": defineRoute(
    "identities:write",
    RPC_RESOURCES.identities,
    identityUpdateRequestSchema,
    (params, hostApi) => hostApi.updateIdentity(params)
  ),
  "identities.delete": defineRoute(
    "identities:write",
    RPC_RESOURCES.identities,
    identityProfileRequestSchema,
    (params, hostApi) => hostApi.deleteIdentity(params)
  ),
  "identities.setGlobal": defineRoute(
    "identities:write",
    RPC_RESOURCES.identities,
    identityProfileRequestSchema,
    (params, hostApi) => hostApi.setGlobalIdentity(params)
  ),
  "repositories.bindIdentity": defineRoute(
    "identities:write",
    RPC_RESOURCES.identities,
    repositoryIdentityBindRequestSchema,
    (params, hostApi) => hostApi.bindRepositoryIdentity(params)
  ),
  "repositories.unbindIdentity": defineRoute(
    "identities:write",
    RPC_RESOURCES.identities,
    repositoryIdentityRequestSchema,
    (params, hostApi) => hostApi.unbindRepositoryIdentity(params)
  ),
  "repositories.getEffectiveIdentity": defineRoute(
    "identities:read",
    RPC_RESOURCES.identities,
    repositoryIdentityRequestSchema,
    (params, hostApi) => hostApi.getEffectiveRepositoryIdentity(params)
  )
};

export async function dispatchPluginRpc(
  pluginId: string,
  request: RpcRequest,
  hostApi: HostApi
): Promise<unknown> {
  const route = routes[request.method];
  if (route === undefined) {
    throw rpcError(
      "rpc.unknown-method",
      "validation",
      "Unknown plugin RPC method",
      pluginId,
      null,
      "rpc.route"
    );
  }

  let execute: () => Promise<unknown>;
  try {
    execute = route.prepare(pluginId, request.params, hostApi);
  } catch {
    throw rpcError(
      "rpc.invalid-params",
      "validation",
      "Plugin RPC parameters are invalid",
      pluginId,
      route.resource,
      "rpc.validation"
    );
  }

  const decision = await hostApi.authorizePluginCall({
    pluginId,
    capability: route.capability,
    resource: route.resource
  });
  if (!decision.allowed) {
    throw rpcError(
      "permission.denied",
      "userActionRequired",
      "Permission denied",
      pluginId,
      route.resource,
      "rpc.authorization"
    );
  }
  return execute();
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function rpcError(
  code: string,
  category: ErrorEnvelope["category"],
  message: string,
  pluginId: string,
  resourceId: string | null,
  failedStep: string
): ErrorEnvelope {
  return {
    code,
    category,
    message,
    operationId: null,
    pluginId,
    resourceId,
    failedStep,
    retryable: false,
    retryAfterMs: null,
    recoveryActions: [],
    details: null
  };
}
