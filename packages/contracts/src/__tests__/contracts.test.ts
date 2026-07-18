import { describe, expect, it } from "vitest";
import manifest from "../../../../plugins/builtin-welcome/plugin.json";
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
  projectListResponseSchema,
  projectUpdateScanRulesRequestSchema,
  repositoryStageRequestSchema,
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
  themeChangedSchema
} from "../index";

const projectId = "87a31769-8aaa-47ca-bef3-47e66f0c62fc";
const workspaceId = "e3d622f1-f1f7-4f7e-8f18-3db8a1e6ffbe";
const repositoryId = "a032bc9c-8759-45ac-856f-b76f9addb9d1";
const profileId = "d23957ac-5c0f-4857-9124-7f1599a41f33";

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
