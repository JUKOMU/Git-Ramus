import { beforeEach, describe, expect, it, vi } from "vitest";
import providerContracts from "../../../../../packages/contracts/src/__fixtures__/provider-contracts.json";
import type { HostFileSelectionPort, ProviderPromptPort } from "../../providers/promptPorts";
import {
  nativeCertificateFileSelectionPort,
  unavailableProviderPromptPort
} from "../../providers/promptPorts";
import { createTauriHostApi, tauriHostApi } from "../hostApi";

const { invoke, open } = vi.hoisted(() => ({ invoke: vi.fn(), open: vi.fn() }));

vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open }));

const projectId = "87a31769-8aaa-47ca-bef3-47e66f0c62fc";
const workspaceId = "e3d622f1-f1f7-4f7e-8f18-3db8a1e6ffbe";
const repositoryId = "a032bc9c-8759-45ac-856f-b76f9addb9d1";
const profileId = "d23957ac-5c0f-4857-9124-7f1599a41f33";
const providerInstanceId = "6da75ccf-f7df-4bf2-92b7-2c158765726f";
const providerAccountId = "7f3c0214-373c-4d43-b0c7-cdaed1cbcc50";
const providerOperationId = "f84223af-c753-4209-be36-12d381375fcb";

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
    open.mockReset();
    invoke.mockImplementation(async (command: string) => responses[command]);
  });

  it("creates a project only from a directory selected by the native host dialog", async () => {
    open.mockResolvedValue("C:\\work\\Demo\\");
    await expect(tauriHostApi.createProject()).resolves.toEqual(project);

    expect(open).toHaveBeenCalledWith({
      directory: true,
      multiple: false,
      title: "Choose project root"
    });
    expect(invoke).toHaveBeenCalledWith("git_project_create", {
      request: {
        rootPath: "C:\\work\\Demo\\",
        name: "Demo",
        scanDepth: null,
        excludePatterns: []
      }
    });
  });

  it("returns null without invoking Rust when native project selection is cancelled", async () => {
    open.mockResolvedValue(null);
    await expect(tauriHostApi.createProject()).resolves.toBeNull();
    expect(invoke).not.toHaveBeenCalled();
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

  it("does not expose path-bearing project creation arguments or root update methods", () => {
    expect(tauriHostApi.createProject).toHaveLength(0);
    expect(tauriHostApi).not.toHaveProperty("updateProject");
  });

  it("strictly validates Rust command responses", async () => {
    invoke.mockResolvedValueOnce({ projects: [], rootPath: "C:/must-not-cross-boundary" });
    await expect(tauriHostApi.listProjects()).rejects.toThrow();
  });
});

describe("trusted Provider Host API", () => {
  let prompts: ProviderPromptPort;
  let files: HostFileSelectionPort;

  beforeEach(() => {
    invoke.mockReset();
    open.mockReset();
    invoke.mockImplementation(async (command: string) => providerResponses[command]);
    prompts = {
      requestCredential: vi.fn(async () => "glpat-host-only"),
      requestAccountAccess: vi.fn(async ({ accounts }) => [accounts[0]!.account.id])
    };
    files = { selectCertificate: vi.fn(async () => "C:/ca/root.pem") };
  });

  it("adds a PAT only after the plugin request crosses into the trusted host", async () => {
    const api = createTauriHostApi({ prompts, files });
    const request = { instanceId: providerInstanceId };

    await expect(api.connectProviderAccount("git-ramus.provider-center", request)).resolves.toEqual(
      providerContracts.authorizedAccount.account
    );

    expect(prompts.requestCredential).toHaveBeenCalledWith({
      providerLabel: "Provider",
      accountLabel: null,
      purpose: "connect"
    });
    expect(invoke).toHaveBeenCalledWith("provider_account_connect", {
      request: { instanceId: providerInstanceId, pat: "glpat-host-only" }
    });
    expect(JSON.stringify(request)).not.toContain("glpat-host-only");
  });

  it("does not invoke Rust when a credential prompt is cancelled", async () => {
    vi.mocked(prompts.requestCredential).mockResolvedValue(null);
    const api = createTauriHostApi({ prompts, files });

    await expect(
      api.connectProviderAccount("git-ramus.provider-center", {
        instanceId: providerInstanceId
      })
    ).resolves.toBeNull();
    expect(invoke).not.toHaveBeenCalled();
  });

  it("keeps certificate paths in the trusted host-only command payload", async () => {
    const api = createTauriHostApi({ prompts, files });
    const request = {
      providerKind: "gitlab" as const,
      displayName: "GitLab Example",
      baseUrl: "https://gitlab.example",
      customCaAction: "selectFile" as const
    };

    await expect(api.createProviderInstance(request)).resolves.toEqual(providerContracts.instance);
    expect(files.selectCertificate).toHaveBeenCalledOnce();
    expect(invoke).toHaveBeenCalledWith("provider_instance_create", {
      request: {
        providerKind: "gitlab",
        displayName: "GitLab Example",
        baseUrl: "https://gitlab.example",
        customCaPath: "C:/ca/root.pem"
      }
    });
    expect(JSON.stringify(request)).not.toContain("C:/ca/root.pem");
    expect(JSON.stringify(providerContracts.instance)).not.toContain("customCaPath");
  });

  it("uses exact scoped Provider commands and injects plugin identity in trusted code", async () => {
    const api = createTauriHostApi({ prompts, files });
    const query = {
      search: "skill",
      visibility: null,
      namespace: null,
      archived: "all" as const,
      sort: "name" as const,
      direction: "asc" as const,
      pageSize: 30
    };

    await api.listProviderRepositories("example.reader", {
      accountId: providerAccountId,
      query,
      cursor: null,
      operationId: providerOperationId
    });
    await api.cancelProviderOperation("example.reader", {
      accountId: providerAccountId,
      operationId: providerOperationId
    });
    await api.matchLocalProviderRemotes("example.reader", {
      instanceId: providerInstanceId,
      accountId: providerAccountId,
      operationId: providerOperationId
    });

    expect(invoke.mock.calls).toEqual([
      [
        "provider_repository_list",
        {
          request: {
            pluginId: "example.reader",
            accountId: providerAccountId,
            query,
            cursor: null,
            operationId: providerOperationId
          }
        }
      ],
      [
        "provider_operation_cancel",
        {
          request: {
            pluginId: "example.reader",
            accountId: providerAccountId,
            operationId: providerOperationId
          }
        }
      ],
      [
        "provider_local_remote_match",
        {
          request: {
            pluginId: "example.reader",
            instanceId: providerInstanceId,
            accountId: providerAccountId,
            operationId: providerOperationId
          }
        }
      ]
    ]);
  });

  it("prompts with safe account summaries and grants only selected UUIDs", async () => {
    const api = createTauriHostApi({ prompts, files });

    await expect(api.requestProviderReadAccess("example.reader")).resolves.toEqual({
      items: [providerContracts.authorizedAccount]
    });

    expect(prompts.requestAccountAccess).toHaveBeenCalledWith({
      pluginId: "example.reader",
      accounts: [providerContracts.authorizedAccount]
    });
    expect(invoke).toHaveBeenLastCalledWith("provider_permission_grant_accounts", {
      request: { pluginId: "example.reader", accountIds: [providerAccountId] }
    });
    expect(JSON.stringify(vi.mocked(prompts.requestAccountAccess).mock.calls)).not.toContain(
      "secretRef"
    );
  });

  it("never grants when account access is cancelled or returns an unknown account", async () => {
    const api = createTauriHostApi({ prompts, files });
    vi.mocked(prompts.requestAccountAccess).mockResolvedValueOnce(null);

    await expect(api.requestProviderReadAccess("example.reader")).resolves.toBeNull();
    expect(invoke.mock.calls.map(([command]) => command)).not.toContain(
      "provider_permission_grant_accounts"
    );

    invoke.mockClear();
    vi.mocked(prompts.requestAccountAccess).mockResolvedValueOnce([
      "fd52be07-485e-44ae-b57d-0fa69d83772f"
    ]);
    await expect(api.requestProviderReadAccess("example.reader")).rejects.toThrow(
      "invalid account selection"
    );
    expect(invoke.mock.calls.map(([command]) => command)).not.toContain(
      "provider_permission_grant_accounts"
    );
  });

  it("maps every remaining Provider operation to its explicit typed Rust command", async () => {
    const api = createTauriHostApi({ prompts, files });
    const instanceRequest = { instanceId: providerInstanceId };
    const accountRequest = { accountId: providerAccountId };
    const bindingRequest = { accountId: providerAccountId };
    const bindingMutation = {
      repositoryId,
      remoteName: "origin",
      instanceId: providerInstanceId,
      accountId: null,
      providerRepositoryId: "4242"
    };

    await api.authorizePluginPermissionRequest({
      pluginId: "example.reader",
      capability: "providers:read",
      resource: "providers"
    });
    await api.listProviderInstances();
    await api.updateProviderInstance({
      ...instanceRequest,
      displayName: "GitLab Example",
      baseUrl: "https://gitlab.example",
      customCaAction: "keep"
    });
    await api.validateProviderInstance(instanceRequest);
    await api.deleteProviderInstance(instanceRequest);
    await api.listProviderAccounts(instanceRequest);
    await api.rotateProviderAccount("git-ramus.provider-center", accountRequest);
    await api.validateProviderAccount(accountRequest);
    await api.setDefaultProviderAccount({ ...instanceRequest, ...accountRequest });
    await api.getProviderAccountDeletionImpact(accountRequest);
    await api.deleteProviderAccount({
      ...accountRequest,
      resolution: { kind: "unbind" },
      newDefaultAccountId: null
    });
    await api.listAuthorizedProviderAccounts("example.reader");
    await api.revokeProviderReadAccess("example.reader", accountRequest);
    await api.listProviderBindings(bindingRequest);
    await api.bindProviderRemote(bindingMutation);
    await api.unbindProviderRemote({ repositoryId, remoteName: "origin" });

    expect(invoke.mock.calls.map(([command]) => command)).toEqual([
      "provider_permission_is_declared",
      "provider_instance_list",
      "provider_instance_update",
      "provider_instance_validate",
      "provider_instance_delete",
      "provider_account_list",
      "provider_account_rotate",
      "provider_account_validate",
      "provider_account_set_default",
      "provider_account_deletion_impact",
      "provider_account_delete",
      "provider_permission_list_authorized_accounts",
      "provider_permission_revoke_account",
      "provider_binding_list",
      "provider_binding_set",
      "provider_binding_delete"
    ]);
  });
});

describe("trusted Provider native ports", () => {
  beforeEach(() => {
    open.mockReset();
  });

  it("selects only one certificate through the native filtered dialog", async () => {
    open.mockResolvedValue("C:/ca/root.pem");

    await expect(nativeCertificateFileSelectionPort.selectCertificate()).resolves.toBe(
      "C:/ca/root.pem"
    );
    expect(open).toHaveBeenCalledWith({
      multiple: false,
      directory: false,
      title: "Choose a trusted CA certificate",
      filters: [
        {
          name: "Certificates",
          extensions: ["pem", "crt", "cer"]
        }
      ]
    });
  });

  it("rejects invalid native certificate selections and preserves cancellation", async () => {
    open.mockResolvedValueOnce(null);
    await expect(nativeCertificateFileSelectionPort.selectCertificate()).resolves.toBeNull();
    open.mockResolvedValueOnce([]);
    await expect(nativeCertificateFileSelectionPort.selectCertificate()).rejects.toThrow(
      "invalid path"
    );
    open.mockResolvedValueOnce("");
    await expect(nativeCertificateFileSelectionPort.selectCertificate()).rejects.toThrow(
      "invalid path"
    );
  });

  it("fails with a stable code until a trusted prompt broker is mounted", async () => {
    await expect(
      unavailableProviderPromptPort.requestCredential({
        providerLabel: "Provider",
        accountLabel: null,
        purpose: "connect"
      })
    ).rejects.toMatchObject({ code: "provider.prompt-unavailable" });
    await expect(
      unavailableProviderPromptPort.requestAccountAccess({
        pluginId: "example.reader",
        accounts: []
      })
    ).rejects.toMatchObject({ code: "provider.prompt-unavailable" });
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
  git_project_create: project,
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
    patch: "diff --git a/src/main.ts b/src/main.ts\n-old\n+new\n",
    truncated: false,
    contentUnavailableReason: null,
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

const providerResponses: Record<string, unknown> = {
  provider_instance_list: { items: [providerContracts.instance] },
  provider_instance_create: providerContracts.instance,
  provider_instance_update: providerContracts.instance,
  provider_instance_validate: providerContracts.instance,
  provider_instance_delete: null,
  provider_account_list: { items: [providerContracts.authorizedAccount.account] },
  provider_account_connect: providerContracts.authorizedAccount.account,
  provider_account_rotate: providerContracts.authorizedAccount.account,
  provider_account_validate: providerContracts.authorizedAccount.account,
  provider_account_set_default: providerContracts.authorizedAccount.account,
  provider_account_deletion_impact: {
    accountId: providerAccountId,
    instanceId: providerInstanceId,
    isDefault: true,
    explicitBindingCount: 0,
    inheritedBindingCount: 0,
    siblingAccountIds: [],
    requiresNewDefault: false
  },
  provider_account_delete: null,
  provider_repository_list: providerContracts.repositoryPage,
  provider_operation_cancel: null,
  provider_local_remote_match: { items: [] },
  provider_permission_is_declared: { allowed: true },
  provider_permission_list_authorized_accounts: {
    items: [providerContracts.authorizedAccount]
  },
  provider_permission_grant_accounts: { items: [providerContracts.authorizedAccount] },
  provider_permission_revoke_account: null,
  provider_binding_list: { items: [providerContracts.binding] },
  provider_binding_set: providerContracts.binding,
  provider_binding_delete: null
};
