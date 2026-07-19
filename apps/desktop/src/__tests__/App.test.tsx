import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { App } from "../App";
import type { HostApi } from "../lib/hostApi";

const hostApi: HostApi = {
  getAppInfo: async () => ({ name: "Git-Ramus", version: "0.1.0" }),
  listPlugins: async () => [],
  listJobs: async () => [],
  authorizePluginCall: async () => ({ allowed: true }),
  startEchoJob: async () => ({
    id: "a032bc9c-8759-45ac-856f-b76f9addb9d1",
    kind: "system.echo",
    title: "Echo hello",
    status: "queued",
    progress: 0,
    cancelRequested: false,
    createdAt: "2026-07-17T00:00:00Z",
    updatedAt: "2026-07-17T00:00:00Z",
    error: null
  }),
  cancelJob: async () => undefined,
  listProjects: async () => ({ projects: [] }),
  updateProjectScanRules: async () => Promise.reject(new Error("not used")),
  scanProject: async () => Promise.reject(new Error("not used")),
  listWorkspaces: async () => ({ workspaces: [] }),
  createWorkspace: async () => Promise.reject(new Error("not used")),
  getWorkspaceMembership: async () => [],
  updateWorkspaceMembership: async () => [],
  deleteWorkspace: async () => undefined,
  getOverview: async () => Promise.reject(new Error("not used")),
  getRepositorySnapshot: async () => Promise.reject(new Error("not used")),
  getRepositoryChanges: async () => Promise.reject(new Error("not used")),
  getRepositoryDiff: async () => Promise.reject(new Error("not used")),
  getRepositoryTrustStatus: async () => Promise.reject(new Error("not used")),
  stageRepository: async () => Promise.reject(new Error("not used")),
  unstageRepository: async () => Promise.reject(new Error("not used")),
  commitRepository: async () => Promise.reject(new Error("not used")),
  trustRepository: async () => Promise.reject(new Error("not used")),
  listIdentities: async () => ({ identities: [], globalIdentityProfileId: null }),
  createIdentity: async () => Promise.reject(new Error("not used")),
  updateIdentity: async () => Promise.reject(new Error("not used")),
  deleteIdentity: async () => undefined,
  setGlobalIdentity: async () => Promise.reject(new Error("not used")),
  bindRepositoryIdentity: async () => Promise.reject(new Error("not used")),
  unbindRepositoryIdentity: async () => undefined,
  getEffectiveRepositoryIdentity: async () => Promise.reject(new Error("not used"))
};

afterEach(cleanup);

describe("App", () => {
  it("renders the trusted shell and host version", async () => {
    render(<App hostApi={hostApi} />);
    expect(screen.getByRole("heading", { name: "Git-Ramus" })).toBeInTheDocument();
    expect(await screen.findByText("Host 0.1.0")).toBeInTheDocument();
    expect(screen.getByRole("navigation", { name: "Primary" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Tasks" })).toBeInTheDocument();
  });

  it("does not render hardcoded no-op navigation entries", async () => {
    render(<App hostApi={hostApi} />);
    expect(await screen.findByText("Host 0.1.0")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Overview" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Projects" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Workspaces" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Plugins" })).not.toBeInTheDocument();
  });
});
