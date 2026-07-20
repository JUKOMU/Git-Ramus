import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { HostApi } from "../lib/hostApi";
import { App } from "../App";

afterEach(cleanup);

const plugin = {
  manifest: {
    schemaVersion: 1 as const,
    id: "git-ramus.welcome",
    name: "Welcome",
    version: "0.1.0",
    publisher: "git-ramus",
    description: "Welcome plugin",
    kind: "builtin" as const,
    sdkVersion: "^0.1.0",
    entrypoints: { ui: "ui.html" },
    contributions: {
      navigation: [
        { id: "welcome", label: "Welcome", route: "/welcome", icon: "sparkles" },
        { id: "projects", label: "Plugin Projects", route: "/projects", icon: "folder" }
      ],
      providers: []
    },
    permissions: [
      { capability: "app:read", resources: ["info"] },
      { capability: "tasks:create", resources: ["echo"] }
    ]
  },
  uiUrl: "http://git-ramus-plugin.localhost/git-ramus.welcome/ui.html"
};

describe("foundation flow", () => {
  it("adds bundled navigation, opens the sandbox, and shows persistent tasks", async () => {
    const user = userEvent.setup();
    const hostApi = createHostApi();
    render(<App hostApi={hostApi} />);
    await user.click(await screen.findByRole("button", { name: "Welcome" }));
    expect(screen.getByTitle("Welcome plugin")).toHaveAttribute("sandbox", "allow-scripts");
    expect(await screen.findByText("Echo hello")).toBeInTheDocument();
    expect(screen.getByText("50%")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(hostApi.cancelJob).toHaveBeenCalledWith("a032bc9c-8759-45ac-856f-b76f9addb9d1");
  });

  it("tracks pluginId plus contribution route and remounts the same plugin for a new route", async () => {
    const user = userEvent.setup();
    const hostApi = createHostApi();
    render(<App hostApi={hostApi} />);

    const welcome = await screen.findByRole("button", { name: "Welcome" });
    const projects = screen.getByRole("button", { name: "Plugin Projects" });
    await user.click(welcome);
    const welcomeFrame = screen.getByTitle("Welcome plugin") as HTMLIFrameElement;
    const welcomePostMessage = vi.fn();
    if (welcomeFrame.contentWindow === null) {
      throw new Error("expected welcome iframe window");
    }
    Object.defineProperty(welcomeFrame.contentWindow, "postMessage", {
      configurable: true,
      value: welcomePostMessage
    });
    fireEvent.load(welcomeFrame);
    const welcomeInit = welcomePostMessage.mock.calls[0]?.[0] as {
      sessionId: string;
      route: string;
    };
    expect(welcomeInit.route).toBe("/welcome");
    expect(welcome).toHaveAttribute("aria-pressed", "true");

    await user.click(projects);
    const projectsFrame = screen.getByTitle("Welcome plugin") as HTMLIFrameElement;
    expect(projectsFrame).not.toBe(welcomeFrame);
    const projectsPostMessage = vi.fn();
    if (projectsFrame.contentWindow === null) {
      throw new Error("expected projects iframe window");
    }
    Object.defineProperty(projectsFrame.contentWindow, "postMessage", {
      configurable: true,
      value: projectsPostMessage
    });
    fireEvent.load(projectsFrame);
    const projectsInit = projectsPostMessage.mock.calls[0]?.[0] as {
      sessionId: string;
      route: string;
    };
    expect(projectsInit.route).toBe("/projects");
    expect(projectsInit.sessionId).not.toBe(welcomeInit.sessionId);
    expect(welcome).toHaveAttribute("aria-pressed", "false");
    expect(projects).toHaveAttribute("aria-pressed", "true");
  });
});

function createHostApi(): HostApi {
  return {
    getAppInfo: vi.fn(async () => ({ name: "Git-Ramus", version: "0.1.0" })),
    listPlugins: vi.fn(async () => [plugin]),
    listJobs: vi.fn(async () => [
      {
        id: "a032bc9c-8759-45ac-856f-b76f9addb9d1",
        kind: "system.echo",
        title: "Echo hello",
        status: "running" as const,
        progress: 0.5,
        cancelRequested: false,
        createdAt: "2026-07-17T00:00:00Z",
        updatedAt: "2026-07-17T00:00:01Z",
        error: null
      }
    ]),
    listThemes: vi.fn(async () => ({
      themes: [
        {
          themeId: "git-ramus.theme.default",
          name: "Git-Ramus Default",
          pluginId: "git-ramus.host",
          version: "0.1.0",
          density: "comfortable" as const
        }
      ]
    })),
    currentTheme: vi.fn(async () => ({
      activeThemeId: "git-ramus.theme.default",
      theme: { themeId: "git-ramus.theme.default", density: "comfortable" as const }
    })),
    activateTheme: vi.fn(async () => ({
      activeThemeId: "git-ramus.theme.default",
      theme: { themeId: "git-ramus.theme.default", density: "comfortable" as const }
    })),
    authorizePluginCall: vi.fn(async () => ({ allowed: true })),
    authorizePluginPermissionRequest: vi.fn(async () => ({ allowed: true })),
    startEchoJob: vi.fn(),
    cancelJob: vi.fn(async () => undefined),
    listProjects: vi.fn(),
    createProject: vi.fn(),
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
    getEffectiveRepositoryIdentity: vi.fn(),
    listProviderInstances: vi.fn(),
    createProviderInstance: vi.fn(),
    updateProviderInstance: vi.fn(),
    validateProviderInstance: vi.fn(),
    deleteProviderInstance: vi.fn(),
    listProviderAccounts: vi.fn(),
    connectProviderAccount: vi.fn(),
    rotateProviderAccount: vi.fn(),
    validateProviderAccount: vi.fn(),
    setDefaultProviderAccount: vi.fn(),
    getProviderAccountDeletionImpact: vi.fn(),
    deleteProviderAccount: vi.fn(),
    listAuthorizedProviderAccounts: vi.fn(),
    requestProviderReadAccess: vi.fn(),
    revokeProviderReadAccess: vi.fn(),
    listProviderRepositories: vi.fn(),
    cancelProviderOperation: vi.fn(),
    matchLocalProviderRemotes: vi.fn(),
    listProviderBindings: vi.fn(),
    bindProviderRemote: vi.fn(),
    unbindProviderRemote: vi.fn(),
    listTransportProfiles: vi.fn(),
    createTransportProfile: vi.fn(),
    updateTransportProfile: vi.fn(),
    getTransportProfileDeletionImpact: vi.fn(),
    deleteTransportProfile: vi.fn(),
    getEffectiveRepositoryTransport: vi.fn(),
    getRepositoryNetworkState: vi.fn(),
    bindRepositoryTransport: vi.fn(),
    unbindRepositoryTransport: vi.fn(),
    createCloneIntent: vi.fn(),
    getCloneIntent: vi.fn(),
    cloneRepository: vi.fn(),
    fetchRepository: vi.fn(),
    pullRepository: vi.fn(),
    pushRepository: vi.fn(),
    cancelTransportOperation: vi.fn()
  };
}
