import type { PluginClient } from "@git-ramus/plugin-sdk";
import { afterEach, describe, expect, it, vi } from "vitest";
import { createProviderCenterApi } from "../api";

const accountId = "7f3c0214-373c-4d43-b0c7-cdaed1cbcc50";
const instanceId = "6da75ccf-f7df-4bf2-92b7-2c158765726f";
const query = {
  search: "skill",
  visibility: null,
  namespace: null,
  archived: "all" as const,
  sort: "name" as const,
  direction: "asc" as const,
  pageSize: 30
};
const now = "2026-07-19T00:00:00Z";
const repositoryId = "5aa1eea1-c250-4a40-9df4-f74534b7f203";
const providerInstance = {
  id: instanceId,
  providerKind: "gitlab",
  displayName: "GitLab",
  baseUrl: "https://gitlab.example",
  customCaConfigured: false,
  customCaLabel: null,
  providerEnabled: true,
  status: "connected",
  lastValidatedAt: now,
  serverVersion: "18.0",
  createdAt: now,
  updatedAt: now
};
const providerAccount = {
  id: accountId,
  instanceId,
  providerUserId: "9001",
  username: "creator",
  displayName: "Creator",
  avatarUrl: null,
  isDefault: true,
  status: "connected",
  lastValidatedAt: now
};
const binding = {
  repositoryId,
  remoteName: "origin",
  providerInstanceId: instanceId,
  providerAccountId: null,
  providerRepositoryId: "4242",
  fullName: "skills/private-skill",
  webUrl: "https://gitlab.example/skills/private-skill",
  matchedUrl: "git@gitlab.example:skills/private-skill.git",
  bindingSource: "manual",
  boundAt: now,
  updatedAt: now
};

function createClient() {
  const requests: Array<{ method: string; params: Record<string, unknown> }> = [];
  const client = {
    requests,
    request: vi.fn(async (method: string, params: Record<string, unknown>) => {
      requests.push({ method, params });
      switch (method) {
        case "providers.listInstances":
          return { items: [] };
        case "providers.createInstance":
        case "providers.updateInstance":
        case "providers.validateInstance":
          return providerInstance;
        case "providers.listAccounts":
          return { items: [] };
        case "providers.connectAccount":
        case "providers.rotateAccount":
        case "providers.validateAccount":
        case "providers.setDefaultAccount":
          return providerAccount;
        case "providers.getAccountDeletionImpact":
          return {
            accountId,
            instanceId,
            isDefault: true,
            explicitBindingCount: 0,
            inheritedBindingCount: 0,
            siblingAccountIds: [],
            requiresNewDefault: false
          };
        case "providers.listAuthorizedAccounts":
        case "providers.requestReadAccess":
          return { items: [{ instance: providerInstance, account: providerAccount }] };
        case "providers.listRepositories":
          return { items: [], nextCursor: null, hasMore: false, rateLimit: null };
        case "cloneIntents.create":
          return { intentId: "90e1e991-f93e-4e78-817e-d0ceeb06a749" };
        case "cloneIntents.open":
          return null;
        case "providers.matchLocalRemotes":
          return { items: [] };
        case "providers.listBindings":
          return { items: [] };
        case "providers.bindRemote":
          return binding;
        default:
          return null;
      }
    })
  } as unknown as PluginClient & {
    requests: Array<{ method: string; params: Record<string, unknown> }>;
  };
  return client;
}

describe("Provider Center API", () => {
  afterEach(() => {
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
  });

  it("uses exact RPC names and no secret or local path fields", async () => {
    const client = createClient();
    const api = createProviderCenterApi(client);
    await api.listInstances();
    await api.listAccounts(instanceId);
    await api.listAuthorizedAccounts();
    await api.listRepositories({ accountId, query, cursor: null });
    await api.matchLocalRemotes({ instanceId, accountId });
    await api.listBindings(accountId);

    expect(client.requests.map(({ method }) => method)).toEqual([
      "providers.listInstances",
      "providers.listAccounts",
      "providers.listAuthorizedAccounts",
      "providers.listRepositories",
      "providers.matchLocalRemotes",
      "providers.listBindings"
    ]);
    expect(JSON.stringify(client.requests)).not.toMatch(/pat|secretRef|customCaPath|[A-Z]:\\/iu);
  });

  it("creates a Clone intent with repository identity only", async () => {
    const client = createClient();
    const api = createProviderCenterApi(client);

    await expect(api.createCloneIntent(accountId, "4242")).resolves.toEqual({
      intentId: "90e1e991-f93e-4e78-817e-d0ceeb06a749"
    });
    expect(client.requests).toEqual([
      {
        method: "cloneIntents.create",
        params: { accountId, repositoryId: "4242" }
      }
    ]);
    expect(JSON.stringify(client.requests)).not.toMatch(
      /pat|secret|sshKeyPath|destination|[A-Z]:\\|\/home\//iu
    );
  });

  it("asks the Host to open a persisted Clone intent without constructing a route", async () => {
    const client = createClient();
    const api = createProviderCenterApi(client);

    await api.openCloneIntent("90e1e991-f93e-4e78-817e-d0ceeb06a749");

    expect(client.requests).toEqual([
      {
        method: "cloneIntents.open",
        params: { intentId: "90e1e991-f93e-4e78-817e-d0ceeb06a749" }
      }
    ]);
    expect(JSON.stringify(client.requests)).not.toContain("/clone/");
  });

  it("cancels an in-flight repository page with the same operation ID", async () => {
    const controller = new AbortController();
    let rejectList!: (error: unknown) => void;
    const client = createClient();
    client.request = vi.fn((method: string, params: Record<string, unknown>) => {
      client.requests.push({ method, params });
      if (method === "providers.listRepositories") {
        return new Promise((_resolve, reject) => {
          rejectList = reject;
        }) as never;
      }
      return Promise.resolve(null) as never;
    });
    const api = createProviderCenterApi(client);
    const promise = api.listRepositories({ accountId, query, cursor: null }, controller.signal);
    const listRequest = client.requests.find(
      ({ method }) => method === "providers.listRepositories"
    )!;
    controller.abort();
    await expect(promise).rejects.toMatchObject({ name: "AbortError" });
    expect(client.requests).toContainEqual({
      method: "providers.cancelOperation",
      params: { accountId, operationId: listRequest.params.operationId }
    });
    rejectList(new DOMException("Aborted", "AbortError"));
  });

  it("rejects an already aborted request without opening an RPC call", async () => {
    const controller = new AbortController();
    controller.abort();
    const client = createClient();
    const api = createProviderCenterApi(client);
    await expect(
      api.listRepositories({ accountId, query, cursor: null }, controller.signal)
    ).rejects.toMatchObject({
      name: "AbortError"
    });
    expect(client.request).not.toHaveBeenCalled();
  });

  it("routes every management and access method through its exact typed boundary", async () => {
    const client = createClient();
    const api = createProviderCenterApi(client);
    await api.createInstance({
      providerKind: "gitlab",
      displayName: "GitLab",
      baseUrl: "https://gitlab.example",
      customCaAction: "none"
    });
    await api.updateInstance({
      instanceId,
      displayName: "GitLab",
      baseUrl: "https://gitlab.example",
      customCaAction: "keep"
    });
    await api.validateInstance({ instanceId });
    await api.deleteInstance({ instanceId });
    await api.connectAccount(instanceId);
    await api.rotateAccount(accountId);
    await api.validateAccount(accountId);
    await api.setDefaultAccount(instanceId, accountId);
    await api.getAccountDeletionImpact(accountId);
    await api.deleteAccount({
      accountId,
      resolution: { kind: "unbind" },
      newDefaultAccountId: null
    });
    await api.requestReadAccess();
    await api.revokeReadAccess(accountId);
    await api.cancelOperation({ accountId, operationId: "f84223af-c753-4209-be36-12d381375fcb" });
    await api.bindRemote({
      repositoryId,
      remoteName: "origin",
      instanceId,
      accountId: null,
      providerRepositoryId: "4242"
    });
    await api.unbindRemote(repositoryId, "origin");

    expect(client.requests.map(({ method }) => method)).toEqual([
      "providers.createInstance",
      "providers.updateInstance",
      "providers.validateInstance",
      "providers.deleteInstance",
      "providers.connectAccount",
      "providers.rotateAccount",
      "providers.validateAccount",
      "providers.setDefaultAccount",
      "providers.getAccountDeletionImpact",
      "providers.deleteAccount",
      "providers.requestReadAccess",
      "providers.revokeReadAccess",
      "providers.cancelOperation",
      "providers.bindRemote",
      "providers.unbindRemote"
    ]);
    expect(JSON.stringify(client.requests)).not.toMatch(/pat|secretRef|customCaPath|[A-Z]:\\/iu);
  });

  it("creates a canonical operation ID without crypto.randomUUID", async () => {
    const originalCrypto = globalThis.crypto;
    vi.stubGlobal("crypto", {
      getRandomValues: originalCrypto.getRandomValues.bind(originalCrypto)
    });
    const client = createClient();
    await createProviderCenterApi(client).listRepositories({ accountId, query, cursor: null });
    const params = client.requests[0]!.params;
    expect(params.operationId).toMatch(
      /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/u
    );
  });

  it("rejects an overlong query asynchronously without opening an RPC call", async () => {
    const client = createClient();
    const api = createProviderCenterApi(client);
    const promise = api.listRepositories({
      accountId,
      query: { ...query, search: "x".repeat(257) },
      cursor: null
    });
    await expect(promise).rejects.toMatchObject({ code: "provider.unexpected-error" });
    expect(client.request).not.toHaveBeenCalled();
  });
});
