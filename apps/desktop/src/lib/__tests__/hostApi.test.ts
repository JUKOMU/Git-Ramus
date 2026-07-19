import { beforeEach, describe, expect, it, vi } from "vitest";
import { tauriHostApi } from "../hostApi";

const { invoke } = vi.hoisted(() => ({ invoke: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const projectId = "87a31769-8aaa-47ca-bef3-47e66f0c62fc";
const workspaceId = "e3d622f1-f1f7-4f7e-8f18-3db8a1e6ffbe";
const repositoryId = "a032bc9c-8759-45ac-856f-b76f9addb9d1";
const profileId = "d23957ac-5c0f-4857-9124-7f1599a41f33";

const project = {
  id: projectId,
  rootPath: "C:/demo",
  name: "Demo",
  scanDepth: 3,
  excludePatterns: [],
  createdAt: "2026-07-17T00:00:00Z",
  updatedAt: "2026-07-17T00:00:00Z"
};
const workspace = {
  id: workspaceId,
  name: "Workspace",
  createdAt: "2026-07-17T00:00:00Z",
  updatedAt: "2026-07-17T00:00:00Z"
};
const repository = {
  id: repositoryId,
  canonicalPath: "C:/demo/repository",
  displayName: "Repository",
  kind: "normal",
  createdAt: "2026-07-17T00:00:00Z",
  updatedAt: "2026-07-17T00:00:00Z"
};
const snapshot = {
  id: "c8f98df3-e949-48e0-a9ad-407fe371a94a",
  repositoryId,
  capturedAt: "2026-07-17T00:00:00Z",
  headOid: "abc123",
  branch: "main",
  upstream: "origin/main",
  ahead: 0,
  behind: 0,
  dirty: false,
  stagedCount: 0,
  unstagedCount: 0,
  untrackedCount: 0,
  conflictedCount: 0,
  refreshErrorSummary: null
};
const identity = {
  id: profileId,
  displayName: "Profile",
  userName: "User",
  userEmail: "user@example.com",
  gpgFormat: null,
  signingKey: null,
  signCommits: false,
  signTags: false,
  createdAt: "2026-07-17T00:00:00Z",
  updatedAt: "2026-07-17T00:00:00Z"
};

const context = { projectId };
const repositoryRequest = { ...context, repositoryId };

describe("tauriHostApi Git client commands", () => {
  beforeEach(() => {
    invoke.mockReset();
    invoke.mockImplementation(async (command: string) => responses[command]);
  });

  it("uses exact Rust command names and wraps every typed command payload in request", async () => {
    const updateScanRules = { projectId, scanDepth: 4, excludePatterns: ["target"] };
    const scan = { projectId };
    const createWorkspace = { name: "Workspace" };
    const updateMembership = { workspaceId, projectIds: [projectId] };
    const deleteWorkspace = { workspaceId };
    const diff = { ...repositoryRequest, paths: ["src/main.ts"], staged: false };
    const stage = { ...repositoryRequest, paths: ["src/main.ts"], all: false };
    const unstage = { ...repositoryRequest, paths: ["src/main.ts"] };
    const commit = {
      ...repositoryRequest,
      message: "Commit",
      identityProfileId: profileId
    };
    const createIdentity = {
      displayName: "Profile",
      userName: "User",
      userEmail: "user@example.com",
      gpgFormat: null,
      signingKey: null,
      signCommits: false,
      signTags: false
    };
    const updateIdentity = { profileId, ...createIdentity };
    const profile = { profileId };
    const bind = { ...repositoryRequest, identityProfileId: profileId };

    await tauriHostApi.listProjects();
    await tauriHostApi.listThemes();
    await tauriHostApi.currentTheme();
    await tauriHostApi.activateTheme({ themeId: "git-ramus.theme.compact" });
    await tauriHostApi.updateProjectScanRules(updateScanRules);
    await tauriHostApi.scanProject(scan);
    await tauriHostApi.listWorkspaces();
    await tauriHostApi.createWorkspace(createWorkspace);
    await tauriHostApi.getWorkspaceMembership({ workspaceId });
    await tauriHostApi.updateWorkspaceMembership(updateMembership);
    await tauriHostApi.deleteWorkspace(deleteWorkspace);
    await tauriHostApi.getOverview(context);
    await tauriHostApi.getRepositorySnapshot(repositoryRequest);
    await tauriHostApi.getRepositoryChanges(repositoryRequest);
    await tauriHostApi.getRepositoryDiff(diff);
    await tauriHostApi.getRepositoryTrustStatus(repositoryRequest);
    await tauriHostApi.stageRepository(stage);
    await tauriHostApi.unstageRepository(unstage);
    await tauriHostApi.commitRepository(commit);
    await tauriHostApi.trustRepository(repositoryRequest);
    await tauriHostApi.listIdentities();
    await tauriHostApi.createIdentity(createIdentity);
    await tauriHostApi.updateIdentity(updateIdentity);
    await tauriHostApi.deleteIdentity(profile);
    await tauriHostApi.setGlobalIdentity(profile);
    await tauriHostApi.bindRepositoryIdentity(bind);
    await tauriHostApi.unbindRepositoryIdentity(repositoryRequest);
    await tauriHostApi.getEffectiveRepositoryIdentity(repositoryRequest);

    expect(invoke.mock.calls).toEqual([
      ["git_project_list"],
      ["list_themes"],
      ["current_theme"],
      ["activate_theme", { request: { themeId: "git-ramus.theme.compact" } }],
      ["git_project_update_scan_rules", { request: updateScanRules }],
      ["git_project_scan", { request: scan }],
      ["git_workspace_list"],
      ["git_workspace_create", { request: createWorkspace }],
      ["git_workspace_get_membership", { request: { workspaceId } }],
      ["git_workspace_update_membership", { request: updateMembership }],
      ["git_workspace_delete", { request: deleteWorkspace }],
      ["git_overview_get", { request: context }],
      ["git_repository_snapshot", { request: repositoryRequest }],
      ["git_repository_changes", { request: repositoryRequest }],
      ["git_repository_diff", { request: diff }],
      ["git_repository_trust_status", { request: repositoryRequest }],
      ["git_repository_stage", { request: stage }],
      ["git_repository_unstage", { request: unstage }],
      ["git_repository_commit", { request: commit }],
      ["git_repository_trust", { request: repositoryRequest }],
      ["git_identity_list"],
      ["git_identity_create", { request: createIdentity }],
      ["git_identity_update", { request: updateIdentity }],
      ["git_identity_delete", { request: profile }],
      ["git_identity_set_global", { request: profile }],
      ["git_repository_bind_identity", { request: bind }],
      ["git_repository_unbind_identity", { request: repositoryRequest }],
      ["git_repository_effective_identity", { request: repositoryRequest }]
    ]);
  });

  it("does not expose path-bearing project creation or root update methods", () => {
    expect(tauriHostApi).not.toHaveProperty("createProject");
    expect(tauriHostApi).not.toHaveProperty("updateProject");
  });

  it("strictly validates Rust command responses", async () => {
    invoke.mockResolvedValueOnce({ projects: [], rootPath: "C:/must-not-cross-boundary" });
    await expect(tauriHostApi.listProjects()).rejects.toThrow();
  });
});

const responses: Record<string, unknown> = {
  list_themes: {
    themes: [
      {
        themeId: "git-ramus.theme.default",
        name: "Git-Ramus Default",
        pluginId: "git-ramus.host",
        version: "0.1.0",
        density: "comfortable"
      }
    ]
  },
  current_theme: {
    activeThemeId: "git-ramus.theme.default",
    theme: { themeId: "git-ramus.theme.default", density: "comfortable" }
  },
  activate_theme: {
    activeThemeId: "git-ramus.theme.compact",
    theme: { themeId: "git-ramus.theme.compact", density: "compact" }
  },
  git_project_list: { projects: [project] },
  git_project_update_scan_rules: project,
  git_project_scan: {
    projectId,
    repositories: [],
    failures: [],
    total: 0,
    completed: 0,
    failed: 0,
    discoveryFailed: 0,
    progress: []
  },
  git_workspace_list: { workspaces: [workspace] },
  git_workspace_create: workspace,
  git_workspace_get_membership: [projectId],
  git_workspace_update_membership: [projectId],
  git_workspace_delete: null,
  git_overview_get: {
    context,
    repositories: [{ repository, snapshot }],
    repositoryCount: 1,
    dirtyCount: 0,
    stagedCount: 0,
    unstagedCount: 0,
    untrackedCount: 0,
    conflictedCount: 0,
    branches: ["main"]
  },
  git_repository_snapshot: { repository, snapshot, changes: null, error: null },
  git_repository_changes: { repositoryId, snapshot, changes: [] },
  git_repository_diff: {
    repositoryId,
    staged: false,
    summary: {
      files: [],
      changes: [],
      entries: [],
      binary: false,
      additions: 0,
      deletions: 0
    }
  },
  git_repository_trust_status: { trusted: true },
  git_repository_stage: { repositoryId, snapshot, output: null },
  git_repository_unstage: { repositoryId, snapshot, output: null },
  git_repository_commit: { repositoryId, snapshot, output: "abc123" },
  git_repository_trust: {
    trust: {
      repositoryId,
      trustedAt: "2026-07-17T00:00:00Z",
      trustVersion: 1
    }
  },
  git_identity_list: { identities: [identity], globalIdentityProfileId: profileId },
  git_identity_create: identity,
  git_identity_update: identity,
  git_identity_delete: null,
  git_identity_set_global: identity,
  git_repository_bind_identity: {
    repositoryId,
    identityProfileId: profileId,
    managed: true,
    boundAt: "2026-07-17T00:00:00Z"
  },
  git_repository_unbind_identity: null,
  git_repository_effective_identity: {
    repositoryId,
    profileId,
    profile: identity,
    source: "repositoryProfile",
    displayName: identity.displayName,
    userName: identity.userName,
    userEmail: identity.userEmail,
    gpgFormat: null,
    signingKey: null,
    signCommits: false,
    signTags: false,
    drift: null
  }
};
