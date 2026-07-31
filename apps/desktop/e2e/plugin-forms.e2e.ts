import { $, browser, expect } from "@wdio/globals";
import { lstat } from "node:fs/promises";
import { resolve } from "node:path";
import { canonicalizeExistingE2eFsPath, E2E_APP_DATA_ROOT_ENV } from "./app-data-profile";
import { cleanupGitClientJourney, invokeHost } from "./fixture-project";

const identity = {
  displayName: "E2E Sandboxed Identity",
  userName: "Git-Ramus E2E",
  userEmail: "sandboxed-identity@example.invalid"
};

describe("Sandboxed plugin forms", () => {
  let identityId: string | null = null;

  after(async () => {
    await browser.switchFrame(null);
    await cleanupGitClientJourney({ workspaceId: null, identityId, fixture: null });
  });

  it("submits Identity creation through the scripts-only plugin frame", async () => {
    await assertIsolatedAppData();
    const shell = await $("[data-testid='app-shell']");
    await shell.waitForDisplayed({ timeout: 15_000, timeoutMsg: "Git-Ramus shell did not load" });
    const identitiesNavigation = await $("button=Identities");
    await identitiesNavigation.waitForClickable({
      timeout: 15_000,
      timeoutMsg: "Identity navigation did not become ready"
    });
    await identitiesNavigation.click();
    const frame = await $("iframe[title='Git Client plugin']");
    await frame.waitForDisplayed();
    await expect(frame).toHaveAttribute("sandbox", "allow-scripts");
    await expect(frame).toHaveAttribute("data-plugin-route", "/identities");
    await waitForFrameRpc(frame, "identities.list");

    await browser.switchFrame(frame);
    const profileName = await $("[aria-label='Profile name']");
    await profileName.waitForDisplayed({
      timeout: 10_000,
      timeoutMsg: "Identity form did not load inside the sandbox"
    });
    await profileName.setValue(identity.displayName);
    await (await $("[aria-label='Git user name']")).setValue(identity.userName);
    await (await $("[aria-label='Git user email']")).setValue(identity.userEmail);

    const makeGlobal = await $("aria/Set as global identity");
    if (await makeGlobal.isSelected()) await makeGlobal.click();
    expect(await makeGlobal.isSelected()).toBe(false);

    const create = await $("button=Create identity");
    await expect(create).toBeEnabled();
    await create.click();
    await browser.switchFrame(null);

    await browser.waitUntil(
      async () =>
        (await frame.getAttribute("data-plugin-last-rpc-method")) === "identities.create" &&
        (await frame.getAttribute("data-plugin-last-rpc-status")) === "complete",
      {
        timeout: 10_000,
        timeoutMsg: "Identity form did not complete identities.create through the sandbox"
      }
    );

    const response = record(await invokeHost("git_identity_list", {}));
    const matches = records(response.identities).filter(
      (candidate) =>
        candidate.displayName === identity.displayName && candidate.userEmail === identity.userEmail
    );
    expect(matches.length).toBe(1);
    identityId = nonEmptyString(matches[0]?.id);
    expect(response.globalIdentityProfileId).not.toBe(identityId);
  });
});

async function assertIsolatedAppData(): Promise<void> {
  const expectedRoot = process.env[E2E_APP_DATA_ROOT_ENV];
  if (expectedRoot === undefined) throw new Error("E2E app-data profile is unavailable");
  const paths = record(await invokeHost("e2e_app_data_paths", {}));
  const [appDataRoot, databasePath, expectedAppDataRoot, expectedDatabasePath] = await Promise.all([
    canonicalizeExistingE2eFsPath(nonEmptyString(paths.appDataRoot)),
    canonicalizeExistingE2eFsPath(nonEmptyString(paths.databasePath)),
    canonicalizeExistingE2eFsPath(expectedRoot),
    canonicalizeExistingE2eFsPath(resolve(expectedRoot, "git-ramus.db"))
  ]);
  expect(appDataRoot).toBe(expectedAppDataRoot);
  expect(databasePath).toBe(expectedDatabasePath);
  expect((await lstat(databasePath)).isFile()).toBe(true);
}

async function waitForFrameRpc(frame: ReturnType<typeof $>, method: string): Promise<void> {
  await browser.waitUntil(
    async () => (await frame.getAttribute("data-plugin-rpc-methods"))?.split(",").includes(method),
    { timeout: 10_000, timeoutMsg: `Git Client did not complete ${method}` }
  );
}

function record(value: unknown): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error("Expected an object from the production command");
  }
  return value as Record<string, unknown>;
}

function records(value: unknown): Record<string, unknown>[] {
  if (!Array.isArray(value)) throw new Error("Expected an array from the production command");
  return value.map(record);
}

function nonEmptyString(value: unknown): string {
  if (typeof value !== "string" || value.length === 0) {
    throw new Error("Expected a non-empty string from the production command");
  }
  return value;
}
