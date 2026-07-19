import { lstat, rm } from "node:fs/promises";
import { lstatSync, mkdirSync, mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, isAbsolute, join, relative, resolve } from "node:path";

export const E2E_APP_DATA_PREFIX = "git-ramus-wdio-profile-";
export const E2E_APP_DATA_ROOT_ENV = "GIT_RAMUS_WDIO_PROFILE_ROOT";

export interface E2eAppDataProfile {
  rootPath: string;
  env: Record<string, string>;
}

export interface AcquiredE2eAppDataProfile extends E2eAppDataProfile {
  owned: boolean;
}

export function acquireE2eAppDataProfile(
  environment: NodeJS.ProcessEnv = process.env
): AcquiredE2eAppDataProfile {
  const existingRoot = environment[E2E_APP_DATA_ROOT_ENV];
  if (existingRoot !== undefined) {
    assertSafeExistingProfile(existingRoot);
    return { rootPath: existingRoot, env: appDataEnvironment(existingRoot), owned: false };
  }
  const profile = createE2eAppDataProfile();
  environment[E2E_APP_DATA_ROOT_ENV] = profile.rootPath;
  return { ...profile, owned: true };
}

export function createE2eAppDataProfile(): E2eAppDataProfile {
  const rootPath = mkdtempSync(join(resolve(tmpdir()), E2E_APP_DATA_PREFIX));
  if (process.platform === "win32") {
    const local = join(rootPath, "local");
    const roaming = join(rootPath, "roaming");
    mkdirSync(local);
    mkdirSync(roaming);
    return { rootPath, env: { LOCALAPPDATA: local, APPDATA: roaming } };
  }
  const data = join(rootPath, "data");
  mkdirSync(data);
  return { rootPath, env: { XDG_DATA_HOME: data } };
}

export async function cleanupE2eAppDataProfile(rootPath: string): Promise<void> {
  const tempRoot = normalizeFsPath(tmpdir());
  const target = normalizeFsPath(rootPath);
  const child = relative(tempRoot, target);
  const targetInfo = await lstat(target);
  if (
    dirname(target).toLocaleLowerCase() !== tempRoot.toLocaleLowerCase() ||
    child.length === 0 ||
    child.startsWith("..") ||
    isAbsolute(child) ||
    !basename(target).startsWith(E2E_APP_DATA_PREFIX) ||
    !targetInfo.isDirectory() ||
    targetInfo.isSymbolicLink()
  ) {
    throw new Error("Refusing to remove an unsafe E2E app-data profile");
  }
  await rm(target, { recursive: true, force: true, maxRetries: 3, retryDelay: 100 });
}

function normalizeFsPath(path: string): string {
  const normalized = resolve(path);
  return process.platform === "win32" && normalized.startsWith("\\\\?\\")
    ? normalized.slice(4)
    : normalized;
}

function appDataEnvironment(rootPath: string): Record<string, string> {
  return process.platform === "win32"
    ? { LOCALAPPDATA: join(rootPath, "local"), APPDATA: join(rootPath, "roaming") }
    : { XDG_DATA_HOME: join(rootPath, "data") };
}

function assertSafeExistingProfile(rootPath: string): void {
  const tempRoot = normalizeFsPath(tmpdir());
  const target = normalizeFsPath(rootPath);
  const child = relative(tempRoot, target);
  const targetInfo = lstatSync(target);
  if (
    dirname(target).toLocaleLowerCase() !== tempRoot.toLocaleLowerCase() ||
    child.length === 0 ||
    child.startsWith("..") ||
    isAbsolute(child) ||
    !basename(target).startsWith(E2E_APP_DATA_PREFIX) ||
    !targetInfo.isDirectory() ||
    targetInfo.isSymbolicLink()
  ) {
    throw new Error("Refusing to reuse an unsafe E2E app-data profile");
  }
}
