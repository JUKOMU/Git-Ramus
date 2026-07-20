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

function createClient() {
  const requests: Array<{ method: string; params: Record<string, unknown> }> = [];
  const client = {
    requests,
    request: vi.fn(async (method: string, params: Record<string, unknown>) => {
      requests.push({ method, params });
      if (method === "providers.listRepositories")
        return { items: [], nextCursor: null, hasMore: false, rateLimit: null };
      if (method === "providers.matchLocalRemotes") return { items: [] };
      return { items: [] };
    })
  } as unknown as PluginClient & {
    requests: Array<{ method: string; params: Record<string, unknown> }>;
  };
  return client;
}

describe("Provider Center API", () => {
  afterEach(() => vi.restoreAllMocks());

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
});
