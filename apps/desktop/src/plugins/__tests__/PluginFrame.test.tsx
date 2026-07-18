import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type {
  ErrorEnvelope,
  PluginDescriptor,
  RpcRequest,
  ThemeDefinition
} from "@git-ramus/contracts";
import type { HostApi } from "../../lib/hostApi";
import { PluginFrame } from "../PluginFrame";
import { dispatchPluginRpc } from "../rpcRouter";

afterEach(() => {
  cleanup();
  vi.clearAllMocks();
});

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
  uiUrl: "http://git-ramus-plugin.localhost/git-ramus.welcome/ui.html"
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
  cancelJob: vi.fn(async () => undefined),
  listProjects: vi.fn(),
  updateProjectScanRules: vi.fn(),
  scanProject: vi.fn(),
  listWorkspaces: vi.fn(),
  createWorkspace: vi.fn(),
  updateWorkspaceMembership: vi.fn(),
  deleteWorkspace: vi.fn(),
  getOverview: vi.fn(),
  getRepositorySnapshot: vi.fn(),
  getRepositoryChanges: vi.fn(),
  getRepositoryDiff: vi.fn(),
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

describe("PluginFrame", () => {
  it("loads the host-served plugin document in an opaque-origin scripts-only sandbox", () => {
    render(<PluginFrame descriptor={descriptor} hostApi={hostApi} />);
    const frame = screen.getByTitle("Welcome plugin") as HTMLIFrameElement;
    expect(frame.getAttribute("sandbox")).toBe("allow-scripts");
    expect(frame.src).toBe("http://git-ramus-plugin.localhost/git-ramus.welcome/ui.html");
    expect(frame).toHaveAttribute("data-plugin-status", "loading");
  });

  it("delivers the exact selected route and the current validated theme for the session", async () => {
    const initialTheme: ThemeDefinition = {
      themeId: "git-ramus.default",
      colors: { background: "#111" }
    };
    const nextTheme: ThemeDefinition = {
      themeId: "git-ramus.compact",
      density: "compact"
    };
    const { rerender } = render(
      <PluginFrame
        descriptor={descriptor}
        hostApi={hostApi}
        route="/projects"
        theme={initialTheme}
      />
    );
    const frame = screen.getByTitle("Welcome plugin") as HTMLIFrameElement;
    const frameWindow = frame.contentWindow;
    if (frameWindow === null) {
      throw new Error("expected an iframe window");
    }
    const postMessage = vi.fn();
    Object.defineProperty(frameWindow, "postMessage", { configurable: true, value: postMessage });

    fireEvent.load(frame);
    const init = postMessage.mock.calls[0]?.[0] as { sessionId: string } | undefined;
    expect(init).toMatchObject({
      type: "host:init",
      pluginId: descriptor.manifest.id,
      route: "/projects"
    });
    expect(postMessage.mock.calls[1]?.[0]).toEqual({
      type: "host:theme-changed",
      sessionId: init?.sessionId,
      theme: initialTheme
    });

    rerender(
      <PluginFrame descriptor={descriptor} hostApi={hostApi} route="/projects" theme={nextTheme} />
    );
    await waitFor(() =>
      expect(postMessage).toHaveBeenLastCalledWith(
        {
          type: "host:theme-changed",
          sessionId: init?.sessionId,
          theme: nextTheme
        },
        "*"
      )
    );
  });

  it("never sends an invalid theme payload across the frame boundary", () => {
    const invalidTheme = {
      themeId: "bad",
      css: "body { background: url(https://evil.test) }"
    } as unknown as ThemeDefinition;
    render(
      <PluginFrame
        descriptor={descriptor}
        hostApi={hostApi}
        route="/projects"
        theme={invalidTheme}
      />
    );
    const frame = screen.getByTitle("Welcome plugin") as HTMLIFrameElement;
    const frameWindow = frame.contentWindow;
    if (frameWindow === null) {
      throw new Error("expected an iframe window");
    }
    const postMessage = vi.fn();
    Object.defineProperty(frameWindow, "postMessage", { configurable: true, value: postMessage });

    fireEvent.load(frame);
    expect(postMessage).toHaveBeenCalledTimes(1);
    expect(postMessage.mock.calls[0]?.[0]).toMatchObject({ type: "host:init", route: "/projects" });
  });

  it("reports the SDK handshake and completed RPC through the frame boundary", async () => {
    render(<PluginFrame descriptor={descriptor} hostApi={hostApi} />);
    const frame = screen.getByTitle("Welcome plugin") as HTMLIFrameElement;
    const frameWindow = frame.contentWindow;
    if (frameWindow === null) {
      throw new Error("expected an iframe window");
    }
    const postMessage = vi.fn();
    Object.defineProperty(frameWindow, "postMessage", {
      configurable: true,
      value: postMessage
    });

    fireEvent.load(frame);
    const init = postMessage.mock.calls[0]?.[0] as { sessionId: string } | undefined;
    if (init === undefined) {
      throw new Error("expected host:init message");
    }

    window.dispatchEvent(
      new MessageEvent("message", {
        data: { type: "plugin:ready", sessionId: init.sessionId },
        source: frameWindow
      })
    );
    await waitFor(() => expect(frame).toHaveAttribute("data-plugin-status", "ready"));

    window.dispatchEvent(
      new MessageEvent("message", {
        data: {
          type: "rpc:request",
          requestId: "87a31769-8aaa-47ca-bef3-47e66f0c62fc",
          sessionId: init.sessionId,
          method: "app.getInfo",
          params: {}
        },
        source: frameWindow
      })
    );
    await waitFor(() => expect(frame).toHaveAttribute("data-plugin-status", "rpc-complete"));
    expect(postMessage).toHaveBeenLastCalledWith(
      {
        type: "rpc:result",
        requestId: "87a31769-8aaa-47ca-bef3-47e66f0c62fc",
        sessionId: init.sessionId,
        ok: true,
        result: { name: "Git-Ramus", version: "0.1.0" }
      },
      "*"
    );
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
    await expect(
      dispatchPluginRpc("git-ramus.welcome", request, deniedHostApi)
    ).rejects.toMatchObject({
      code: "permission.denied",
      category: "userActionRequired",
      resourceId: "info"
    });
    expect(deniedHostApi.getAppInfo).not.toHaveBeenCalled();
  });

  it("rejects messages from another frame or another session", async () => {
    const isolatedHostApi = { ...hostApi, getAppInfo: vi.fn() };
    render(
      <>
        <PluginFrame descriptor={descriptor} hostApi={isolatedHostApi} route="/welcome" />
        <iframe title="foreign frame" />
      </>
    );
    const frame = screen.getByTitle("Welcome plugin") as HTMLIFrameElement;
    const foreignFrame = screen.getByTitle("foreign frame") as HTMLIFrameElement;
    const frameWindow = frame.contentWindow;
    if (frameWindow === null || foreignFrame.contentWindow === null) {
      throw new Error("expected iframe windows");
    }
    const postMessage = vi.fn();
    Object.defineProperty(frameWindow, "postMessage", { configurable: true, value: postMessage });
    fireEvent.load(frame);
    const init = postMessage.mock.calls[0]?.[0] as { sessionId: string };
    const rpc = {
      type: "rpc:request",
      requestId: "87a31769-8aaa-47ca-bef3-47e66f0c62fc",
      sessionId: init.sessionId,
      method: "app.getInfo",
      params: {}
    };

    window.dispatchEvent(
      new MessageEvent("message", { data: rpc, source: foreignFrame.contentWindow })
    );
    window.dispatchEvent(
      new MessageEvent("message", {
        data: { ...rpc, sessionId: "e3d622f1-f1f7-4f7e-8f18-3db8a1e6ffbe" },
        source: frameWindow
      })
    );
    await Promise.resolve();
    expect(isolatedHostApi.getAppInfo).not.toHaveBeenCalled();
  });

  it("preserves a valid Rust ErrorEnvelope in a failed RPC result", async () => {
    const rustError: ErrorEnvelope = {
      code: "git.trust-required",
      category: "userActionRequired",
      message: "Repository trust is required before this write",
      operationId: null,
      pluginId: null,
      resourceId: "a032bc9c-8759-45ac-856f-b76f9addb9d1",
      failedStep: null,
      retryable: false,
      retryAfterMs: null,
      recoveryActions: [],
      details: null
    };
    const failingHostApi: HostApi = {
      ...hostApi,
      stageRepository: vi.fn().mockRejectedValue(rustError)
    };
    const postMessage = await dispatchFrameRequest(failingHostApi, {
      method: "repositories.stage",
      params: {
        projectId: "87a31769-8aaa-47ca-bef3-47e66f0c62fc",
        repositoryId: "a032bc9c-8759-45ac-856f-b76f9addb9d1",
        paths: [],
        all: true
      }
    });
    await waitFor(() =>
      expect(postMessage).toHaveBeenLastCalledWith(
        expect.objectContaining({ ok: false, error: rustError }),
        "*"
      )
    );
  });

  it("redacts generic JavaScript errors behind a stable plugin envelope", async () => {
    const failingHostApi: HostApi = {
      ...hostApi,
      getAppInfo: vi.fn().mockRejectedValue(new Error("C:/secret/repository credential=token"))
    };
    const postMessage = await dispatchFrameRequest(failingHostApi, {
      method: "app.getInfo",
      params: {}
    });
    await waitFor(() =>
      expect(postMessage).toHaveBeenLastCalledWith(
        expect.objectContaining({
          ok: false,
          error: expect.objectContaining({
            code: "plugin.rpc-failed",
            message: "Plugin RPC failed",
            details: null
          })
        }),
        "*"
      )
    );
    expect(JSON.stringify(postMessage.mock.calls.at(-1))).not.toContain("secret/repository");
  });
});

async function dispatchFrameRequest(
  api: HostApi,
  input: { method: string; params: unknown }
): Promise<ReturnType<typeof vi.fn>> {
  render(<PluginFrame descriptor={descriptor} hostApi={api} route="/projects" />);
  const frame = screen.getByTitle("Welcome plugin") as HTMLIFrameElement;
  const frameWindow = frame.contentWindow;
  if (frameWindow === null) {
    throw new Error("expected an iframe window");
  }
  const postMessage = vi.fn();
  Object.defineProperty(frameWindow, "postMessage", { configurable: true, value: postMessage });
  fireEvent.load(frame);
  const init = postMessage.mock.calls[0]?.[0] as { sessionId: string };
  window.dispatchEvent(
    new MessageEvent("message", {
      data: {
        type: "rpc:request",
        requestId: "87a31769-8aaa-47ca-bef3-47e66f0c62fc",
        sessionId: init.sessionId,
        method: input.method,
        params: input.params
      },
      source: frameWindow
    })
  );
  return postMessage;
}
