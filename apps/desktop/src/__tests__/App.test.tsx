import type { PluginDescriptor, ThemeCatalog, ThemeState } from "@git-ramus/contracts";
import { act, cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { App } from "../App";
import type { HostApi } from "../lib/hostApi";
import { tauriHostApi } from "../lib/hostApi";

const { listen } = vi.hoisted(() => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));

const defaultState: ThemeState = {
  activeThemeId: "git-ramus.theme.default",
  theme: {
    themeId: "git-ramus.theme.default",
    name: "Git-Ramus Default",
    colors: { background: "#080d18", surface: "#111827", text: "#e2e8f0" },
    spacing: { sm: 8, md: 12 },
    shape: { radius: 8 },
    density: "comfortable"
  }
};

const compactState: ThemeState = {
  activeThemeId: "git-ramus.theme.compact",
  theme: {
    themeId: "git-ramus.theme.compact",
    name: "Compact",
    colors: { background: "#07111f", surface: "#0d1b2a", text: "#e6f2ff" },
    spacing: { sm: 6, md: 10 },
    shape: { radius: 5 },
    density: "compact"
  }
};

const lightState: ThemeState = {
  activeThemeId: "git-ramus.theme.light",
  theme: {
    themeId: "git-ramus.theme.light",
    name: "Light",
    colors: { background: "#ffffff", surface: "#f8fafc", text: "#111827" },
    density: "comfortable"
  }
};

const catalog: ThemeCatalog = {
  themes: [
    {
      themeId: defaultState.activeThemeId,
      name: "Git-Ramus Default",
      pluginId: "git-ramus.host",
      version: "0.1.0",
      density: "comfortable"
    },
    {
      themeId: compactState.activeThemeId,
      name: "Compact",
      pluginId: "git-ramus.compact-theme",
      version: "0.1.0",
      density: "compact"
    },
    {
      themeId: lightState.activeThemeId,
      name: "Light",
      pluginId: "git-ramus.light-theme",
      version: "0.1.0",
      density: "comfortable"
    }
  ]
};

const gitClient: PluginDescriptor = {
  manifest: {
    schemaVersion: 1,
    id: "git-ramus.git-client",
    name: "Git Client",
    version: "0.1.0",
    publisher: "git-ramus",
    description: "Git client",
    kind: "builtin",
    sdkVersion: "^0.1.0",
    entrypoints: { ui: "ui.html" },
    contributions: {
      navigation: [{ id: "overview", label: "Overview", route: "/overview", icon: "grid" }]
    },
    permissions: []
  },
  uiUrl: "http://git-ramus-plugin.localhost/git-ramus.git-client/ui.html"
};

function createHostApi(plugins: PluginDescriptor[] = []): HostApi {
  return {
    getAppInfo: vi.fn(async () => ({ name: "Git-Ramus", version: "0.1.0" })),
    listPlugins: vi.fn(async () => plugins),
    listJobs: vi.fn(async () => []),
    listThemes: vi.fn(async () => catalog),
    currentTheme: vi.fn(async () => defaultState),
    activateTheme: vi.fn(async ({ themeId }) =>
      themeId === compactState.activeThemeId ? compactState : defaultState
    ),
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
    listProjects: vi.fn(async () => ({ projects: [] })),
    updateProjectScanRules: vi.fn(),
    scanProject: vi.fn(),
    listWorkspaces: vi.fn(async () => ({ workspaces: [] })),
    createWorkspace: vi.fn(),
    getWorkspaceMembership: vi.fn(async () => []),
    updateWorkspaceMembership: vi.fn(async () => []),
    deleteWorkspace: vi.fn(async () => undefined),
    getOverview: vi.fn(),
    getRepositorySnapshot: vi.fn(),
    getRepositoryChanges: vi.fn(),
    getRepositoryDiff: vi.fn(),
    getRepositoryTrustStatus: vi.fn(),
    stageRepository: vi.fn(),
    unstageRepository: vi.fn(),
    commitRepository: vi.fn(),
    trustRepository: vi.fn(),
    listIdentities: vi.fn(async () => ({ identities: [], globalIdentityProfileId: null })),
    createIdentity: vi.fn(),
    updateIdentity: vi.fn(),
    deleteIdentity: vi.fn(async () => undefined),
    setGlobalIdentity: vi.fn(),
    bindRepositoryIdentity: vi.fn(),
    unbindRepositoryIdentity: vi.fn(async () => undefined),
    getEffectiveRepositoryIdentity: vi.fn()
  };
}

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  listen.mockReset();
});

describe("App", () => {
  it("renders the trusted shell and host version", async () => {
    render(<App hostApi={createHostApi()} />);
    expect(screen.getByRole("heading", { name: "Git-Ramus" })).toBeInTheDocument();
    expect(await screen.findByText("Host 0.1.0")).toBeInTheDocument();
    expect(screen.getByRole("navigation", { name: "Primary" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Tasks" })).toBeInTheDocument();
  });

  it("does not render hardcoded no-op navigation entries", async () => {
    render(<App hostApi={createHostApi()} />);
    expect(await screen.findByText("Host 0.1.0")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Overview" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Projects" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Workspaces" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Plugins" })).not.toBeInTheDocument();
  });

  it("applies validated Shell tokens and switches a mounted plugin without reloading it", async () => {
    const hostApi = createHostApi([gitClient]);
    render(<App hostApi={hostApi} />);
    fireEvent.click(await screen.findByRole("button", { name: "Overview" }));
    const frame = await screen.findByTitle("Git Client plugin");
    const frameWindow = (frame as HTMLIFrameElement).contentWindow;
    if (frameWindow === null) throw new Error("expected iframe window");
    const postMessage = vi.fn();
    Object.defineProperty(frameWindow, "postMessage", { configurable: true, value: postMessage });
    fireEvent.load(frame);

    const shell = screen.getByTestId("app-shell");
    expect(shell).toHaveAttribute("data-theme-id", defaultState.activeThemeId);
    expect(shell).toHaveClass("density-comfortable");
    expect(shell.style.getPropertyValue("--gr-colors-background")).toBe("#080d18");
    expect(postMessage.mock.calls.map((call) => call[0])).toContainEqual(
      expect.objectContaining({ type: "host:theme-changed", theme: defaultState.theme })
    );

    fireEvent.change(screen.getByRole("combobox", { name: "Theme" }), {
      target: { value: compactState.activeThemeId }
    });

    await waitFor(() => expect(shell).toHaveAttribute("data-theme-id", compactState.activeThemeId));
    expect(shell).toHaveClass("density-compact");
    expect(shell.style.getPropertyValue("--gr-colors-background")).toBe("#07111f");
    expect(hostApi.activateTheme).toHaveBeenCalledWith({ themeId: compactState.activeThemeId });
    expect(screen.getByTitle("Git Client plugin")).toBe(frame);
    expect(postMessage.mock.calls.filter((call) => call[0]?.type === "host:init")).toHaveLength(1);
    expect(postMessage).toHaveBeenLastCalledWith(
      expect.objectContaining({ type: "host:theme-changed", theme: compactState.theme }),
      "*"
    );
  });

  it("serializes rapid theme activations without rendering a stale response", async () => {
    const first = deferred<ThemeState>();
    const second = deferred<ThemeState>();
    const hostApi = createHostApi([gitClient]);
    hostApi.activateTheme = vi.fn(({ themeId }) => {
      if (themeId === compactState.activeThemeId) return first.promise;
      if (themeId === lightState.activeThemeId) return second.promise;
      throw new Error(`unexpected theme ${themeId}`);
    });
    render(<App hostApi={hostApi} />);
    fireEvent.click(await screen.findByRole("button", { name: "Overview" }));
    const frame = await screen.findByTitle("Git Client plugin");
    const frameWindow = (frame as HTMLIFrameElement).contentWindow;
    if (frameWindow === null) throw new Error("expected iframe window");
    const postMessage = vi.fn();
    Object.defineProperty(frameWindow, "postMessage", { configurable: true, value: postMessage });
    fireEvent.load(frame);
    const shell = screen.getByTestId("app-shell");
    const selector = screen.getByRole("combobox", { name: "Theme" });

    act(() => {
      fireEvent.change(selector, { target: { value: compactState.activeThemeId } });
      fireEvent.change(selector, { target: { value: lightState.activeThemeId } });
    });

    await waitFor(() => expect(hostApi.activateTheme).toHaveBeenCalledTimes(1));
    expect(hostApi.activateTheme).toHaveBeenNthCalledWith(1, {
      themeId: compactState.activeThemeId
    });
    expect(selector).toBeDisabled();
    expect(selector).toHaveAttribute("aria-busy", "true");

    await act(async () => {
      first.resolve(compactState);
      await first.promise;
    });

    await waitFor(() =>
      expect(hostApi.activateTheme).toHaveBeenNthCalledWith(2, {
        themeId: lightState.activeThemeId
      })
    );
    expect(shell).toHaveAttribute("data-theme-id", defaultState.activeThemeId);
    expect(postMessage.mock.calls.map((call) => call[0])).not.toContainEqual(
      expect.objectContaining({ type: "host:theme-changed", theme: compactState.theme })
    );

    await act(async () => {
      second.resolve(lightState);
      await second.promise;
    });

    await waitFor(() => expect(shell).toHaveAttribute("data-theme-id", lightState.activeThemeId));
    expect(selector).not.toBeDisabled();
    expect(selector).toHaveAttribute("aria-busy", "false");
    expect(postMessage).toHaveBeenLastCalledWith(
      expect.objectContaining({ type: "host:theme-changed", theme: lightState.theme }),
      "*"
    );
  });

  it("continues queued theme activations after an earlier request fails", async () => {
    const first = deferred<ThemeState>();
    const second = deferred<ThemeState>();
    const hostApi = createHostApi();
    hostApi.activateTheme = vi.fn(({ themeId }) => {
      if (themeId === compactState.activeThemeId) return first.promise;
      if (themeId === lightState.activeThemeId) return second.promise;
      throw new Error(`unexpected theme ${themeId}`);
    });
    render(<App hostApi={hostApi} />);
    const selector = await screen.findByRole("combobox", { name: "Theme" });
    const shell = screen.getByTestId("app-shell");

    act(() => {
      fireEvent.change(selector, { target: { value: compactState.activeThemeId } });
      fireEvent.change(selector, { target: { value: lightState.activeThemeId } });
    });
    await waitFor(() => expect(hostApi.activateTheme).toHaveBeenCalledTimes(1));

    await act(async () => {
      first.reject(new Error("activation failed"));
      await first.promise.catch(() => undefined);
    });

    await waitFor(() =>
      expect(hostApi.activateTheme).toHaveBeenNthCalledWith(2, {
        themeId: lightState.activeThemeId
      })
    );
    expect(shell).toHaveAttribute("data-theme-id", defaultState.activeThemeId);

    await act(async () => {
      second.resolve(lightState);
      await second.promise;
    });
    await waitFor(() => expect(shell).toHaveAttribute("data-theme-id", lightState.activeThemeId));
    expect(selector).not.toBeDisabled();
  });

  it("reconciles the authoritative theme when the latest queued activation fails", async () => {
    const first = deferred<ThemeState>();
    const second = deferred<ThemeState>();
    const hostApi = createHostApi([gitClient]);
    hostApi.currentTheme = vi
      .fn()
      .mockResolvedValueOnce(defaultState)
      .mockResolvedValueOnce(compactState);
    hostApi.activateTheme = vi.fn(({ themeId }) => {
      if (themeId === compactState.activeThemeId) return first.promise;
      if (themeId === lightState.activeThemeId) return second.promise;
      throw new Error(`unexpected theme ${themeId}`);
    });
    render(<App hostApi={hostApi} />);
    fireEvent.click(await screen.findByRole("button", { name: "Overview" }));
    const frame = await screen.findByTitle("Git Client plugin");
    const frameWindow = (frame as HTMLIFrameElement).contentWindow;
    if (frameWindow === null) throw new Error("expected iframe window");
    const postMessage = vi.fn();
    Object.defineProperty(frameWindow, "postMessage", { configurable: true, value: postMessage });
    fireEvent.load(frame);
    const shell = screen.getByTestId("app-shell");
    const selector = screen.getByRole("combobox", { name: "Theme" });

    act(() => {
      fireEvent.change(selector, { target: { value: compactState.activeThemeId } });
      fireEvent.change(selector, { target: { value: lightState.activeThemeId } });
    });
    await waitFor(() => expect(hostApi.activateTheme).toHaveBeenCalledTimes(1));

    await act(async () => {
      first.resolve(compactState);
      await first.promise;
    });
    await waitFor(() => expect(hostApi.activateTheme).toHaveBeenCalledTimes(2));
    expect(shell).toHaveAttribute("data-theme-id", defaultState.activeThemeId);
    expect(postMessage.mock.calls.map((call) => call[0])).not.toContainEqual(
      expect.objectContaining({ type: "host:theme-changed", theme: compactState.theme })
    );

    await act(async () => {
      second.reject(new Error("latest activation failed"));
      await second.promise.catch(() => undefined);
    });

    await waitFor(() => expect(hostApi.currentTheme).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(shell).toHaveAttribute("data-theme-id", compactState.activeThemeId));
    expect(selector).not.toBeDisabled();
    expect(selector).toHaveAttribute("aria-busy", "false");
    expect(postMessage).toHaveBeenLastCalledWith(
      expect.objectContaining({ type: "host:theme-changed", theme: compactState.theme }),
      "*"
    );
  });

  it("does not apply an obsolete reconciliation when another activation is queued", async () => {
    const first = deferred<ThemeState>();
    const second = deferred<ThemeState>();
    const third = deferred<ThemeState>();
    const reconciliation = deferred<ThemeState>();
    const hostApi = createHostApi();
    hostApi.currentTheme = vi
      .fn()
      .mockResolvedValueOnce(defaultState)
      .mockReturnValueOnce(reconciliation.promise);
    hostApi.activateTheme = vi
      .fn()
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise)
      .mockReturnValueOnce(third.promise);
    render(<App hostApi={hostApi} />);
    const shell = await screen.findByTestId("app-shell");
    const selector = screen.getByRole("combobox", { name: "Theme" });

    act(() => {
      fireEvent.change(selector, { target: { value: compactState.activeThemeId } });
      fireEvent.change(selector, { target: { value: lightState.activeThemeId } });
    });
    await waitFor(() => expect(hostApi.activateTheme).toHaveBeenCalledTimes(1));
    await act(async () => {
      first.resolve(compactState);
      await first.promise;
    });
    await waitFor(() => expect(hostApi.activateTheme).toHaveBeenCalledTimes(2));

    await act(async () => {
      second.reject(new Error("latest activation failed"));
      await second.promise.catch(() => undefined);
    });
    await waitFor(() => expect(hostApi.currentTheme).toHaveBeenCalledTimes(2));
    expect(selector).toBeDisabled();

    fireEvent.change(selector, { target: { value: lightState.activeThemeId } });
    expect(hostApi.activateTheme).toHaveBeenCalledTimes(2);

    await act(async () => {
      reconciliation.resolve(compactState);
      await reconciliation.promise;
    });
    await waitFor(() => expect(hostApi.activateTheme).toHaveBeenCalledTimes(3));

    expect(shell).toHaveAttribute("data-theme-id", defaultState.activeThemeId);

    await act(async () => {
      third.resolve(lightState);
      await third.promise;
    });
    await waitFor(() => expect(shell).toHaveAttribute("data-theme-id", lightState.activeThemeId));
    expect(selector).not.toBeDisabled();
  });

  it("continues the activation queue when authoritative reconciliation fails", async () => {
    const failedActivation = deferred<ThemeState>();
    const nextActivation = deferred<ThemeState>();
    const reconciliation = deferred<ThemeState>();
    const hostApi = createHostApi();
    hostApi.currentTheme = vi
      .fn()
      .mockResolvedValueOnce(defaultState)
      .mockReturnValueOnce(reconciliation.promise);
    hostApi.activateTheme = vi
      .fn()
      .mockReturnValueOnce(failedActivation.promise)
      .mockReturnValueOnce(nextActivation.promise);
    render(<App hostApi={hostApi} />);
    const shell = await screen.findByTestId("app-shell");
    const selector = screen.getByRole("combobox", { name: "Theme" });

    fireEvent.change(selector, { target: { value: lightState.activeThemeId } });
    await waitFor(() => expect(hostApi.activateTheme).toHaveBeenCalledTimes(1));
    await act(async () => {
      failedActivation.reject(new Error("activation failed"));
      await failedActivation.promise.catch(() => undefined);
    });
    await waitFor(() => expect(hostApi.currentTheme).toHaveBeenCalledTimes(2));

    await act(async () => {
      reconciliation.reject(new Error("reconciliation failed"));
      await reconciliation.promise.catch(() => undefined);
    });
    await waitFor(() => expect(selector).not.toBeDisabled());
    expect(selector).toHaveAttribute("aria-busy", "false");

    fireEvent.change(selector, { target: { value: compactState.activeThemeId } });
    await waitFor(() => expect(hostApi.activateTheme).toHaveBeenCalledTimes(2));
    await act(async () => {
      nextActivation.resolve(compactState);
      await nextActivation.promise;
    });

    await waitFor(() => expect(shell).toHaveAttribute("data-theme-id", compactState.activeThemeId));
    expect(selector).not.toBeDisabled();
    expect(selector).toHaveAttribute("aria-busy", "false");
  });

  it("does not run queued theme work after unmount", async () => {
    const first = deferred<ThemeState>();
    const second = deferred<ThemeState>();
    const hostApi = createHostApi();
    hostApi.activateTheme = vi.fn(({ themeId }) => {
      if (themeId === compactState.activeThemeId) return first.promise;
      if (themeId === lightState.activeThemeId) return second.promise;
      throw new Error(`unexpected theme ${themeId}`);
    });
    const rendered = render(<App hostApi={hostApi} />);
    const selector = await screen.findByRole("combobox", { name: "Theme" });

    act(() => {
      fireEvent.change(selector, { target: { value: compactState.activeThemeId } });
      fireEvent.change(selector, { target: { value: lightState.activeThemeId } });
    });
    await waitFor(() => expect(hostApi.activateTheme).toHaveBeenCalledTimes(1));
    rendered.unmount();

    await act(async () => {
      first.resolve(compactState);
      await first.promise;
      await Promise.resolve();
    });

    expect(hostApi.activateTheme).toHaveBeenCalledTimes(1);
  });

  it("listens for native theme changes and cleans up both Tauri subscriptions", async () => {
    let onThemeChanged: ((event: { payload: ThemeState }) => void) | undefined;
    const disposeJobs = vi.fn();
    const disposeTheme = vi.fn();
    listen.mockImplementation(async (event: string, listener: (event: never) => void) => {
      if (event === "theme://changed") {
        onThemeChanged = listener as (event: { payload: ThemeState }) => void;
        return disposeTheme;
      }
      return disposeJobs;
    });
    vi.spyOn(tauriHostApi, "getAppInfo").mockResolvedValue({ name: "Git-Ramus", version: "0.1.0" });
    vi.spyOn(tauriHostApi, "listPlugins").mockResolvedValue([]);
    vi.spyOn(tauriHostApi, "listJobs").mockResolvedValue([]);
    vi.spyOn(tauriHostApi, "listThemes").mockResolvedValue(catalog);
    vi.spyOn(tauriHostApi, "currentTheme").mockResolvedValue(defaultState);

    const rendered = render(<App />);
    const shell = await screen.findByTestId("app-shell");
    await waitFor(() =>
      expect(listen).toHaveBeenCalledWith("theme://changed", expect.any(Function))
    );
    onThemeChanged?.({ payload: compactState });
    await waitFor(() => expect(shell).toHaveAttribute("data-theme-id", compactState.activeThemeId));

    rendered.unmount();
    await waitFor(() => {
      expect(disposeJobs).toHaveBeenCalledTimes(1);
      expect(disposeTheme).toHaveBeenCalledTimes(1);
    });
  });
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}
