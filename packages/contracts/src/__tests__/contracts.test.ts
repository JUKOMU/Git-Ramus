import { describe, expect, it } from "vitest";
import manifest from "../../../../plugins/builtin-welcome/plugin.json";
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
  identityProfileSchema,
  themeContributionSchema,
  hostThemeChangedSchema,
  themeChangedSchema
} from "../index";

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

  it("parses Git project DTOs with opaque UUID ids", () => {
    const project = projectSchema.parse({
      id: "87a31769-8aaa-47ca-bef3-47e66f0c62fc",
      name: "Demo",
      path: "C:/demo",
      rootPath: "C:/demo",
      scanDepth: 3,
      excludePatterns: ["node_modules"],
      createdAt: "2026-07-17T00:00:00Z",
      updatedAt: "2026-07-17T00:00:00Z"
    });
    expect(project.id).toBe("87a31769-8aaa-47ca-bef3-47e66f0c62fc");
  });

  it("models workspace and repository relationships as many-to-many", () => {
    expect(
      workspaceSchema.parse({
        id: "87a31769-8aaa-47ca-bef3-47e66f0c62fc",
        name: "Shared",
        projectIds: ["e3d622f1-f1f7-4f7e-8f18-3db8a1e6ffbe"],
        createdAt: "2026-07-17T00:00:00Z",
        updatedAt: "2026-07-17T00:00:00Z"
      }).projectIds
    ).toHaveLength(1);
    expect(
      repositorySchema.parse({
        id: "e3d622f1-f1f7-4f7e-8f18-3db8a1e6ffbe",
        name: "Repo",
        path: "C:/repo",
        canonicalPath: "C:/repo",
        displayName: "Repo",
        kind: "normal",
        workspaceIds: ["87a31769-8aaa-47ca-bef3-47e66f0c62fc"],
        createdAt: "2026-07-17T00:00:00Z",
        updatedAt: "2026-07-17T00:00:00Z"
      }).workspaceIds
    ).toHaveLength(1);
  });

  it("includes repository overview snapshot upstream and summary fields", () => {
    const snapshot = repositorySnapshotSchema.parse({
      id: "87a31769-8aaa-47ca-bef3-47e66f0c62fc",
      repositoryId: "e3d622f1-f1f7-4f7e-8f18-3db8a1e6ffbe",
      branch: "main",
      headSha: "abc123",
      isDirty: false,
      ahead: 0,
      behind: 0,
      changes: [],
      upstream: { branch: "origin/main", ahead: 0, behind: 0 },
      summary: { total: 0, added: 0, modified: 0, deleted: 0, untracked: 0 },
      capturedAt: "2026-07-17T00:00:00Z"
    });
    expect(snapshot.summary.total).toBe(0);
  });

  it("includes identity timestamps and signing policies", () => {
    const identity = identityProfileSchema.parse({
      id: "87a31769-8aaa-47ca-bef3-47e66f0c62fc",
      displayName: "Demo User",
      email: "demo@example.com",
      gpgFormat: "ssh",
      signingKey: "SHA256:key",
      signCommits: true,
      signTags: false,
      createdAt: "2026-07-17T00:00:00Z",
      updatedAt: "2026-07-17T00:00:00Z"
    });
    expect(identity.signCommits).toBe(true);
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
