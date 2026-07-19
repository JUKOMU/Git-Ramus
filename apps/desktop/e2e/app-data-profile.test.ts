import { access, mkdtemp, rm } from "node:fs/promises";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { describe, expect, it } from "vitest";
import {
  acquireE2eAppDataProfile,
  cleanupE2eAppDataProfile,
  createE2eAppDataProfile,
  E2E_APP_DATA_PREFIX
} from "./app-data-profile";

describe("E2E app-data profile", () => {
  it("reuses one safe profile across launcher and worker configuration loads", async () => {
    const environment: NodeJS.ProcessEnv = {};
    const launcher = acquireE2eAppDataProfile(environment);
    const worker = acquireE2eAppDataProfile(environment);
    expect(worker.rootPath).toBe(launcher.rootPath);
    expect(launcher.owned).toBe(true);
    expect(worker.owned).toBe(false);
    await cleanupE2eAppDataProfile(launcher.rootPath);
  });

  it("creates a unique direct temp child with platform-specific app-data variables", async () => {
    const profile = await createE2eAppDataProfile();
    expect(dirname(resolve(profile.rootPath)).toLocaleLowerCase()).toBe(
      resolve(tmpdir()).toLocaleLowerCase()
    );
    expect(profile.rootPath.split(/[\\/]/u).at(-1)).toMatch(new RegExp(`^${E2E_APP_DATA_PREFIX}`));
    if (process.platform === "win32") {
      expect(profile.env).toEqual({
        LOCALAPPDATA: join(profile.rootPath, "local"),
        APPDATA: join(profile.rootPath, "roaming")
      });
    } else {
      expect(profile.env).toEqual({ XDG_DATA_HOME: join(profile.rootPath, "data") });
    }
    await cleanupE2eAppDataProfile(profile.rootPath);
    await expect(access(profile.rootPath)).rejects.toMatchObject({ code: "ENOENT" });
  });

  it("rejects a nested cleanup target without removing it", async () => {
    const parent = await mkdtemp(join(tmpdir(), E2E_APP_DATA_PREFIX));
    const nested = await mkdtemp(join(parent, "nested-"));
    await expect(cleanupE2eAppDataProfile(nested)).rejects.toThrow(
      "Refusing to remove an unsafe E2E app-data profile"
    );
    await expect(access(nested)).resolves.toBeUndefined();
    await rm(parent, { recursive: true, force: true });
  });
});
