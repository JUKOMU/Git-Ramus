import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { PluginDescriptor, RpcRequest } from "@git-ramus/contracts";
import type { HostApi } from "../../lib/hostApi";
import { PluginFrame } from "../PluginFrame";
import { dispatchPluginRpc } from "../rpcRouter";
import { buildSandboxDocument } from "../sandboxDocument";

const descriptor: PluginDescriptor = {
  manifest: {
    schemaVersion: 1,
    id: "git-ramus.welcome",
    name: "Welcome",
    version: "0.1.0",
    publisher: "git-ramus",
    description: "Welcome plugin",
    kind: "builtin",
    sdkVersion: "^0.1.0",
    entrypoints: { ui: "ui.html" },
    contributions: { navigation: [] },
    permissions: [
      { capability: "app:read", resources: ["info"] },
      { capability: "tasks:create", resources: ["echo"] }
    ]
  },
  uiHtml: "<!doctype html><html><head></head><body><h1>Plugin</h1></body></html>"
};

const hostApi: HostApi = {
  getAppInfo: vi.fn(async () => ({ name: "Git-Ramus", version: "0.1.0" })),
  listPlugins: vi.fn(async () => []),
  listJobs: vi.fn(async () => []),
  authorizePluginCall: vi.fn(async () => ({ allowed: true })),
  startEchoJob: vi.fn(async () => ({
    id: "a032bc9c-8759-45ac-856f-b76f9addb9d1",
    kind: "system.echo",
    title: "Echo hello",
    status: "queued" as const,
    progress: 0,
    cancelRequested: false,
    createdAt: "2026-07-17T00:00:00Z",
    updatedAt: "2026-07-17T00:00:00Z",
    error: null
  })),
  cancelJob: vi.fn(async () => undefined)
};

describe("PluginFrame", () => {
  it("uses an opaque-origin scripts-only sandbox and injects a network-denying CSP", () => {
    render(<PluginFrame descriptor={descriptor} hostApi={hostApi} />);
    const frame = screen.getByTitle("Welcome plugin") as HTMLIFrameElement;
    expect(frame.getAttribute("sandbox")).toBe("allow-scripts");
    expect(frame.src).toMatch(/^data:text\/html;charset=utf-8,/u);
    const [, encodedDocument = ""] = frame.src.split(",", 2);
    const sandboxDocument = decodeURIComponent(encodedDocument);
    expect(sandboxDocument).toContain("default-src 'none'");
    expect(sandboxDocument).toContain("connect-src 'none'");
  });

  it("authorizes a route before calling its handler", async () => {
    const request: RpcRequest = {
      type: "rpc:request",
      requestId: "87a31769-8aaa-47ca-bef3-47e66f0c62fc",
      sessionId: "e3d622f1-f1f7-4f7e-8f18-3db8a1e6ffbe",
      method: "app.getInfo",
      params: {}
    };
    await expect(dispatchPluginRpc("git-ramus.welcome", request, hostApi)).resolves.toEqual({
      name: "Git-Ramus",
      version: "0.1.0"
    });
    expect(hostApi.authorizePluginCall).toHaveBeenCalledWith({
      pluginId: "git-ramus.welcome",
      capability: "app:read",
      resource: "info"
    });
    const [authorizationOrder] = vi.mocked(hostApi.authorizePluginCall).mock.invocationCallOrder;
    const [handlerOrder] = vi.mocked(hostApi.getAppInfo).mock.invocationCallOrder;
    if (authorizationOrder === undefined || handlerOrder === undefined) {
      throw new Error("expected authorization and handler calls");
    }
    expect(authorizationOrder).toBeLessThan(handlerOrder);
  });

  it("never calls a handler after the host denies permission", async () => {
    const deniedHostApi: HostApi = {
      ...hostApi,
      getAppInfo: vi.fn(async () => ({ name: "Git-Ramus", version: "0.1.0" })),
      authorizePluginCall: vi.fn(async () => ({ allowed: false }))
    };
    const request: RpcRequest = {
      type: "rpc:request",
      requestId: "d23957ac-5c0f-4857-9124-7f1599a41f33",
      sessionId: "c8f98df3-e949-48e0-a9ad-407fe371a94a",
      method: "app.getInfo",
      params: {}
    };
    await expect(dispatchPluginRpc("git-ramus.welcome", request, deniedHostApi)).rejects.toThrow(
      "Permission denied: app:read/info"
    );
    expect(deniedHostApi.getAppInfo).not.toHaveBeenCalled();
  });

  it("places the CSP before any untrusted plugin markup", () => {
    const document = buildSandboxDocument(
      '<head data-value=">"><script>window.evil = true</script>'
    );
    expect(document.indexOf("Content-Security-Policy")).toBeGreaterThanOrEqual(0);
    expect(document.indexOf("Content-Security-Policy")).toBeLessThan(document.indexOf("<script>"));
  });
});
