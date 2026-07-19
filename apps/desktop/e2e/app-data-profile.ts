import { lstat, rm } from "node:fs/promises";
import { lstatSync, mkdirSync, mkdtempSync } from "node:fs";
import { tmpdir } from "node:os";
import { basename, dirname, isAbsolute, join, relative, resolve } from "node:path";

export const E2E_APP_DATA_PREFIX = "git-ramus-wdio-profile-";
export const E2E_APP_DATA_ROOT_ENV = "GIT_RAMUS_WDIO_PROFILE_ROOT";
export const E2E_APP_DATA_OWNER_ENV = "GIT_RAMUS_WDIO_PROFILE_OWNER";

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
  environment[E2E_APP_DATA_OWNER_ENV] = String(process.pid);
  return { ...profile, owned: true };
}

export function createE2eAppDataProfile(): E2eAppDataProfile {
  const rootPath = mkdtempSync(join(resolve(tmpdir()), E2E_APP_DATA_PREFIX));
  if (process.platform === "win32") {
    const local = join(rootPath, "local");
    const roaming = join(rootPath, "roaming");
    mkdirSync(local);
    mkdirSync(roaming);
    return {
      rootPath,
      env: {
        [E2E_APP_DATA_ROOT_ENV]: rootPath,
        LOCALAPPDATA: local,
        APPDATA: roaming
      }
    };
  }
  const data = join(rootPath, "data");
  mkdirSync(data);
  return {
    rootPath,
    env: { [E2E_APP_DATA_ROOT_ENV]: rootPath, XDG_DATA_HOME: data }
  };
}

export async function cleanupE2eAppDataProfile(rootPath: string): Promise<void> {
  const tempRoot = normalizeE2eFsPath(tmpdir());
  const target = normalizeE2eFsPath(rootPath);
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

export async function cleanupOwnedE2eAppDataProfile(
  environment: NodeJS.ProcessEnv = process.env
): Promise<void> {
  if (environment[E2E_APP_DATA_OWNER_ENV] !== String(process.pid)) return;
  const rootPath = environment[E2E_APP_DATA_ROOT_ENV];
  if (rootPath === undefined) return;
  await cleanupE2eAppDataProfile(rootPath);
  delete environment[E2E_APP_DATA_ROOT_ENV];
  delete environment[E2E_APP_DATA_OWNER_ENV];
}

export function normalizeE2eFsPath(path: string): string {
  const normalized = resolve(path);
  return process.platform === "win32" && normalized.startsWith("\\\\?\\")
    ? normalized.slice(4)
    : normalized;
}

function appDataEnvironment(rootPath: string): Record<string, string> {
  return process.platform === "win32"
    ? {
        [E2E_APP_DATA_ROOT_ENV]: rootPath,
        LOCALAPPDATA: join(rootPath, "local"),
        APPDATA: join(rootPath, "roaming")
      }
    : { [E2E_APP_DATA_ROOT_ENV]: rootPath, XDG_DATA_HOME: join(rootPath, "data") };
}

function assertSafeExistingProfile(rootPath: string): void {
  const tempRoot = normalizeE2eFsPath(tmpdir());
  const target = normalizeE2eFsPath(rootPath);
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
