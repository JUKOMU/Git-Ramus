import "@testing-library/jest-dom/vitest";
import { act, cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { GitClientApi } from "../api";
import { App } from "../App";
import { OverviewView, type OverviewApi } from "../views/OverviewView";
import { ProjectsView, type ProjectsApi } from "../views/ProjectsView";
import { WorkspacesView, type WorkspacesApi } from "../views/WorkspacesView";

const projectId = "87a31769-8aaa-47ca-bef3-47e66f0c62fc";
const workspaceId = "e3d622f1-f1f7-4f7e-8f18-3db8a1e6ffbe";
const alphaId = "a032bc9c-8759-45ac-856f-b76f9addb9d1";
const betaId = "f9479c0b-8770-4ec0-8a34-fd8d327b43e9";

const project = {
  id: projectId,
  rootPath: "C:/work/demo",
  name: "Demo",
  scanDepth: 3,
  excludePatterns: ["node_modules"],
  createdAt: "2026-07-17T00:00:00Z",
  updatedAt: "2026-07-17T00:00:00Z"
};

const betaProject = {
  ...project,
  id: "c8f98df3-e949-48e0-a9ad-407fe371a94a",
  rootPath: "D:/work/beta",
  name: "Beta"
};

const workspace = {
  id: workspaceId,
  name: "Shared",
  createdAt: "2026-07-17T00:00:00Z",
  updatedAt: "2026-07-17T00:00:00Z"
};

const alphaRepository = {
  id: alphaId,
  canonicalPath: "C:/work/demo/alpha",
  displayName: "Alpha",
  kind: "normal" as const,
  createdAt: "2026-07-17T00:00:00Z",
  updatedAt: "2026-07-17T00:00:00Z"
};

const betaRepository = {
  ...alphaRepository,
  id: betaId,
  canonicalPath: "C:/work/demo/beta",
  displayName: "Beta"
};

function snapshot(repositoryId: string, branch: string, dirty: boolean) {
  return {
    id:
      repositoryId === alphaId
        ? "5d497627-6613-4273-99e3-2f59c20d121f"
        : "6a8ac5c5-1914-4b20-a8bc-df90ee8a22f0",
    repositoryId,
    capturedAt: "2026-07-17T00:00:00Z",
    headOid: "abc123",
    branch,
    upstream: `origin/${branch}`,
    ahead: 0,
    behind: 0,
    dirty,
    stagedCount: dirty ? 1 : 0,
    unstagedCount: 0,
    untrackedCount: 0,
    conflictedCount: 0,
    refreshErrorSummary: null
  };
}

afterEach(cleanup);

describe("OverviewView", () => {
  it("shows loading, progressively adds repository rows, and applies branch/status filters", async () => {
    const user = userEvent.setup();
    const overview = deferred<Awaited<ReturnType<OverviewApi["getOverview"]>>>();
    const alpha = deferred<Awaited<ReturnType<OverviewApi["getRepositorySnapshot"]>>>();
    const beta = deferred<Awaited<ReturnType<OverviewApi["getRepositorySnapshot"]>>>();
    const api: OverviewApi = {
      listProjects: vi.fn(async () => ({ projects: [project] })),
      listWorkspaces: vi.fn(async () => ({ workspaces: [workspace] })),
      getOverview: vi.fn(() => overview.promise),
      getRepositorySnapshot: vi.fn((request) =>
        request.repositoryId === alphaId ? alpha.promise : beta.promise
      )
    };

    render(<OverviewView api={api} onOpenRepository={vi.fn()} />);
    expect(screen.getByText("Loading overview…")).toBeInTheDocument();

    overview.resolve({
      context: { projectId },
      repositories: [
        { repository: alphaRepository, snapshot: null },
        { repository: betaRepository, snapshot: null }
      ],
      repositoryCount: 2,
      dirtyCount: 1,
      stagedCount: 1,
      unstagedCount: 0,
      untrackedCount: 0,
      conflictedCount: 0,
      branches: ["main", "release"]
    });
    expect(await screen.findByText("Loading repositories 0/2…")).toBeInTheDocument();

    alpha.resolve({
      repository: alphaRepository,
      snapshot: snapshot(alphaId, "main", false),
      changes: null,
      error: null
    });
    expect(await screen.findByText("Alpha")).toBeInTheDocument();
    expect(screen.queryByText("Beta")).not.toBeInTheDocument();

    beta.resolve({
      repository: betaRepository,
      snapshot: snapshot(betaId, "release", true),
      changes: null,
      error: null
    });
    expect(await screen.findByText("Beta")).toBeInTheDocument();

    await user.selectOptions(screen.getByLabelText("Status filter"), "dirty");
    expect(screen.queryByText("Alpha")).not.toBeInTheDocument();
    expect(screen.getByText("Beta")).toBeInTheDocument();

    await user.selectOptions(screen.getByLabelText("Branch filter"), "main");
    expect(screen.getByText("No repositories match the selected filters.")).toBeInTheDocument();
  });
});

describe("ProjectsView", () => {
  it("does nothing when native root selection is cancelled", async () => {
    const user = userEvent.setup();
    const api = {
      listProjects: vi.fn(async () => ({ projects: [project] })),
      createProject: vi.fn(async () => null),
      updateProjectScanRules: vi.fn(),
      scanProject: vi.fn()
    };

    render(<ProjectsView api={api} onOpenRepository={vi.fn()} />);
    await user.click(await screen.findByRole("button", { name: "Choose root folder" }));

    expect(api.createProject).toHaveBeenCalledWith();
    expect(api.listProjects).toHaveBeenCalledOnce();
    expect(screen.queryByText(/Project created/u)).not.toBeInTheDocument();
  });

  it("refreshes the project list after native root selection succeeds", async () => {
    const user = userEvent.setup();
    const api = {
      listProjects: vi
        .fn()
        .mockResolvedValueOnce({ projects: [project] })
        .mockResolvedValueOnce({ projects: [project, betaProject] }),
      createProject: vi.fn(async () => betaProject),
      updateProjectScanRules: vi.fn(),
      scanProject: vi.fn()
    };

    render(<ProjectsView api={api} onOpenRepository={vi.fn()} />);
    await user.click(await screen.findByRole("button", { name: "Choose root folder" }));

    expect(await screen.findByText("Project Beta created.")).toBeInTheDocument();
    expect(screen.getByText("D:/work/beta")).toBeInTheDocument();
    expect(api.listProjects).toHaveBeenCalledTimes(2);
  });

  it("shows the structured host error when native root selection fails", async () => {
    const user = userEvent.setup();
    const api = {
      listProjects: vi.fn(async () => ({ projects: [project] })),
      createProject: vi.fn(async () => {
        throw errorEnvelope("project.root-unavailable", "Choose another folder");
      }),
      updateProjectScanRules: vi.fn(),
      scanProject: vi.fn()
    };

    render(<ProjectsView api={api} onOpenRepository={vi.fn()} />);
    await user.click(await screen.findByRole("button", { name: "Choose root folder" }));

    expect(await screen.findByText("Membership could not be loaded.")).toBeInTheDocument();
    expect(api.listProjects).toHaveBeenCalledOnce();
  });

  it("keeps root selection host-controlled and persists scan rules by project ID", async () => {
    const user = userEvent.setup();
    const updated = deferred<typeof project>();
    const api: ProjectsApi = {
      listProjects: vi.fn(async () => ({ projects: [project] })),
      createProject: vi.fn(async () => null),
      updateProjectScanRules: vi.fn(() => updated.promise),
      scanProject: vi.fn()
    };

    render(<ProjectsView api={api} onOpenRepository={vi.fn()} />);
    expect(await screen.findByText("C:/work/demo")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Choose root folder" })).toBeEnabled();
    expect(screen.getByText("The host selects and validates project roots.")).toBeInTheDocument();

    await user.clear(screen.getByLabelText("Scan depth for Demo"));
    await user.type(screen.getByLabelText("Scan depth for Demo"), "5");
    await user.clear(screen.getByLabelText("Exclusions for Demo"));
    await user.type(screen.getByLabelText("Exclusions for Demo"), "node_modules\ndist");
    await user.click(screen.getByRole("button", { name: "Save scan rules for Demo" }));

    expect(api.updateProjectScanRules).toHaveBeenCalledWith({
      projectId,
      scanDepth: 5,
      excludePatterns: ["node_modules", "dist"]
    });
    expect(JSON.stringify(vi.mocked(api.updateProjectScanRules).mock.calls[0]?.[0])).not.toContain(
      "rootPath"
    );
    expect(screen.getByRole("button", { name: "Saving scan rules for Demo" })).toBeDisabled();

    updated.resolve({
      ...project,
      scanDepth: 5,
      excludePatterns: ["node_modules", "dist"]
    });
    expect(await screen.findByText("Scan rules saved for Demo.")).toBeInTheDocument();
  });

  it("keeps each project busy independently while operations overlap", async () => {
    const user = userEvent.setup();
    const saved = deferred<typeof project>();
    const scanned = deferred<Awaited<ReturnType<ProjectsApi["scanProject"]>>>();
    const api: ProjectsApi = {
      listProjects: vi.fn(async () => ({ projects: [project, betaProject] })),
      createProject: vi.fn(async () => null),
      updateProjectScanRules: vi.fn(() => saved.promise),
      scanProject: vi.fn(() => scanned.promise)
    };
    render(<ProjectsView api={api} onOpenRepository={vi.fn()} />);
    const demoCard = (await screen.findByRole("heading", { name: "Demo" })).closest("article");
    const betaCard = screen.getByRole("heading", { name: "Beta" }).closest("article");
    expect(demoCard).not.toBeNull();
    expect(betaCard).not.toBeNull();

    await user.click(within(demoCard!).getByRole("button", { name: "Save scan rules for Demo" }));
    await user.click(within(betaCard!).getByRole("button", { name: "Rescan" }));

    expect(
      within(demoCard!).getByRole("button", { name: "Saving scan rules for Demo" })
    ).toBeDisabled();
    expect(within(betaCard!).getByRole("button", { name: "Rescan" })).toBeDisabled();

    await act(async () => {
      scanned.resolve({
        projectId: betaProject.id,
        repositories: [],
        failures: [],
        total: 0,
        completed: 0,
        failed: 0,
        discoveryFailed: 0,
        progress: []
      });
      await scanned.promise;
    });
    expect(within(betaCard!).getByRole("button", { name: "Rescan" })).toBeEnabled();
    expect(
      within(demoCard!).getByRole("button", { name: "Saving scan rules for Demo" })
    ).toBeDisabled();

    await act(async () => {
      saved.resolve(project);
      await saved.promise;
    });
    expect(
      within(demoCard!).getByRole("button", { name: "Save scan rules for Demo" })
    ).toBeEnabled();
  });
});

describe("WorkspacesView", () => {
  it("keeps a newer membership load pending when an older load completes", async () => {
    const oldMembership = deferred<string[]>();
    const newMembership = deferred<string[]>();
    const firstApi: WorkspacesApi = {
      listProjects: vi.fn(async () => ({ projects: [project, betaProject] })),
      listWorkspaces: vi.fn(async () => ({ workspaces: [workspace] })),
      getWorkspaceMembership: vi.fn(() => oldMembership.promise),
      createWorkspace: vi.fn(),
      updateWorkspaceMembership: vi.fn(),
      deleteWorkspace: vi.fn()
    };
    const secondApi: WorkspacesApi = {
      ...firstApi,
      listProjects: vi.fn(async () => ({ projects: [project, betaProject] })),
      listWorkspaces: vi.fn(async () => ({ workspaces: [workspace] })),
      getWorkspaceMembership: vi.fn(() => newMembership.promise)
    };
    const { rerender } = render(<WorkspacesView api={firstApi} />);
    expect(await screen.findByText("Shared")).toBeInTheDocument();
    await vi.waitFor(() => expect(firstApi.getWorkspaceMembership).toHaveBeenCalledOnce());

    rerender(<WorkspacesView api={secondApi} />);
    await vi.waitFor(() => expect(secondApi.getWorkspaceMembership).toHaveBeenCalledOnce());
    await act(async () => {
      oldMembership.resolve([projectId]);
      await oldMembership.promise;
    });

    expect(screen.getByText("Loading membership…")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Delete" })).toBeDisabled();
    expect(
      screen.queryByRole("button", { name: /Demo (?:to|from) Shared/u })
    ).not.toBeInTheDocument();

    await act(async () => {
      newMembership.resolve([betaProject.id]);
      await newMembership.promise;
    });
    expect(screen.getByRole("button", { name: "Remove Beta from Shared" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Add Demo to Shared" })).toBeEnabled();
  });

  it("ignores an older membership update after a newer membership load", async () => {
    const user = userEvent.setup();
    const oldUpdate = deferred<string[]>();
    const firstApi: WorkspacesApi = {
      listProjects: vi.fn(async () => ({ projects: [project, betaProject] })),
      listWorkspaces: vi.fn(async () => ({ workspaces: [workspace] })),
      getWorkspaceMembership: vi.fn(async () => [projectId]),
      createWorkspace: vi.fn(),
      updateWorkspaceMembership: vi.fn(() => oldUpdate.promise),
      deleteWorkspace: vi.fn()
    };
    const secondApi: WorkspacesApi = {
      ...firstApi,
      listProjects: vi.fn(async () => ({ projects: [project, betaProject] })),
      listWorkspaces: vi.fn(async () => ({ workspaces: [workspace] })),
      getWorkspaceMembership: vi.fn(async () => [betaProject.id])
    };
    const { rerender } = render(<WorkspacesView api={firstApi} />);
    await user.click(await screen.findByRole("button", { name: "Add Beta to Shared" }));

    rerender(<WorkspacesView api={secondApi} />);
    expect(await screen.findByRole("button", { name: "Add Demo to Shared" })).toBeEnabled();
    await act(async () => {
      oldUpdate.resolve([projectId, betaProject.id]);
      await oldUpdate.promise;
    });

    expect(screen.getByRole("button", { name: "Add Demo to Shared" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Remove Beta from Shared" })).toBeEnabled();
  });

  it("ignores an older delete response after a newer membership load", async () => {
    const user = userEvent.setup();
    const oldDelete = deferred<void>();
    const firstApi: WorkspacesApi = {
      listProjects: vi.fn(async () => ({ projects: [project, betaProject] })),
      listWorkspaces: vi.fn(async () => ({ workspaces: [workspace] })),
      getWorkspaceMembership: vi.fn(async () => [projectId]),
      createWorkspace: vi.fn(),
      updateWorkspaceMembership: vi.fn(),
      deleteWorkspace: vi.fn(() => oldDelete.promise)
    };
    const secondApi: WorkspacesApi = {
      ...firstApi,
      listProjects: vi.fn(async () => ({ projects: [project, betaProject] })),
      listWorkspaces: vi.fn(async () => ({ workspaces: [workspace] })),
      getWorkspaceMembership: vi.fn(async () => [betaProject.id])
    };
    const { rerender } = render(<WorkspacesView api={firstApi} />);
    await screen.findByRole("button", { name: "Remove Demo from Shared" });
    await user.click(screen.getByRole("button", { name: "Delete" }));

    rerender(<WorkspacesView api={secondApi} />);
    expect(await screen.findByRole("button", { name: "Remove Beta from Shared" })).toBeEnabled();
    await act(async () => {
      oldDelete.resolve();
      await oldDelete.promise;
    });

    expect(screen.getByText("Shared")).toBeInTheDocument();
    expect(screen.queryByText("No workspaces are available.")).not.toBeInTheDocument();
  });

  it("clears workspace operation state when deletion succeeds", async () => {
    const user = userEvent.setup();
    const api: WorkspacesApi = {
      listProjects: vi.fn(async () => ({ projects: [project] })),
      listWorkspaces: vi.fn(async () => ({ workspaces: [workspace] })),
      getWorkspaceMembership: vi.fn(async () => {
        throw errorEnvelope("workspace.membership-unavailable", "Try again");
      }),
      createWorkspace: vi.fn(async ({ name }) => ({ ...workspace, name })),
      updateWorkspaceMembership: vi.fn(),
      deleteWorkspace: vi.fn(async () => undefined)
    };
    render(<WorkspacesView api={api} />);
    expect(await screen.findByText("Membership could not be loaded.")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Delete" }));
    expect(await screen.findByText("No workspaces are available.")).toBeInTheDocument();
    await user.type(screen.getByLabelText("Workspace name"), "Recreated");
    await user.click(screen.getByRole("button", { name: "Create workspace" }));

    expect(await screen.findByText("Recreated")).toBeInTheDocument();
    expect(screen.queryByText("Membership could not be loaded.")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Add Demo to Recreated" })).toBeEnabled();
  });

  it("keeps retry recovery disabled while its workspace operation is pending", async () => {
    const user = userEvent.setup();
    const retry = deferred<string[]>();
    const api: WorkspacesApi = {
      listProjects: vi.fn(async () => ({ projects: [project] })),
      listWorkspaces: vi.fn(async () => ({ workspaces: [workspace] })),
      getWorkspaceMembership: vi
        .fn()
        .mockRejectedValueOnce(errorEnvelope("workspace.membership-unavailable", "Try again"))
        .mockImplementationOnce(() => retry.promise),
      createWorkspace: vi.fn(),
      updateWorkspaceMembership: vi.fn(),
      deleteWorkspace: vi.fn()
    };
    render(<WorkspacesView api={api} />);
    const recovery = await screen.findByRole("button", { name: "Try again" });
    await user.click(recovery);

    expect(screen.getByRole("button", { name: "Try again" })).toBeDisabled();
    await user.dblClick(screen.getByRole("button", { name: "Try again" }));
    expect(api.getWorkspaceMembership).toHaveBeenCalledTimes(2);

    await act(async () => {
      retry.resolve([projectId]);
      await retry.promise;
    });
    expect(await screen.findByRole("button", { name: "Remove Demo from Shared" })).toBeEnabled();
  });

  it("shows unsupported workspace recovery actions as host-guided and disabled", async () => {
    const api: WorkspacesApi = {
      listProjects: vi.fn(async () => ({ projects: [project] })),
      listWorkspaces: vi.fn(async () => ({ workspaces: [workspace] })),
      getWorkspaceMembership: vi.fn(async () => {
        throw {
          ...errorEnvelope("workspace.settings-required", "Open settings"),
          recoveryActions: [
            { id: "settings", label: "Open settings", kind: "openSettings" as const }
          ]
        };
      }),
      createWorkspace: vi.fn(),
      updateWorkspaceMembership: vi.fn(),
      deleteWorkspace: vi.fn()
    };
    render(<WorkspacesView api={api} />);

    const action = await screen.findByRole("button", { name: "Open settings" });
    expect(action).toBeDisabled();
    expect(action).toHaveAttribute("title", "Complete this action in the Git-Ramus host");
    expect(api.getWorkspaceMembership).toHaveBeenCalledOnce();
  });

  it("loads membership and reflects add/remove only after path-free host updates succeed", async () => {
    const user = userEvent.setup();
    const add = deferred<string[]>();
    const remove = deferred<string[]>();
    const api: WorkspacesApi = {
      listProjects: vi.fn(async () => ({ projects: [project, betaProject] })),
      listWorkspaces: vi.fn(async () => ({ workspaces: [workspace] })),
      getWorkspaceMembership: vi.fn(async () => [projectId]),
      createWorkspace: vi.fn(),
      updateWorkspaceMembership: vi
        .fn()
        .mockImplementationOnce(() => add.promise)
        .mockImplementationOnce(() => remove.promise),
      deleteWorkspace: vi.fn()
    };

    render(<WorkspacesView api={api} />);
    expect(await screen.findByRole("button", { name: "Remove Demo from Shared" })).toBeEnabled();
    const addButton = screen.getByRole("button", { name: "Add Beta to Shared" });
    await user.click(addButton);
    expect(api.updateWorkspaceMembership).toHaveBeenCalledWith({
      workspaceId,
      projectIds: [projectId, betaProject.id]
    });
    expect(JSON.stringify(vi.mocked(api.updateWorkspaceMembership).mock.calls[0]?.[0])).not.toMatch(
      /rootPath|C:\/|D:\//u
    );
    expect(screen.getByRole("button", { name: "Add Beta to Shared" })).toBeDisabled();

    add.resolve([projectId, betaProject.id]);
    expect(await screen.findByRole("button", { name: "Remove Beta from Shared" })).toBeEnabled();

    await user.click(screen.getByRole("button", { name: "Remove Demo from Shared" }));
    expect(api.updateWorkspaceMembership).toHaveBeenLastCalledWith({
      workspaceId,
      projectIds: [betaProject.id]
    });
    expect(screen.getByRole("button", { name: "Remove Demo from Shared" })).toBeDisabled();
    remove.resolve([betaProject.id]);
    expect(await screen.findByRole("button", { name: "Add Demo to Shared" })).toBeEnabled();
  });

  it("keeps failed membership loads recoverable without inventing local state", async () => {
    const user = userEvent.setup();
    const api: WorkspacesApi = {
      listProjects: vi.fn(async () => ({ projects: [project] })),
      listWorkspaces: vi.fn(async () => ({ workspaces: [workspace] })),
      getWorkspaceMembership: vi
        .fn()
        .mockRejectedValueOnce(errorEnvelope("workspace.membership-unavailable", "Try again"))
        .mockResolvedValueOnce([projectId]),
      createWorkspace: vi.fn(),
      updateWorkspaceMembership: vi.fn(),
      deleteWorkspace: vi.fn()
    };

    render(<WorkspacesView api={api} />);
    expect(await screen.findByText("Membership could not be loaded.")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Demo (?:to|from) Shared/u })
    ).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Try again" }));
    expect(await screen.findByRole("button", { name: "Remove Demo from Shared" })).toBeEnabled();
    expect(api.getWorkspaceMembership).toHaveBeenCalledTimes(2);
  });
});

describe("Git Client routes", () => {
  it("renders the view selected by the host route", async () => {
    const api = routeApi();
    const { rerender } = render(<App api={api} route="/projects" />);
    expect(await screen.findByRole("heading", { name: "Projects" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Overview" })).not.toBeInTheDocument();

    rerender(<App api={api} route="/workspaces" />);
    expect(await screen.findByRole("heading", { name: "Workspaces" })).toBeInTheDocument();
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

function errorEnvelope(code: string, recoveryLabel: string) {
  return {
    code,
    category: "retryable" as const,
    message: "Membership could not be loaded.",
    operationId: null,
    pluginId: "git-ramus.git-client",
    resourceId: workspaceId,
    failedStep: "workspaces.getMembership",
    retryable: true,
    retryAfterMs: null,
    recoveryActions: [{ id: "retry", label: recoveryLabel, kind: "retry" as const }],
    details: null
  };
}

function routeApi(): GitClientApi {
  return {
    listProjects: vi.fn(async () => ({ projects: [] })),
    createProject: vi.fn(async () => null),
    updateProjectScanRules: vi.fn(),
    scanProject: vi.fn(),
    listWorkspaces: vi.fn(async () => ({ workspaces: [] })),
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
