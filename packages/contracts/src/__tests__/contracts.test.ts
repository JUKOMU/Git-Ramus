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
  projectSchema
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
          density: { scale: 1 }
        }
      }).type
    ).toBe("host:theme-changed");
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

  it("parses Git project DTOs with opaque UUID ids", () => {
    const project = projectSchema.parse({
      id: "87a31769-8aaa-47ca-bef3-47e66f0c62fc",
      name: "Demo",
      path: "C:/demo",
      createdAt: "2026-07-17T00:00:00Z",
      updatedAt: "2026-07-17T00:00:00Z"
    });
    expect(project.id).toBe("87a31769-8aaa-47ca-bef3-47e66f0c62fc");
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
