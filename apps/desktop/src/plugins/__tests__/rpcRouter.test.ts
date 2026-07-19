import type { ErrorEnvelope, RpcRequest } from "@git-ramus/contracts";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { HostApi } from "../../lib/hostApi";
import { dispatchPluginRpc } from "../rpcRouter";

const pluginId = "git-ramus.git-client";
const projectId = "87a31769-8aaa-47ca-bef3-47e66f0c62fc";
const workspaceId = "e3d622f1-f1f7-4f7e-8f18-3db8a1e6ffbe";
const repositoryId = "a032bc9c-8759-45ac-856f-b76f9addb9d1";
const profileId = "d23957ac-5c0f-4857-9124-7f1599a41f33";

function createHostApi(): HostApi {
  return {
    getAppInfo: vi.fn(),
    listPlugins: vi.fn(),
    listJobs: vi.fn(),
    authorizePluginCall: vi.fn(async () => ({ allowed: true })),
    startEchoJob: vi.fn(),
    cancelJob: vi.fn(),
    listProjects: vi.fn(),
    updateProjectScanRules: vi.fn(),
    scanProject: vi.fn(),
    listWorkspaces: vi.fn(),
    createWorkspace: vi.fn(),
    getWorkspaceMembership: vi.fn(),
    updateWorkspaceMembership: vi.fn(),
    deleteWorkspace: vi.fn(),
    getOverview: vi.fn(),
    getRepositorySnapshot: vi.fn(),
    getRepositoryChanges: vi.fn(),
    getRepositoryDiff: vi.fn(),
    getRepositoryTrustStatus: vi.fn(),
    stageRepository: vi.fn(),
    unstageRepository: vi.fn(),
    commitRepository: vi.fn(),
    trustRepository: vi.fn(),
    listIdentities: vi.fn(),
    createIdentity: vi.fn(),
    updateIdentity: vi.fn(),
    deleteIdentity: vi.fn(),
    setGlobalIdentity: vi.fn(),
    bindRepositoryIdentity: vi.fn(),
    unbindRepositoryIdentity: vi.fn(),
    getEffectiveRepositoryIdentity: vi.fn()
  };
}

function request(method: string, params: unknown): RpcRequest {
  return {
    type: "rpc:request",
    requestId: "c8f98df3-e949-48e0-a9ad-407fe371a94a",
    sessionId: "f9479c0b-8770-4ec0-8a34-fd8d327b43e9",
    method,
    params
  };
}

function expectAuthorizedBefore(
  hostApi: HostApi,
  capability: string,
  resource: string,
  handler: ReturnType<typeof vi.fn>
) {
  expect(hostApi.authorizePluginCall).toHaveBeenCalledWith({ pluginId, capability, resource });
  const [authorizationOrder] = vi.mocked(hostApi.authorizePluginCall).mock.invocationCallOrder;
  const [handlerOrder] = handler.mock.invocationCallOrder;
  expect(authorizationOrder).toBeDefined();
  expect(handlerOrder).toBeDefined();
  expect(authorizationOrder!).toBeLessThan(handlerOrder!);
}

describe("Git client RPC routes", () => {
  let hostApi: HostApi;

  beforeEach(() => {
    hostApi = createHostApi();
  });

  it("dispatches projects.list through projects:manage/projects", async () => {
    vi.mocked(hostApi.listProjects).mockResolvedValue({ projects: [] });
    await expect(
      dispatchPluginRpc(pluginId, request("projects.list", {}), hostApi)
    ).resolves.toEqual({
      projects: []
    });
    expect(hostApi.listProjects).toHaveBeenCalledWith();
    expectAuthorizedBefore(hostApi, "projects:manage", "projects", vi.mocked(hostApi.listProjects));
  });

  it("dispatches repositories.getChanges through repositories:read/repositories", async () => {
    const params = { projectId, repositoryId };
    const result = { repositoryId, snapshot: null, changes: [] };
    vi.mocked(hostApi.getRepositoryChanges).mockResolvedValue(result as never);
    await expect(
      dispatchPluginRpc(pluginId, request("repositories.getChanges", params), hostApi)
    ).resolves.toBe(result);
    expect(hostApi.getRepositoryChanges).toHaveBeenCalledWith(params);
    expectAuthorizedBefore(
      hostApi,
      "repositories:read",
      "repositories",
      vi.mocked(hostApi.getRepositoryChanges)
    );
  });

  it("dispatches repositories.stage through repositories:write/repositories", async () => {
    const params = { projectId, repositoryId, paths: ["src/main.ts"], all: false };
    const result = { repositoryId, snapshot: null, output: null };
    vi.mocked(hostApi.stageRepository).mockResolvedValue(result as never);
    await expect(
      dispatchPluginRpc(pluginId, request("repositories.stage", params), hostApi)
    ).resolves.toBe(result);
    expect(hostApi.stageRepository).toHaveBeenCalledWith(params);
    expectAuthorizedBefore(
      hostApi,
      "repositories:write",
      "repositories",
      vi.mocked(hostApi.stageRepository)
    );
  });

  it("dispatches repositories.commit through repositories:write/repositories", async () => {
    const params = {
      workspaceId,
      repositoryId,
      message: "Commit from plugin",
      identityProfileId: profileId
    };
    const result = { repositoryId, snapshot: null, output: "abc123" };
    vi.mocked(hostApi.commitRepository).mockResolvedValue(result as never);
    await expect(
      dispatchPluginRpc(pluginId, request("repositories.commit", params), hostApi)
    ).resolves.toBe(result);
    expect(hostApi.commitRepository).toHaveBeenCalledWith(params);
    expectAuthorizedBefore(
      hostApi,
      "repositories:write",
      "repositories",
      vi.mocked(hostApi.commitRepository)
    );
  });

  it("dispatches identities.setGlobal through identities:write/identities", async () => {
    const params = { profileId };
    const result = { id: profileId };
    vi.mocked(hostApi.setGlobalIdentity).mockResolvedValue(result as never);
    await expect(
      dispatchPluginRpc(pluginId, request("identities.setGlobal", params), hostApi)
    ).resolves.toBe(result);
    expect(hostApi.setGlobalIdentity).toHaveBeenCalledWith(params);
    expectAuthorizedBefore(
      hostApi,
      "identities:write",
      "identities",
      vi.mocked(hostApi.setGlobalIdentity)
    );
  });

  it("dispatches repositories.trust through repositories:write/repositories", async () => {
    const params = { projectId, repositoryId };
    const result = { trust: { repositoryId } };
    vi.mocked(hostApi.trustRepository).mockResolvedValue(result as never);
    await expect(
      dispatchPluginRpc(pluginId, request("repositories.trust", params), hostApi)
    ).resolves.toBe(result);
    expect(hostApi.trustRepository).toHaveBeenCalledWith(params);
    expectAuthorizedBefore(
      hostApi,
      "repositories:write",
      "repositories",
      vi.mocked(hostApi.trustRepository)
    );
  });

  it("uses identities:write for repository identity binding", async () => {
    const params = { projectId, repositoryId, identityProfileId: profileId };
    vi.mocked(hostApi.bindRepositoryIdentity).mockResolvedValue({} as never);
    await dispatchPluginRpc(pluginId, request("repositories.bindIdentity", params), hostApi);
    expect(hostApi.bindRepositoryIdentity).toHaveBeenCalledWith(params);
    expectAuthorizedBefore(
      hostApi,
      "identities:write",
      "identities",
      vi.mocked(hostApi.bindRepositoryIdentity)
    );
  });

  it.each([
    {
      method: "projects.updateScanRules",
      params: { projectId, scanDepth: 4, excludePatterns: ["target"] },
      capability: "projects:manage",
      resource: "projects",
      hostMethod: "updateProjectScanRules",
      argument: { projectId, scanDepth: 4, excludePatterns: ["target"] }
    },
    {
      method: "projects.scan",
      params: { projectId },
      capability: "projects:manage",
      resource: "projects",
      hostMethod: "scanProject",
      argument: { projectId }
    },
    {
      method: "workspaces.list",
      params: {},
      capability: "workspaces:manage",
      resource: "workspaces",
      hostMethod: "listWorkspaces",
      argument: undefined
    },
    {
      method: "workspaces.create",
      params: { name: "Workspace" },
      capability: "workspaces:manage",
      resource: "workspaces",
      hostMethod: "createWorkspace",
      argument: { name: "Workspace" }
    },
    {
      method: "workspaces.getMembership",
      params: { workspaceId },
      capability: "workspaces:manage",
      resource: "workspaces",
      hostMethod: "getWorkspaceMembership",
      argument: { workspaceId }
    },
    {
      method: "workspaces.updateMembership",
      params: { workspaceId, projectIds: [projectId] },
      capability: "workspaces:manage",
      resource: "workspaces",
      hostMethod: "updateWorkspaceMembership",
      argument: { workspaceId, projectIds: [projectId] }
    },
    {
      method: "workspaces.delete",
      params: { workspaceId },
      capability: "workspaces:manage",
      resource: "workspaces",
      hostMethod: "deleteWorkspace",
      argument: { workspaceId }
    },
    {
      method: "overview.get",
      params: { projectId },
      capability: "repositories:read",
      resource: "repositories",
      hostMethod: "getOverview",
      argument: { projectId }
    },
    {
      method: "repositories.getSnapshot",
      params: { projectId, repositoryId },
      capability: "repositories:read",
      resource: "repositories",
      hostMethod: "getRepositorySnapshot",
      argument: { projectId, repositoryId }
    },
    {
      method: "repositories.getDiff",
      params: { projectId, repositoryId, paths: [], staged: false },
      capability: "repositories:read",
      resource: "repositories",
      hostMethod: "getRepositoryDiff",
      argument: { projectId, repositoryId, paths: [], staged: false }
    },
    {
      method: "repositories.getTrustStatus",
      params: { projectId, repositoryId },
      capability: "repositories:read",
      resource: "repositories",
      hostMethod: "getRepositoryTrustStatus",
      argument: { projectId, repositoryId }
    },
    {
      method: "repositories.unstage",
      params: { projectId, repositoryId, paths: ["src/main.ts"] },
      capability: "repositories:write",
      resource: "repositories",
      hostMethod: "unstageRepository",
      argument: { projectId, repositoryId, paths: ["src/main.ts"] }
    },
    {
      method: "identities.list",
      params: {},
      capability: "identities:read",
      resource: "identities",
      hostMethod: "listIdentities",
      argument: undefined
    },
    {
      method: "identities.create",
      params: {
        displayName: "Profile",
        userName: "User",
        userEmail: "user@example.com",
        gpgFormat: null,
        signingKey: null,
        signCommits: false,
        signTags: false
      },
      capability: "identities:write",
      resource: "identities",
      hostMethod: "createIdentity",
      argument: {
        displayName: "Profile",
        userName: "User",
        userEmail: "user@example.com",
        gpgFormat: null,
        signingKey: null,
        signCommits: false,
        signTags: false
      }
    },
    {
      method: "identities.update",
      params: {
        profileId,
        displayName: "Profile",
        userName: "User",
        userEmail: "user@example.com",
        gpgFormat: null,
        signingKey: null,
        signCommits: false,
        signTags: false
      },
      capability: "identities:write",
      resource: "identities",
      hostMethod: "updateIdentity",
      argument: {
        profileId,
        displayName: "Profile",
        userName: "User",
        userEmail: "user@example.com",
        gpgFormat: null,
        signingKey: null,
        signCommits: false,
        signTags: false
      }
    },
    {
      method: "identities.delete",
      params: { profileId },
      capability: "identities:write",
      resource: "identities",
      hostMethod: "deleteIdentity",
      argument: { profileId }
    },
    {
      method: "repositories.unbindIdentity",
      params: { projectId, repositoryId },
      capability: "identities:write",
      resource: "identities",
      hostMethod: "unbindRepositoryIdentity",
      argument: { projectId, repositoryId }
    },
    {
      method: "repositories.getEffectiveIdentity",
      params: { projectId, repositoryId },
      capability: "identities:read",
      resource: "identities",
      hostMethod: "getEffectiveRepositoryIdentity",
      argument: { projectId, repositoryId }
    }
  ] as const)(
    "dispatches $method through $capability/$resource",
    async ({ method, params, capability, resource, hostMethod, argument }) => {
      const handler = hostApi[hostMethod] as ReturnType<typeof vi.fn>;
      handler.mockResolvedValue(undefined);

      await expect(
        dispatchPluginRpc(pluginId, request(method, params), hostApi)
      ).resolves.toBeUndefined();
      if (argument === undefined) {
        expect(handler).toHaveBeenCalledWith();
      } else {
        expect(handler).toHaveBeenCalledWith(argument);
      }
      expectAuthorizedBefore(hostApi, capability, resource, handler);
    }
  );

  it.each([
    ["projects.updateScanRules", { projectId, rootPath: "C:/secret" }, "updateProjectScanRules"],
    [
      "repositories.stage",
      { projectId, repositoryId, paths: [], all: true, path: "C:/secret" },
      "stageRepository"
    ],
    [
      "repositories.getTrustStatus",
      { projectId, repositoryId, rootPath: "C:/secret" },
      "getRepositoryTrustStatus"
    ]
  ] as const)("rejects arbitrary rootPath/path fields for %s", async (method, params, handler) => {
    await expect(
      dispatchPluginRpc(pluginId, request(method, params), hostApi)
    ).rejects.toMatchObject({
      code: "rpc.invalid-params",
      category: "validation"
    });
    expect(hostApi[handler]).not.toHaveBeenCalled();
  });

  it("does not expose the path-bearing projects.create RPC method", async () => {
    await expect(
      dispatchPluginRpc(
        pluginId,
        request("projects.create", { name: "Secret", rootPath: "C:/secret" }),
        hostApi
      )
    ).rejects.toMatchObject({ code: "rpc.unknown-method" });
    expect(hostApi.authorizePluginCall).not.toHaveBeenCalled();
  });

  it("preserves structured unknown-repository and untrusted-write host errors", async () => {
    const unknownRepository = error("resource.not-found", "validation", "Repository not found");
    const trustRequired = error(
      "git.trust-required",
      "userActionRequired",
      "Repository trust is required before this write"
    );
    vi.mocked(hostApi.getRepositoryChanges).mockRejectedValueOnce(unknownRepository);
    await expect(
      dispatchPluginRpc(
        pluginId,
        request("repositories.getChanges", { projectId, repositoryId }),
        hostApi
      )
    ).rejects.toBe(unknownRepository);
    vi.mocked(hostApi.stageRepository).mockRejectedValueOnce(trustRequired);
    await expect(
      dispatchPluginRpc(
        pluginId,
        request("repositories.stage", { projectId, repositoryId, paths: [], all: true }),
        hostApi
      )
    ).rejects.toBe(trustRequired);
  });

  it("never calls a handler when the capability is undeclared or denied", async () => {
    vi.mocked(hostApi.authorizePluginCall).mockResolvedValue({ allowed: false });
    await expect(
      dispatchPluginRpc(
        pluginId,
        request("repositories.commit", {
          projectId,
          repositoryId,
          message: "Denied"
        }),
        hostApi
      )
    ).rejects.toMatchObject({
      code: "permission.denied",
      category: "userActionRequired",
      resourceId: "repositories"
    });
    expect(hostApi.commitRepository).not.toHaveBeenCalled();
  });

  it("rejects unknown methods without authorizing or invoking a handler", async () => {
    await expect(
      dispatchPluginRpc(pluginId, request("repositories.nope", {}), hostApi)
    ).rejects.toMatchObject({
      code: "rpc.unknown-method",
      category: "validation"
    });
    expect(hostApi.authorizePluginCall).not.toHaveBeenCalled();
  });
});

function error(code: string, category: ErrorEnvelope["category"], message: string): ErrorEnvelope {
  return {
    code,
    category,
    message,
    operationId: null,
    pluginId: null,
    resourceId: repositoryId,
    failedStep: null,
    retryable: false,
    retryAfterMs: null,
    recoveryActions: [],
    details: null
  };
}
