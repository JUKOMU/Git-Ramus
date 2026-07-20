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

export interface GitClientJourneyResources {
  workspaceId: string | null;
  identityId: string | null;
  fixture: GitClientFixture | null;
}

interface InvokeResult {
  ok: boolean;
  value?: unknown;
  error?: unknown;
}

interface InvokeWireResult {
  ok: boolean;
  value?: unknown;
  code?: string | null;
  message?: string;
}

export async function seedFixture(): Promise<GitClientFixture> {
  return parseFixture(await invokeHost("e2e_seed_fixture", {}));
}

export async function cleanupFixture(fixture: GitClientFixture): Promise<void> {
  const errors: unknown[] = [];
  for (const project of [...fixture.projects].reverse()) {
    try {
      const result = await invokeHostResult("git_project_delete", {
        request: { projectId: project.projectId }
      });
      if (!result.ok && recordCode(result.error) !== "resource.not-found") {
        errors.push(result.error);
      }
    } catch (error: unknown) {
      errors.push(error);
    }
  }
  try {
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
  } catch (error: unknown) {
    errors.push(error);
  }
  if (errors.length > 0) {
    throw new AggregateError(errors, "E2E fixture cleanup failed");
  }
}

export async function cleanupGitClientJourney(resources: GitClientJourneyResources): Promise<void> {
  const errors: unknown[] = [];
  if (resources.workspaceId !== null) {
    await collectDeleteFailure(errors, "git_workspace_delete", {
      request: { workspaceId: resources.workspaceId }
    });
  }
  if (resources.identityId !== null) {
    await collectDeleteFailure(errors, "git_identity_delete", {
      request: { profileId: resources.identityId }
    });
  }
  if (resources.fixture !== null) {
    try {
      await cleanupFixture(resources.fixture);
    } catch (error: unknown) {
      collectError(errors, error);
    }
  }
  if (errors.length > 0) {
    throw new AggregateError(errors, "Git Client E2E cleanup failed");
  }
}

async function collectDeleteFailure(
  errors: unknown[],
  command: string,
  args: unknown
): Promise<void> {
  try {
    const result = await invokeHostResult(command, args);
    if (!result.ok && recordCode(result.error) !== "resource.not-found") {
      errors.push(result.error);
    }
  } catch (error: unknown) {
    errors.push(error);
  }
}

function collectError(errors: unknown[], error: unknown): void {
  if (error instanceof AggregateError) {
    errors.push(...error.errors);
  } else {
    errors.push(error);
  }
}

export async function invokeHost(command: string, args: unknown): Promise<unknown> {
  const result = await invokeHostResult(command, args);
  if (!result.ok) throw result.error;
  return result.value;
}

export async function invokeHostResult(command: string, args: unknown): Promise<InvokeResult> {
  const result = await browser.execute(
    async (commandName, commandArgs) => {
      const internals = (
        window as typeof window & {
          __TAURI_INTERNALS__?: {
            invoke: (command: string, args?: unknown) => Promise<unknown>;
          };
        }
      ).__TAURI_INTERNALS__;
      if (internals === undefined) {
        return {
          ok: false,
          code: "e2e.tauri-internals-unavailable",
          message: "Tauri internals are unavailable"
        };
      }
      try {
        return { ok: true, value: await internals.invoke(commandName, commandArgs) };
      } catch (error: unknown) {
        let code: string | null = null;
        let message = String(error);
        if (typeof error === "object" && error !== null) {
          try {
            const candidate = error as Record<string, unknown>;
            if (typeof candidate.code === "string") code = candidate.code;
            if (typeof candidate.message === "string") message = candidate.message;
          } catch {
            // Preserve the primitive fallback diagnostics.
          }
        }
        return { ok: false, code, message };
      }
    },
    command,
    args
  );
  const wire = result as InvokeWireResult;
  return wire.ok
    ? { ok: true, value: wire.value }
    : { ok: false, error: { code: wire.code ?? null, message: wire.message ?? "Host failed" } };
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

function recordCode(value: unknown): string | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const code = (value as Record<string, unknown>).code;
  return typeof code === "string" ? code : null;
}
