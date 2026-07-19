import { describe, expect, it } from "vitest";
import manifest from "../../../../plugins/builtin-welcome/plugin.json";
import gitClientManifest from "../../../../plugins/git-client/plugin.json";
import compactManifest from "../../../../plugins/builtin-compact-theme/plugin.json";
import compactTheme from "../../../../plugins/builtin-compact-theme/theme.json";
import providerContracts from "../__fixtures__/provider-contracts.json";
import type { PersistedRepositorySnapshot, RepositorySnapshot } from "../index";
import {
  errorEnvelopeSchema,
  hostInitSchema,
  hostToPluginMessageSchema,
  jobSchema,
  pluginManifestSchema,
  rpcRequestSchema,
  themeDefinitionSchema,
  projectSchema,
  workspaceSchema,
  repositorySchema,
  repositorySnapshotSchema,
  persistedRepositorySnapshotSchema,
  parsedChangeEntrySchema,
  identityProfileSchema,
  effectiveIdentitySchema,
  gitContextRequestSchema,
  projectCreateRequestSchema,
  projectListResponseSchema,
  projectUpdateScanRulesRequestSchema,
  workspaceRequestSchema,
  workspaceMembershipResponseSchema,
  repositoryRequestSchema,
  repositoryStageRequestSchema,
  trustStatusResponseSchema,
  scanProjectResultSchema,
  overviewSchema,
  changesResultSchema,
  diffResultSchema,
  writeResultSchema,
  identityListResponseSchema,
  operationResponseSchema,
  repositoryOperationResponseSchema,
  themeContributionSchema,
  hostThemeChangedSchema,
  themeChangedSchema,
  themeActivateRequestSchema,
  themeCatalogSchema,
  themeStateSchema,
  providerAccountConnectRequestSchema,
  providerAccountDeleteRequestSchema,
  providerAccountSummarySchema,
  providerAuthorizedAccountSchema,
  providerBindingListRequestSchema,
  providerBindingSchema,
  providerBindingSetRequestSchema,
  providerContributionSchema,
  providerInstanceCreateRequestSchema,
  providerInstanceSchema,
  providerOperationCancelRequestSchema,
  providerRepositoryListRequestSchema,
  providerRepositoryPageSchema,
  providerRepositoryQuerySchema
} from "../index";

const validMotionDurations = ["0ms", "1ms", "1.5s", "2000ms"] as const;
const invalidMotionDurations = ["1e3ms", ".5s", "1.s", "+1ms", "-1ms", " 1ms", "1ms "] as const;
const projectId = "87a31769-8aaa-47ca-bef3-47e66f0c62fc";
const workspaceId = "e3d622f1-f1f7-4f7e-8f18-3db8a1e6ffbe";
const repositoryId = "a032bc9c-8759-45ac-856f-b76f9addb9d1";
const profileId = "d23957ac-5c0f-4857-9124-7f1599a41f33";
const instanceId = "6da75ccf-f7df-4bf2-92b7-2c158765726f";
const accountId = "7f3c0214-373c-4d43-b0c7-cdaed1cbcc50";
const providerOperationId = "f84223af-c753-4209-be36-12d381375fcb";

const backendManifest = {
  schemaVersion: 1,
  id: "git-ramus.provider.gitlab",
  name: "GitLab Provider",
  version: "0.1.0",
  publisher: "git-ramus",
  description: "GitLab API adapter.",
  kind: "builtin",
  sdkVersion: "^0.1.0",
  entrypoints: {},
  contributions: {
    navigation: [],
    providers: [
      {
        providerId: "gitlab",
        adapterId: "git-ramus.provider.gitlab",
        displayName: "GitLab",
        icon: "gitlab",
        instanceModes: ["cloud", "selfHosted"],
        capabilities: ["repositoryDiscovery", "customCa"]
      }
    ]
  },
  permissions: []
} as const;

const externalUiManifest = {
  schemaVersion: 1,
  id: "example.provider-reader",
  name: "Provider Reader",
  version: "0.1.0",
  publisher: "example",
  description: "Reads authorized Provider accounts.",
  kind: "external",
  sdkVersion: "^0.1.0",
  entrypoints: { ui: "ui.html" },
  contributions: { navigation: [] },
  permissions: [{ capability: "providers:read", resources: ["providers"] }]
} as const;

const accountSummary = {
  id: accountId,
  instanceId,
  providerUserId: "9001",
  username: "creator",
  displayName: "Skill Creator",
  avatarUrl: "https://gitlab.example/uploads/avatar.png",
  isDefault: true,
  status: "connected" as const,
  lastValidatedAt: "2026-07-19T00:00:00Z"
};

const assertNoProviderSecrets = (value: unknown): void => {
  if (Array.isArray(value)) {
    value.forEach(assertNoProviderSecrets);
    return;
  }
  if (typeof value !== "object" || value === null) return;
  for (const [key, child] of Object.entries(value)) {
    expect(key).not.toMatch(/pat|secretRef|authorization|customCaPath/iu);
    assertNoProviderSecrets(child);
  }
};

const project = {
  id: projectId,
  name: "Demo",
  rootPath: "C:/demo",
  scanDepth: 3,
  excludePatterns: ["node_modules"],
  createdAt: "2026-07-17T00:00:00Z",
  updatedAt: "2026-07-17T00:00:00Z"
};

const repository = {
  id: repositoryId,
  canonicalPath: "C:/demo/repository",
  displayName: "Repository",
  kind: "normal" as const,
  createdAt: "2026-07-17T00:00:00Z",
  updatedAt: "2026-07-17T00:00:00Z"
};

const persistedSnapshot = {
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
} satisfies PersistedRepositorySnapshot;

describe("shared contracts", () => {
  it("accepts the built-in welcome manifest", () => {
    expect(pluginManifestSchema.parse(manifest).id).toBe("git-ramus.welcome");
  });

  it.each([
    "",
    "<style>body{color:red}</style>",
    "x".repeat(65),
    "unsafe\u0000name",
    "url\u00a0(https://evil.test)"
  ])("rejects unsafe manifest name %s", (name) => {
    expect(() => pluginManifestSchema.parse({ ...manifest, name })).toThrow();
  });

  it.each([
    "",
    "<script>alert(1)</script>",
    "x".repeat(257),
    "unsafe\ndescription",
    "url\u00a0(https://evil.test)"
  ])("rejects unsafe manifest description %s", (description) => {
    expect(() => pluginManifestSchema.parse({ ...manifest, description })).toThrow();
  });

  it("accepts the built-in Git Client routes and scoped permissions", () => {
    const parsed = pluginManifestSchema.parse(gitClientManifest);
    expect(parsed.id).toBe("git-ramus.git-client");
    expect(parsed.contributions.navigation.map(({ route }) => route)).toEqual([
      "/overview",
      "/projects",
      "/workspaces",
      "/identities"
    ]);
    expect(parsed.permissions).toEqual([
      { capability: "projects:manage", resources: ["projects"] },
      { capability: "workspaces:manage", resources: ["workspaces"] },
      { capability: "repositories:read", resources: ["repositories"] },
      { capability: "repositories:write", resources: ["repositories"] },
      { capability: "identities:read", resources: ["identities"] },
      { capability: "identities:write", resources: ["identities"] }
    ]);
  });

  it("accepts a backend-only built-in Provider contribution", () => {
    const parsed = pluginManifestSchema.parse(backendManifest);
    expect(parsed.entrypoints.ui).toBeUndefined();
    expect(parsed.contributions.providers).toHaveLength(1);
  });

  it("rejects external backend adapters and navigation without a UI", () => {
    expect(() => pluginManifestSchema.parse({ ...backendManifest, kind: "external" })).toThrow();
    expect(() =>
      pluginManifestSchema.parse({
        ...backendManifest,
        contributions: {
          ...backendManifest.contributions,
          navigation: [{ id: "bad", label: "Bad", route: "/bad", icon: "x" }]
        }
      })
    ).toThrow();
    expect(() =>
      pluginManifestSchema.parse({
        ...externalUiManifest,
        permissions: [{ capability: "providers:manage", resources: ["providers"] }]
      })
    ).toThrow();
  });

  it("parses Provider pages without accepting a secret field", () => {
    const page = providerRepositoryPageSchema.parse({
      items: [
        {
          providerKind: "gitlab",
          instanceId,
          repositoryId: "42",
          namespace: "group",
          name: "skill-set",
          fullName: "group/skill-set",
          webUrl: "https://gitlab.example/group/skill-set",
          httpsUrl: "https://gitlab.example/group/skill-set.git",
          sshUrl: "git@gitlab.example:group/skill-set.git",
          defaultBranch: "main",
          visibility: "private",
          archived: false,
          fork: false,
          permission: "write",
          updatedAt: "2026-07-19T00:00:00Z"
        }
      ],
      nextCursor: null,
      hasMore: false,
      rateLimit: null
    });
    expect(page.items[0]?.fullName).toBe("group/skill-set");
    expect(() =>
      providerAccountSummarySchema.parse({ ...accountSummary, secretRef: "leak" })
    ).toThrow();
  });

  it("round-trips the canonical secret-free Provider fixtures", () => {
    expect(providerInstanceSchema.parse(providerContracts.instance)).toEqual(
      providerContracts.instance
    );
    expect(providerAuthorizedAccountSchema.parse(providerContracts.authorizedAccount)).toEqual(
      providerContracts.authorizedAccount
    );
    expect(providerRepositoryPageSchema.parse(providerContracts.repositoryPage)).toEqual(
      providerContracts.repositoryPage
    );
    expect(providerBindingSchema.parse(providerContracts.binding)).toEqual(
      providerContracts.binding
    );
    expect(errorEnvelopeSchema.parse(providerContracts.error)).toEqual(providerContracts.error);
    assertNoProviderSecrets(providerContracts);
  });

  it("enforces the Provider contribution capability matrix and unique lists", () => {
    expect(
      providerContributionSchema.parse({
        ...backendManifest.contributions.providers[0]
      }).providerId
    ).toBe("gitlab");
    expect(() =>
      providerContributionSchema.parse({
        ...backendManifest.contributions.providers[0],
        instanceModes: ["cloud", "cloud"]
      })
    ).toThrow();
    expect(() =>
      providerContributionSchema.parse({
        ...backendManifest.contributions.providers[0],
        providerId: "github",
        icon: "github",
        adapterId: "git-ramus.provider.github",
        instanceModes: ["cloud", "selfHosted"],
        capabilities: ["repositoryDiscovery", "customCa"]
      })
    ).toThrow();
  });

  it("keeps Provider plugin requests ID-only and strictly bounded", () => {
    expect(
      providerInstanceCreateRequestSchema.parse({
        providerKind: "gitlab",
        displayName: "Self managed",
        baseUrl: "https://gitlab.example/root",
        customCaAction: "selectFile"
      }).baseUrl
    ).toBe("https://gitlab.example/root");
    expect(() =>
      providerInstanceCreateRequestSchema.parse({
        providerKind: "gitlab",
        displayName: "Unsafe",
        baseUrl: "http://gitlab.example",
        customCaAction: "none"
      })
    ).toThrow();
    expect(() =>
      providerAccountConnectRequestSchema.parse({ instanceId, pat: "must-not-cross" })
    ).toThrow();

    const query = providerRepositoryQuerySchema.parse({
      search: "  skill  ",
      visibility: "private",
      namespace: "  group  ",
      archived: "active",
      sort: "updated",
      direction: "desc",
      pageSize: 30
    });
    expect(query.search).toBe("skill");
    expect(query.namespace).toBe("group");
    expect(
      providerRepositoryListRequestSchema.parse({
        accountId,
        query,
        cursor: null,
        operationId: providerOperationId
      }).operationId
    ).toBe(providerOperationId);
    expect(
      providerOperationCancelRequestSchema.parse({ accountId, operationId: providerOperationId })
        .accountId
    ).toBe(accountId);
    expect(providerBindingListRequestSchema.parse({ accountId })).toEqual({ accountId });
    expect(
      providerAccountDeleteRequestSchema.parse({
        accountId,
        resolution: { kind: "unbind" }
      }).newDefaultAccountId
    ).toBeNull();
    expect(() =>
      providerBindingSetRequestSchema.parse({
        repositoryId,
        remoteName: "origin",
        instanceId,
        accountId: null,
        providerRepositoryId: "42",
        rootPath: "C:/must-not-cross"
      })
    ).toThrow();
  });

  it.each(["../secret.html", "..\\secret.html", "C:\\secret.html", "C:secret.html"])(
    "rejects unsafe plugin entrypoint %s",
    (ui) => {
      expect(() =>
        pluginManifestSchema.parse({
          ...manifest,
          entrypoints: { ui }
        })
      ).toThrow();
    }
  );

  it.each([
    "https://evil.test/theme.json",
    "data:text/html,evil",
    "file:theme.json",
    "theme.json\u0000",
    "theme\n.json"
  ])("rejects scheme-like or control-character theme definition %s", (definition) => {
    expect(() =>
      pluginManifestSchema.parse({
        ...manifest,
        contributions: {
          ...manifest.contributions,
          theme: { themeId: "git-ramus.safe", definition }
        }
      })
    ).toThrow();
  });

  it("parses an RPC request", () => {
    const request = rpcRequestSchema.parse({
      type: "rpc:request",
      requestId: "87a31769-8aaa-47ca-bef3-47e66f0c62fc",
      sessionId: "e3d622f1-f1f7-4f7e-8f18-3db8a1e6ffbe",
      method: "app.getInfo",
      params: {}
    });
    expect(request.method).toBe("app.getInfo");
  });

  it("accepts a route-aware init and validated theme updates", () => {
    expect(
      hostInitSchema.parse({
        type: "host:init",
        sessionId: "e3d622f1-f1f7-4f7e-8f18-3db8a1e6ffbe",
        pluginId: "git-ramus.welcome",
        sdkVersion: "0.1.0",
        route: "/projects"
      }).route
    ).toBe("/projects");
    expect(
      hostToPluginMessageSchema.parse({
        type: "host:theme-changed",
        sessionId: "e3d622f1-f1f7-4f7e-8f18-3db8a1e6ffbe",
        theme: {
          themeId: "git-ramus.default",
          colors: { background: "#fff" },
          typography: { fontFamily: "system-ui" },
          spacing: { unit: 4 },
          shape: { radius: 4 },
          elevation: { level1: "0 1px 2px #0002" },
          motion: { durationFast: "120ms" },
          density: "comfortable"
        }
      }).type
    ).toBe("host:theme-changed");
  });

  it("normalizes an omitted init route and exports theme message schemas", () => {
    expect(
      hostInitSchema.parse({
        type: "host:init",
        sessionId: "e3d622f1-f1f7-4f7e-8f18-3db8a1e6ffbe",
        pluginId: "git-ramus.welcome",
        sdkVersion: "0.1.0"
      }).route
    ).toBe("/");
    expect(
      hostToPluginMessageSchema.parse({
        type: "host:theme-changed",
        sessionId: "e3d622f1-f1f7-4f7e-8f18-3db8a1e6ffbe",
        theme: { themeId: "git-ramus.default", density: "compact" }
      }).type
    ).toBe("host:theme-changed");
    expect(themeContributionSchema).toBeDefined();
    expect(hostThemeChangedSchema).toBeDefined();
    expect(themeChangedSchema).toBe(hostThemeChangedSchema);
  });

  it("accepts a theme contribution with a safe relative definition", () => {
    const parsed = pluginManifestSchema.parse({
      ...manifest,
      contributions: {
        ...manifest.contributions,
        theme: { themeId: "git-ramus.default", definition: "theme.json" }
      }
    });
    expect(parsed.contributions.theme?.themeId).toBe("git-ramus.default");
    expect(() =>
      pluginManifestSchema.parse({
        ...manifest,
        contributions: {
          ...manifest.contributions,
          theme: { themeId: "x", definition: "../theme.json" }
        }
      })
    ).toThrow();
    expect(
      pluginManifestSchema.parse({
        ...manifest,
        contributions: {
          ...manifest.contributions,
          theme: { themeId: "git-ramus.default", definitionPath: "theme.json" }
        }
      }).contributions.theme?.definitionPath
    ).toBe("theme.json");
    expect(() =>
      pluginManifestSchema.parse({
        ...manifest,
        contributions: {
          ...manifest.contributions,
          theme: {
            themeId: "git-ramus.default",
            definition: "theme.json",
            definitionPath: "other.json"
          }
        }
      })
    ).toThrow();
  });

  it("parses the shipped Compact manifest and definition", () => {
    const parsedManifest = pluginManifestSchema.parse(compactManifest);
    const parsedTheme = themeDefinitionSchema.parse(compactTheme);
    expect(parsedManifest.contributions.theme?.themeId).toBe("git-ramus.theme.compact");
    expect(parsedTheme.themeId).toBe(parsedManifest.contributions.theme?.themeId);
    expect(parsedTheme.density).toBe("compact");
  });

  it.each(validMotionDurations)("accepts canonical motion duration %s", (duration) => {
    expect(() =>
      themeDefinitionSchema.parse({
        themeId: "git-ramus.theme.duration",
        motion: { durationFast: duration }
      })
    ).not.toThrow();
  });

  it.each(invalidMotionDurations)("rejects non-canonical motion duration %s", (duration) => {
    expect(() =>
      themeDefinitionSchema.parse({
        themeId: "git-ramus.theme.duration",
        motion: { durationFast: duration }
      })
    ).toThrow();
  });

  it("rejects executable or arbitrary theme payloads", () => {
    expect(() => themeDefinitionSchema.parse({ themeId: "x", css: "body{}" })).toThrow();
    expect(() =>
      themeDefinitionSchema.parse({ themeId: "x", colors: { background: () => 1 } })
    ).toThrow();
  });

  it.each([
    ["colors", "url(javascript:alert(1))"],
    ["colors", "<style>body{color:red}</style>"],
    ["typography", "@import url(https://evil.test/x.css)"],
    ["spacing", "1rem; color:red"],
    ["shape", "4px}"],
    ["elevation", "url(javascript:alert(1))"],
    ["motion", "100ms; background:url(https://evil.test)"],
    ["density", "url(https://evil.test)"]
  ])("rejects unsafe %s token values", (group, value) => {
    expect(() =>
      themeDefinitionSchema.parse({ themeId: "git-ramus.safe", [group]: { token: value } })
    ).toThrow();
  });

  it("rejects unknown theme groups and token keys", () => {
    expect(() => themeDefinitionSchema.parse({ themeId: "git-ramus.safe", shadows: {} })).toThrow();
    expect(() =>
      themeDefinitionSchema.parse({ themeId: "git-ramus.safe", colors: { arbitrary: "#fff" } })
    ).toThrow();
  });

  it("strictly matches the Rust theme catalog, state, and activation contracts", () => {
    const theme = {
      themeId: "git-ramus.theme.compact",
      name: "Compact",
      colors: { background: "#07111f" },
      density: "compact" as const
    };
    expect(
      themeCatalogSchema.parse({
        themes: [
          {
            themeId: theme.themeId,
            name: "Compact",
            pluginId: "git-ramus.compact-theme",
            version: "0.1.0",
            density: "compact"
          }
        ]
      }).themes[0]?.themeId
    ).toBe(theme.themeId);
    expect(themeStateSchema.parse({ activeThemeId: theme.themeId, theme }).theme).toEqual(theme);
    expect(themeActivateRequestSchema.parse({ themeId: theme.themeId })).toEqual({
      themeId: theme.themeId
    });
    expect(() =>
      themeStateSchema.parse({
        activeThemeId: "git-ramus.theme.default",
        theme,
        definitionPath: "C:/must-not-cross-boundary"
      })
    ).toThrow();
    expect(() =>
      themeStateSchema.parse({ activeThemeId: "git-ramus.theme.default", theme })
    ).toThrow();
  });

  it("rejects allowed token keys whose values exceed host safety bounds", () => {
    expect(() =>
      themeDefinitionSchema.parse({
        themeId: "git-ramus.theme.unsafe",
        spacing: { unit: 10000 }
      })
    ).toThrow();
    expect(() =>
      themeDefinitionSchema.parse({
        themeId: "git-ramus.theme.unsafe",
        typography: { fontWeight: 901 }
      })
    ).toThrow();
    expect(() =>
      themeDefinitionSchema.parse({
        themeId: "git-ramus.theme.unsafe",
        colors: { background: "url(https://evil.test/a.png)" }
      })
    ).toThrow();
  });

  it("matches the camelCase Rust project, workspace, repository, and snapshot DTOs", () => {
    expect(projectSchema.parse(project).id).toBe(projectId);
    expect(
      workspaceSchema.parse({
        id: workspaceId,
        name: "Shared",
        createdAt: "2026-07-17T00:00:00Z",
        updatedAt: "2026-07-17T00:00:00Z"
      }).id
    ).toBe(workspaceId);
    expect(repositorySchema.parse(repository).id).toBe(repositoryId);
    expect(persistedRepositorySnapshotSchema.parse(persistedSnapshot).headOid).toBe("abc123");

    expect(() => workspaceSchema.parse({ ...project, projectIds: [projectId] })).toThrow();
    expect(() => repositorySchema.parse({ ...repository, workspaceIds: [workspaceId] })).toThrow();
    expect(() =>
      persistedRepositorySnapshotSchema.parse({ ...persistedSnapshot, headSha: "abc123" })
    ).toThrow();
  });

  it("accepts only a registered workspace ID when reading membership", () => {
    expect(workspaceRequestSchema.parse({ workspaceId })).toEqual({ workspaceId });
    expect(workspaceMembershipResponseSchema.parse([projectId])).toEqual([projectId]);
    expect(() =>
      workspaceRequestSchema.parse({ workspaceId, rootPath: "C:/must-not-cross-boundary" })
    ).toThrow();
  });

  it("reads repository Trust status through a strict ID-only contract", () => {
    expect(repositoryRequestSchema.parse({ projectId, repositoryId })).toEqual({
      projectId,
      repositoryId
    });
    expect(trustStatusResponseSchema.parse({ trusted: true })).toEqual({ trusted: true });
    expect(() =>
      trustStatusResponseSchema.parse({ trusted: true, trustedAt: "2026-07-17T00:00:00Z" })
    ).toThrow();
    expect(() =>
      repositoryRequestSchema.parse({ projectId, repositoryId, rootPath: "C:/secret" })
    ).toThrow();
  });

  it("matches the complete Rust change entry DTO and rejects unknown fields", () => {
    const change = {
      path: "src/main.ts",
      originalPath: null,
      kind: "modified" as const,
      staged: false,
      unstaged: true,
      conflicted: false,
      binary: false,
      old: null,
      new: null,
      oldPath: null,
      newPath: null,
      status: ".M",
      indexStatus: ".",
      worktreeStatus: "M",
      additions: 2,
      deletions: 1
    };
    expect(parsedChangeEntrySchema.parse(change).unstaged).toBe(true);
    expect(() => parsedChangeEntrySchema.parse({ ...change, absolutePath: "C:/secret" })).toThrow();
  });

  it("matches identity profile, source, and drift DTOs from the Rust host", () => {
    const identity = identityProfileSchema.parse({
      id: profileId,
      displayName: "Demo Profile",
      userName: "Demo User",
      userEmail: "demo@example.com",
      gpgFormat: "ssh",
      signingKey: "SHA256:key",
      signCommits: true,
      signTags: false,
      createdAt: "2026-07-17T00:00:00Z",
      updatedAt: "2026-07-17T00:00:00Z"
    });
    expect(identity.userEmail).toBe("demo@example.com");
    expect(identityProfileSchema.parse({ ...identity, userEmail: "a@b" }).userEmail).toBe("a@b");
    expect(
      effectiveIdentitySchema.parse({
        repositoryId,
        profileId,
        profile: identity,
        source: "repositoryProfile",
        displayName: "Demo Profile",
        userName: "Demo User",
        userEmail: "demo@example.com",
        gpgFormat: "ssh",
        signingKey: "SHA256:key",
        signCommits: true,
        signTags: false,
        drift: {
          fields: [{ key: "user.email", expected: ["demo@example.com"], actual: ["other@x.test"] }]
        }
      }).source
    ).toBe("repositoryProfile");
    expect(
      effectiveIdentitySchema.parse({
        repositoryId,
        profileId: null,
        profile: null,
        source: "externalGlobal",
        displayName: "External User",
        userName: "External User",
        userEmail: "git-config-value",
        gpgFormat: "custom",
        signingKey: null,
        signCommits: false,
        signTags: false,
        drift: null
      }).source
    ).toBe("externalGlobal");
    expect(() => identityProfileSchema.parse({ ...identity, email: identity.userEmail })).toThrow();
  });

  it("uses strict UUID request schemas and rejects arbitrary filesystem path keys", () => {
    expect(projectCreateRequestSchema.parse({})).toEqual({});
    expect(() => projectCreateRequestSchema.parse({ rootPath: "C:/secret" })).toThrow();
    expect(gitContextRequestSchema.parse({ projectId })).toEqual({ projectId });
    expect(() => gitContextRequestSchema.parse({ projectId, workspaceId })).toThrow();
    expect(() => gitContextRequestSchema.parse({ projectId: "project" })).toThrow();
    expect(() =>
      projectUpdateScanRulesRequestSchema.parse({ projectId, rootPath: "C:/secret" })
    ).toThrow();
    expect(() =>
      repositoryStageRequestSchema.parse({
        projectId,
        repositoryId,
        paths: [],
        all: true,
        path: "C:/secret"
      })
    ).toThrow();
    expect(projectListResponseSchema.parse({ projects: [project] }).projects).toHaveLength(1);
    expect(() =>
      projectListResponseSchema.parse({ projects: [project], rootPath: "C:/secret" })
    ).toThrow();
  });

  it("parses the Rust overview, changes, diff, write, scan, and identity-list responses", () => {
    expect(
      overviewSchema.parse({
        context: { projectId },
        repositories: [{ repository, snapshot: null }],
        repositoryCount: 1,
        dirtyCount: 0,
        stagedCount: 0,
        unstagedCount: 0,
        untrackedCount: 0,
        conflictedCount: 0,
        branches: ["main"]
      }).repositoryCount
    ).toBe(1);
    expect(
      changesResultSchema.parse({ repositoryId, snapshot: persistedSnapshot, changes: [] })
        .repositoryId
    ).toBe(repositoryId);
    expect(
      diffResultSchema.parse({
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
      }).summary.files
    ).toEqual([]);
    expect(() =>
      diffResultSchema.parse({
        repositoryId,
        staged: false,
        patch: null,
        truncated: false,
        contentUnavailableReason: "repositoryNotTrusted",
        summary: {
          files: [],
          changes: [],
          entries: [],
          binary: false,
          additions: 0,
          deletions: 0
        }
      })
    ).toThrow();
    for (const contentUnavailableReason of ["nonUtf8Content", "outputLimit"] as const) {
      expect(
        diffResultSchema.parse({
          repositoryId,
          staged: false,
          patch: null,
          truncated: false,
          contentUnavailableReason,
          summary: {
            files: [],
            changes: [],
            entries: [],
            binary: false,
            additions: 0,
            deletions: 0
          }
        }).contentUnavailableReason
      ).toBe(contentUnavailableReason);
    }
    expect(
      writeResultSchema.parse({ repositoryId, snapshot: null, output: null }).output
    ).toBeNull();
    expect(
      scanProjectResultSchema.parse({
        projectId,
        repositories: [],
        failures: [],
        total: 0,
        completed: 0,
        failed: 0,
        discoveryFailed: 0,
        progress: []
      }).failed
    ).toBe(0);
    expect(
      identityListResponseSchema.parse({ identities: [], globalIdentityProfileId: null })
        .globalIdentityProfileId
    ).toBeNull();
  });

  it("retains the asynchronous operation response compatibility contract", () => {
    expect(
      operationResponseSchema.parse({
        operationId: projectId,
        status: "accepted",
        result: { queued: true }
      }).status
    ).toBe("accepted");
    expect(
      repositoryOperationResponseSchema.parse({
        operationId: projectId,
        repositoryId,
        status: "accepted"
      }).repositoryId
    ).toBe(repositoryId);
    expect(() =>
      operationResponseSchema.parse({ repositoryId, snapshot: null, output: null })
    ).toThrow();
  });

  it("retains the Task 1 repository snapshot payload contract", () => {
    const task1Snapshot = {
      id: "c8f98df3-e949-48e0-a9ad-407fe371a94a",
      repositoryId,
      branch: "main",
      headSha: "abc123",
      isDirty: true,
      ahead: 1,
      behind: 2,
      changes: [
        {
          path: "README.md",
          status: "modified",
          oldPath: null,
          staged: true,
          additions: 3,
          deletions: 1
        }
      ],
      upstream: {
        remote: "origin",
        branch: "origin/main",
        ahead: 1,
        behind: 2
      },
      summary: {
        total: 1,
        added: 0,
        modified: 1,
        deleted: 0,
        untracked: 0,
        staged: 1,
        unstaged: 0,
        conflicted: 0
      },
      capturedAt: "2026-07-17T00:00:00Z"
    } satisfies RepositorySnapshot;

    expect(repositorySnapshotSchema.parse(task1Snapshot).headSha).toBe("abc123");
    expect(
      repositoryOperationResponseSchema.parse({
        operationId: projectId,
        repositoryId,
        status: "completed",
        snapshot: task1Snapshot
      }).snapshot?.summary.total
    ).toBe(1);
  });

  it("requires canonical Git DTO fields", () => {
    expect(() =>
      projectSchema.parse({
        id: "87a31769-8aaa-47ca-bef3-47e66f0c62fc",
        name: "Demo",
        path: "C:/demo",
        createdAt: "2026-07-17T00:00:00Z",
        updatedAt: "2026-07-17T00:00:00Z"
      })
    ).toThrow();
    expect(() =>
      repositorySchema.parse({
        id: "e3d622f1-f1f7-4f7e-8f18-3db8a1e6ffbe",
        name: "Repo",
        path: "C:/repo",
        workspaceIds: [],
        createdAt: "2026-07-17T00:00:00Z",
        updatedAt: "2026-07-17T00:00:00Z"
      })
    ).toThrow();
  });

  it("requires stable job and error codes", () => {
    expect(
      jobSchema.parse({
        id: "a032bc9c-8759-45ac-856f-b76f9addb9d1",
        kind: "system.echo",
        title: "Echo hello",
        status: "queued",
        progress: 0,
        cancelRequested: false,
        createdAt: "2026-07-17T00:00:00Z",
        updatedAt: "2026-07-17T00:00:00Z",
        error: null
      }).status
    ).toBe("queued");
    expect(
      errorEnvelopeSchema.parse({
        code: "permission.denied",
        category: "userActionRequired",
        message: "Permission denied",
        operationId: null,
        pluginId: "git-ramus.welcome",
        resourceId: "echo",
        failedStep: "rpc.authorization",
        retryable: false,
        retryAfterMs: null,
        recoveryActions: [
          {
            id: "review-plugin-permissions",
            label: "Review plugin permissions",
            kind: "openSettings"
          }
        ],
        details: null
      }).code
    ).toBe("permission.denied");
  });
});
