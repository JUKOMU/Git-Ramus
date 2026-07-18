import { describe, expect, it } from "vitest";
import manifest from "../../../../plugins/builtin-welcome/plugin.json";
import { errorEnvelopeSchema, jobSchema, pluginManifestSchema, rpcRequestSchema } from "../index";

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
