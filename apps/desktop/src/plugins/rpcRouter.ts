import {
  gitContextRequestSchema,
  identityCreateRequestSchema,
  identityProfileRequestSchema,
  identityUpdateRequestSchema,
  projectCreateRequestSchema,
  projectScanRequestSchema,
  projectUpdateScanRulesRequestSchema,
  providerAccountConnectRequestSchema,
  providerAccountDeleteRequestSchema,
  providerAccountDeletionImpactRequestSchema,
  providerAccountListRequestSchema,
  providerAccountRotateRequestSchema,
  providerAccountSetDefaultRequestSchema,
  providerAccountValidateRequestSchema,
  providerBindingDeleteRequestSchema,
  providerBindingListRequestSchema,
  providerBindingSetRequestSchema,
  providerInstanceCreateRequestSchema,
  providerInstanceRequestSchema,
  providerInstanceUpdateRequestSchema,
  providerLocalRemoteMatchRequestSchema,
  providerOperationCancelRequestSchema,
  providerReadAccessRequestSchema,
  providerReadAccessRevokeRequestSchema,
  providerRepositoryListRequestSchema,
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
  identities: "identities",
  providers: "providers"
} as const;

interface RuntimeSchema<T> {
  parse(value: unknown): T;
}

interface AuthorizationRequirement<T> {
  check: "granted" | "declared";
  capability: string;
  resources(params: T): string[];
  mode: "all" | "any";
}

interface PreparedAuthorizationRequirement {
  check: "granted" | "declared";
  capability: string;
  resources: string[];
  mode: "all" | "any";
}

interface PreparedRoute {
  requirements: PreparedAuthorizationRequirement[];
  execute(): Promise<unknown>;
}

interface Route {
  resource: string;
  prepare(pluginId: string, params: unknown, hostApi: HostApi): PreparedRoute;
}

function defineRoute<T>(
  capability: string,
  resource: string,
  schema: RuntimeSchema<T>,
  handle: (params: T, hostApi: HostApi, pluginId: string) => Promise<unknown>
): Route {
  return defineRouteWithRequirements(
    resource,
    schema,
    [fixedRequirement(capability, resource)],
    handle
  );
}

function defineRouteWithRequirements<T>(
  resource: string,
  schema: RuntimeSchema<T>,
  requirements: AuthorizationRequirement<T>[],
  handle: (params: T, hostApi: HostApi, pluginId: string) => Promise<unknown>
): Route {
  return {
    resource,
    prepare(pluginId, params, hostApi) {
      const parsed = schema.parse(params);
      return {
        requirements: requirements.map((requirement) => ({
          check: requirement.check,
          capability: requirement.capability,
          resources: requirement.resources(parsed),
          mode: requirement.mode
        })),
        execute: () => handle(parsed, hostApi, pluginId)
      };
    }
  };
}

function fixedRequirement<T>(
  capability: string,
  resource: string,
  check: "granted" | "declared" = "granted"
): AuthorizationRequirement<T> {
  return {
    check,
    capability,
    resources: () => [resource],
    mode: "all"
  };
}

function providerAccountReadRequirement<
  T extends { accountId: string }
>(): AuthorizationRequirement<T> {
  return {
    check: "granted",
    capability: "providers:read",
    resources: ({ accountId }) => [`provider-account/${accountId}`, RPC_RESOURCES.providers],
    mode: "any"
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
  "projects.create": defineRoute(
    "projects:manage",
    RPC_RESOURCES.projects,
    projectCreateRequestSchema,
    (_params, hostApi) => hostApi.createProject()
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
  "repositories.getTrustStatus": defineRoute(
    "repositories:read",
    RPC_RESOURCES.repositories,
    repositoryRequestSchema,
    (params, hostApi) => hostApi.getRepositoryTrustStatus(params)
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
  ),
  "providers.listInstances": defineRoute(
    "providers:manage",
    RPC_RESOURCES.providers,
    emptyParamsSchema,
    (_params, hostApi) => hostApi.listProviderInstances()
  ),
  "providers.createInstance": defineRoute(
    "providers:manage",
    RPC_RESOURCES.providers,
    providerInstanceCreateRequestSchema,
    (params, hostApi) => hostApi.createProviderInstance(params)
  ),
  "providers.updateInstance": defineRoute(
    "providers:manage",
    RPC_RESOURCES.providers,
    providerInstanceUpdateRequestSchema,
    (params, hostApi) => hostApi.updateProviderInstance(params)
  ),
  "providers.validateInstance": defineRoute(
    "providers:manage",
    RPC_RESOURCES.providers,
    providerInstanceRequestSchema,
    (params, hostApi) => hostApi.validateProviderInstance(params)
  ),
  "providers.deleteInstance": defineRoute(
    "providers:manage",
    RPC_RESOURCES.providers,
    providerInstanceRequestSchema,
    (params, hostApi) => hostApi.deleteProviderInstance(params)
  ),
  "providers.listAccounts": defineRoute(
    "providers:manage",
    RPC_RESOURCES.providers,
    providerAccountListRequestSchema,
    (params, hostApi) => hostApi.listProviderAccounts(params)
  ),
  "providers.connectAccount": defineRoute(
    "providers:manage",
    RPC_RESOURCES.providers,
    providerAccountConnectRequestSchema,
    (params, hostApi, pluginId) => hostApi.connectProviderAccount(pluginId, params)
  ),
  "providers.rotateAccount": defineRoute(
    "providers:manage",
    RPC_RESOURCES.providers,
    providerAccountRotateRequestSchema,
    (params, hostApi, pluginId) => hostApi.rotateProviderAccount(pluginId, params)
  ),
  "providers.validateAccount": defineRoute(
    "providers:manage",
    RPC_RESOURCES.providers,
    providerAccountValidateRequestSchema,
    (params, hostApi) => hostApi.validateProviderAccount(params)
  ),
  "providers.setDefaultAccount": defineRoute(
    "providers:manage",
    RPC_RESOURCES.providers,
    providerAccountSetDefaultRequestSchema,
    (params, hostApi) => hostApi.setDefaultProviderAccount(params)
  ),
  "providers.getAccountDeletionImpact": defineRoute(
    "providers:manage",
    RPC_RESOURCES.providers,
    providerAccountDeletionImpactRequestSchema,
    (params, hostApi) => hostApi.getProviderAccountDeletionImpact(params)
  ),
  "providers.deleteAccount": defineRoute(
    "providers:manage",
    RPC_RESOURCES.providers,
    providerAccountDeleteRequestSchema,
    (params, hostApi) => hostApi.deleteProviderAccount(params)
  ),
  "providers.listAuthorizedAccounts": defineRouteWithRequirements(
    RPC_RESOURCES.providers,
    emptyParamsSchema,
    [fixedRequirement("providers:read", RPC_RESOURCES.providers, "declared")],
    (_params, hostApi, pluginId) => hostApi.listAuthorizedProviderAccounts(pluginId)
  ),
  "providers.requestReadAccess": defineRouteWithRequirements(
    RPC_RESOURCES.providers,
    providerReadAccessRequestSchema,
    [fixedRequirement("providers:read", RPC_RESOURCES.providers, "declared")],
    (_params, hostApi, pluginId) => hostApi.requestProviderReadAccess(pluginId)
  ),
  "providers.revokeReadAccess": defineRouteWithRequirements(
    RPC_RESOURCES.providers,
    providerReadAccessRevokeRequestSchema,
    [fixedRequirement("providers:read", RPC_RESOURCES.providers, "declared")],
    (params, hostApi, pluginId) => hostApi.revokeProviderReadAccess(pluginId, params)
  ),
  "providers.listRepositories": defineRouteWithRequirements(
    RPC_RESOURCES.providers,
    providerRepositoryListRequestSchema,
    [providerAccountReadRequirement()],
    (params, hostApi, pluginId) => hostApi.listProviderRepositories(pluginId, params)
  ),
  "providers.cancelOperation": defineRouteWithRequirements(
    RPC_RESOURCES.providers,
    providerOperationCancelRequestSchema,
    [providerAccountReadRequirement()],
    (params, hostApi, pluginId) => hostApi.cancelProviderOperation(pluginId, params)
  ),
  "providers.matchLocalRemotes": defineRouteWithRequirements(
    RPC_RESOURCES.providers,
    providerLocalRemoteMatchRequestSchema,
    [
      providerAccountReadRequirement(),
      fixedRequirement("repositories:read", RPC_RESOURCES.repositories)
    ],
    (params, hostApi, pluginId) => hostApi.matchLocalProviderRemotes(pluginId, params)
  ),
  "providers.listBindings": defineRouteWithRequirements(
    RPC_RESOURCES.providers,
    providerBindingListRequestSchema,
    [
      providerAccountReadRequirement(),
      fixedRequirement("repositories:read", RPC_RESOURCES.repositories)
    ],
    (params, hostApi) => hostApi.listProviderBindings(params)
  ),
  "providers.bindRemote": defineRouteWithRequirements(
    RPC_RESOURCES.providers,
    providerBindingSetRequestSchema,
    [
      fixedRequirement("providers:manage", RPC_RESOURCES.providers),
      fixedRequirement("repositories:read", RPC_RESOURCES.repositories)
    ],
    (params, hostApi) => hostApi.bindProviderRemote(params)
  ),
  "providers.unbindRemote": defineRouteWithRequirements(
    RPC_RESOURCES.providers,
    providerBindingDeleteRequestSchema,
    [
      fixedRequirement("providers:manage", RPC_RESOURCES.providers),
      fixedRequirement("repositories:read", RPC_RESOURCES.repositories)
    ],
    (params, hostApi) => hostApi.unbindProviderRemote(params)
  )
};

export function isKnownPluginRpcMethod(method: string): boolean {
  return routeFor(method) !== undefined;
}

export async function dispatchPluginRpc(
  pluginId: string,
  request: RpcRequest,
  hostApi: HostApi
): Promise<unknown> {
  const route = routeFor(request.method);
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

  let prepared: PreparedRoute;
  try {
    prepared = route.prepare(pluginId, request.params, hostApi);
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

  for (const requirement of prepared.requirements) {
    if (!(await requirementIsAllowed(pluginId, requirement, hostApi))) {
      throw rpcError(
        "permission.denied",
        "userActionRequired",
        "Permission denied",
        pluginId,
        requirement.resources[0] ?? route.resource,
        "rpc.authorization"
      );
    }
  }
  return prepared.execute();
}

async function requirementIsAllowed(
  pluginId: string,
  requirement: PreparedAuthorizationRequirement,
  hostApi: HostApi
): Promise<boolean> {
  if (requirement.resources.length === 0) return false;
  const authorize =
    requirement.check === "declared"
      ? hostApi.authorizePluginPermissionRequest.bind(hostApi)
      : hostApi.authorizePluginCall.bind(hostApi);
  if (requirement.mode === "any") {
    for (const resource of requirement.resources) {
      const decision = await authorize({
        pluginId,
        capability: requirement.capability,
        resource
      });
      if (decision.allowed) return true;
    }
    return false;
  }
  for (const resource of requirement.resources) {
    const decision = await authorize({
      pluginId,
      capability: requirement.capability,
      resource
    });
    if (!decision.allowed) return false;
  }
  return true;
}

function routeFor(method: string): Route | undefined {
  return Object.prototype.hasOwnProperty.call(routes, method) ? routes[method] : undefined;
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
