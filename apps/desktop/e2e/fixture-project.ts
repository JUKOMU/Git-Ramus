import { lstat, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, dirname, isAbsolute, relative, resolve } from "node:path";
import { browser } from "@wdio/globals";

export const E2E_TEMP_PREFIX = "git-ramus-e2e-";

export interface FixtureProject {
  projectId: string;
  rootPath: string;
  name: string;
  scanDepth: number;
  excludePatterns: string[];
}

export interface FixtureRepository {
  displayName: string;
  relativePath: string;
}

export interface GitClientFixture {
  rootPath: string;
  projects: [FixtureProject, FixtureProject];
  primaryRepository: FixtureRepository;
  secondaryRepository: FixtureRepository;
  excludedRepository: FixtureRepository;
  tooDeepRepository: FixtureRepository;
  changes: {
    stagedPath: string;
    stagePath: string;
    remainUnstagedPath: string;
  };
}

interface InvokeResult {
  ok: boolean;
  value?: unknown;
  error?: unknown;
}

export async function seedFixture(): Promise<GitClientFixture> {
  await cleanupStaleDatabaseFixtures();
  return parseFixture(await invokeHost("e2e_seed_fixture", {}));
}

export async function cleanupFixture(fixture: GitClientFixture): Promise<void> {
  for (const project of [...fixture.projects].reverse()) {
    const result = await invokeHostResult("git_project_delete", {
      request: { projectId: project.projectId }
    });
    if (!result.ok && recordCode(result.error) !== "resource.not-found") throw result.error;
  }
  const tempRoot = resolve(tmpdir());
  const target = resolve(fixture.rootPath);
  const child = relative(tempRoot, target);
  const targetInfo = await lstat(target);
  if (
    dirname(target).toLocaleLowerCase() !== tempRoot.toLocaleLowerCase() ||
    child.length === 0 ||
    child.startsWith("..") ||
    isAbsolute(child) ||
    !basename(target).startsWith(E2E_TEMP_PREFIX) ||
    !targetInfo.isDirectory() ||
    targetInfo.isSymbolicLink()
  ) {
    throw new Error("Refusing to remove an unsafe E2E fixture path");
  }
  await rm(target, { recursive: true, force: true, maxRetries: 3, retryDelay: 100 });
}

async function cleanupStaleDatabaseFixtures(): Promise<void> {
  const projectsResponse = await invokeHost("git_project_list", {});
  const projects = recordArrayField(projectsResponse, "projects");
  for (const project of projects) {
    const rootPath = project.rootPath;
    const projectId = project.id;
    if (typeof rootPath !== "string" || typeof projectId !== "string") continue;
    const fixtureRoot = dirname(normalizeFsPath(rootPath));
    if (isSafeFixtureRoot(fixtureRoot)) {
      await invokeHost("git_project_delete", { request: { projectId } });
    }
  }

  const workspacesResponse = await invokeHost("git_workspace_list", {});
  for (const workspace of recordArrayField(workspacesResponse, "workspaces")) {
    if (
      typeof workspace.id === "string" &&
      typeof workspace.name === "string" &&
      workspace.name.startsWith("E2E Cross Directory")
    ) {
      await invokeHost("git_workspace_delete", { request: { workspaceId: workspace.id } });
    }
  }

  const identitiesResponse = await invokeHost("git_identity_list", {});
  for (const identity of recordArrayField(identitiesResponse, "identities")) {
    if (
      typeof identity.id === "string" &&
      typeof identity.displayName === "string" &&
      identity.displayName === "E2E Identity"
    ) {
      await invokeHost("git_identity_delete", { request: { profileId: identity.id } });
    }
  }
}

export async function invokeHost(command: string, args: unknown): Promise<unknown> {
  const result = await invokeHostResult(command, args);
  if (!result.ok) throw result.error;
  return result.value;
}

export async function invokeHostResult(command: string, args: unknown): Promise<InvokeResult> {
  return browser.execute(
    async (commandName, commandArgs) => {
      const internals = (
        window as typeof window & {
          __TAURI_INTERNALS__?: {
            invoke: (command: string, args?: unknown) => Promise<unknown>;
          };
        }
      ).__TAURI_INTERNALS__;
      if (internals === undefined) {
        return { ok: false, error: { code: "e2e.tauri-internals-unavailable" } };
      }
      try {
        return { ok: true, value: await internals.invoke(commandName, commandArgs) };
      } catch (error: unknown) {
        return { ok: false, error };
      }
    },
    command,
    args
  );
}

function parseFixture(value: unknown): GitClientFixture {
  const fixture = strictRecord(value, [
    "rootPath",
    "projects",
    "primaryRepository",
    "secondaryRepository",
    "excludedRepository",
    "tooDeepRepository",
    "changes"
  ]);
  if (!Array.isArray(fixture.projects) || fixture.projects.length !== 2) {
    throw new Error("E2E fixture must contain exactly two Projects");
  }
  const projects = fixture.projects.map(parseProject) as [FixtureProject, FixtureProject];
  const parsed: GitClientFixture = {
    rootPath: nonEmptyString(fixture.rootPath),
    projects,
    primaryRepository: parseRepository(fixture.primaryRepository),
    secondaryRepository: parseRepository(fixture.secondaryRepository),
    excludedRepository: parseRepository(fixture.excludedRepository),
    tooDeepRepository: parseRepository(fixture.tooDeepRepository),
    changes: parseChanges(fixture.changes)
  };
  if (!basename(resolve(parsed.rootPath)).startsWith(E2E_TEMP_PREFIX)) {
    throw new Error("E2E fixture root has an unexpected prefix");
  }
  return parsed;
}

function parseProject(value: unknown): FixtureProject {
  const project = strictRecord(value, [
    "projectId",
    "rootPath",
    "name",
    "scanDepth",
    "excludePatterns"
  ]);
  const projectId = nonEmptyString(project.projectId);
  if (!/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/iu.test(projectId)) {
    throw new Error("E2E Project ID is not a UUID v4");
  }
  if (!Number.isInteger(project.scanDepth) || (project.scanDepth as number) < 0) {
    throw new Error("E2E scan depth is invalid");
  }
  if (!Array.isArray(project.excludePatterns) || !project.excludePatterns.every(isNonEmptyString)) {
    throw new Error("E2E exclusions are invalid");
  }
  return {
    projectId,
    rootPath: nonEmptyString(project.rootPath),
    name: nonEmptyString(project.name),
    scanDepth: project.scanDepth as number,
    excludePatterns: project.excludePatterns
  };
}

function parseRepository(value: unknown): FixtureRepository {
  const repository = strictRecord(value, ["displayName", "relativePath"]);
  return {
    displayName: nonEmptyString(repository.displayName),
    relativePath: nonEmptyString(repository.relativePath)
  };
}

function parseChanges(value: unknown): GitClientFixture["changes"] {
  const changes = strictRecord(value, ["stagedPath", "stagePath", "remainUnstagedPath"]);
  return {
    stagedPath: nonEmptyString(changes.stagedPath),
    stagePath: nonEmptyString(changes.stagePath),
    remainUnstagedPath: nonEmptyString(changes.remainUnstagedPath)
  };
}

function strictRecord(value: unknown, keys: string[]): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error("E2E fixture response is not an object");
  }
  const record = value as Record<string, unknown>;
  const actual = Object.keys(record).sort();
  const expected = [...keys].sort();
  if (actual.length !== expected.length || actual.some((key, index) => key !== expected[index])) {
    throw new Error("E2E fixture response has unexpected fields");
  }
  return record;
}

function nonEmptyString(value: unknown): string {
  if (!isNonEmptyString(value)) throw new Error("E2E fixture string is empty");
  return value;
}

function isNonEmptyString(value: unknown): value is string {
  return typeof value === "string" && value.length > 0;
}

function recordArrayField(value: unknown, field: string): Record<string, unknown>[] {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return [];
  const entries = (value as Record<string, unknown>)[field];
  if (!Array.isArray(entries)) return [];
  return entries.filter(
    (entry): entry is Record<string, unknown> =>
      typeof entry === "object" && entry !== null && !Array.isArray(entry)
  );
}

function isSafeFixtureRoot(rootPath: string): boolean {
  const tempRoot = normalizeFsPath(tmpdir());
  const normalizedRoot = normalizeFsPath(rootPath);
  return (
    dirname(normalizedRoot).toLocaleLowerCase() === tempRoot.toLocaleLowerCase() &&
    basename(normalizedRoot).startsWith(E2E_TEMP_PREFIX)
  );
}

function normalizeFsPath(path: string): string {
  const normalized = resolve(path);
  return process.platform === "win32" && normalized.startsWith("\\\\?\\")
    ? normalized.slice(4)
    : normalized;
}

function recordCode(value: unknown): string | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const code = (value as Record<string, unknown>).code;
  return typeof code === "string" ? code : null;
}
