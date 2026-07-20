import { beforeEach, describe, expect, it, vi } from "vitest";
import { cleanupTransportFixture, seedTransportFixture } from "./fixture-transport";

const { execute } = vi.hoisted(() => ({ execute: vi.fn() }));

vi.mock("@wdio/globals", () => ({ browser: { execute } }));

const fixture = {
  projectId: "87a31769-8aaa-47ca-bef3-47e66f0c62fc",
  projectName: "E2E Transport",
  repositoryName: "private-skill",
  branchName: "main",
  remoteName: "origin",
  cleanupToken: "f84223af-c753-4209-be36-12d381375fcb"
} as const;

describe("Transport E2E fixture boundary", () => {
  beforeEach(() => execute.mockReset());

  it("accepts only opaque IDs and fixed repository metadata", async () => {
    execute.mockResolvedValue({ ok: true, value: fixture });

    await expect(seedTransportFixture()).resolves.toEqual(fixture);
  });

  it("rejects an arbitrary fixture path", async () => {
    execute.mockResolvedValue({
      ok: true,
      value: { ...fixture, rootPath: "C:/must-not-cross-the-boundary" }
    });

    await expect(seedTransportFixture()).rejects.toThrow("unexpected fields");
  });

  it("cleans up by opaque token without receiving a deletion path", async () => {
    execute.mockResolvedValue({ ok: true, value: undefined });

    await cleanupTransportFixture(fixture);

    expect(execute).toHaveBeenCalledTimes(1);
    expect(execute.mock.calls[0]?.slice(1)).toEqual([
      "e2e_cleanup_transport_fixture",
      { request: { cleanupToken: fixture.cleanupToken } }
    ]);
    expect(JSON.stringify(execute.mock.calls)).not.toMatch(/rootPath|[A-Z]:[\\/]|\/home\//u);
  });
});
