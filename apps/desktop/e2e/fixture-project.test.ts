import { access, mkdtemp, rm, stat } from "node:fs/promises";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { beforeEach, describe, expect, it, vi } from "vitest";
import {
  cleanupFixture,
  cleanupGitClientJourney,
  E2E_TEMP_PREFIX,
  type GitClientFixture
} from "./fixture-project";

const { execute } = vi.hoisted(() => ({ execute: vi.fn() }));

vi.mock("@wdio/globals", () => ({ browser: { execute } }));

describe("E2E fixture cleanup", () => {
  beforeEach(() => execute.mockReset());

  it("attempts every Project deletion and the guarded root removal before aggregating failures", async () => {
    const rootPath = await mkdtemp(join(tmpdir(), E2E_TEMP_PREFIX));
    const fixture = createFixture(rootPath);
    const commands: string[] = [];
    execute.mockImplementation(async (_script, command: string) => {
      commands.push(command);
      return { ok: false, error: new Error(`injected ${command} failure`) };
    });

    const rejection = cleanupFixture(fixture).catch((error: unknown) => error);

    await expect(rejection).resolves.toBeInstanceOf(AggregateError);
    await expect(rejection).resolves.toMatchObject({ errors: expect.any(Array) });
    expect(commands).toEqual(["git_project_delete", "git_project_delete"]);
    await expect(stat(rootPath)).rejects.toMatchObject({ code: "ENOENT" });
  });

  it("attempts Workspace, Identity, Projects, and root cleanup even when every step fails", async () => {
    const parent = await mkdtemp(join(tmpdir(), E2E_TEMP_PREFIX));
    const unsafeNestedRoot = await mkdtemp(join(parent, "nested-"));
    const fixture = createFixture(unsafeNestedRoot);
    const commands: string[] = [];
    execute.mockImplementation(async (_script, command: string) => {
      commands.push(command);
      return { ok: false, error: new Error(`injected ${command} failure`) };
    });

    const rejection = cleanupGitClientJourney({
      workspaceId: "a032bc9c-8759-45ac-856f-b76f9addb9d1",
      identityId: "d23957ac-5c0f-4857-9124-7f1599a41f33",
      fixture
    }).catch((error: unknown) => error);

    await expect(rejection).resolves.toBeInstanceOf(AggregateError);
    await expect(rejection).resolves.toMatchObject({ errors: { length: 5 } });
    expect(commands).toEqual([
      "git_workspace_delete",
      "git_identity_delete",
      "git_project_delete",
      "git_project_delete"
    ]);
    await expect(access(unsafeNestedRoot)).resolves.toBeUndefined();
    await rm(parent, { recursive: true, force: true });
  });
});

function createFixture(rootPath: string): GitClientFixture {
  return {
    rootPath,
    projects: [
      {
        projectId: "87a31769-8aaa-47ca-bef3-47e66f0c62fc",
        rootPath: join(rootPath, "primary"),
        name: "Primary",
        scanDepth: 3,
        excludePatterns: ["excluded"]
      },
      {
        projectId: "e3d622f1-f1f7-4f7e-8f18-3db8a1e6ffbe",
        rootPath: join(rootPath, "secondary"),
        name: "Secondary",
        scanDepth: 1,
        excludePatterns: []
      }
    ],
    primaryRepository: { displayName: "primary", relativePath: "primary" },
    secondaryRepository: { displayName: "secondary", relativePath: "secondary" },
    excludedRepository: { displayName: "excluded", relativePath: "excluded" },
    tooDeepRepository: { displayName: "too-deep", relativePath: "too-deep" },
    changes: {
      stagedPath: "staged.txt",
      stagePath: "unstaged.txt",
      remainUnstagedPath: "untracked.txt"
    }
  };
}
