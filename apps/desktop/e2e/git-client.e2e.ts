import { $, browser, expect } from "@wdio/globals";
import { cleanupFixture, invokeHost, seedFixture, type GitClientFixture } from "./fixture-project";

describe("Git Client vertical slice", () => {
  let fixture: GitClientFixture | null = null;
  let workspaceId: string | null = null;
  let identityId: string | null = null;

  after(async () => {
    if (workspaceId !== null) {
      await invokeHost("git_workspace_delete", { request: { workspaceId } });
    }
    if (identityId !== null) {
      await invokeHost("git_identity_delete", { request: { profileId: identityId } });
    }
    if (fixture !== null) await cleanupFixture(fixture);
  });

  it("scans, groups, trusts, stages, commits, and updates one opaque plugin frame theme", async () => {
    await (await $("button=Overview")).click();
    let frame = await $("iframe[title='Git Client plugin']");
    await frame.waitForDisplayed();
    await expect(frame).toHaveAttribute("sandbox", "allow-scripts");
    await waitForFrameRpc(frame, "projects.list");

    fixture = await seedFixture();
    const [primaryProject, secondaryProject] = fixture.projects;
    const primaryScan = record(
      await invokeHost("git_project_scan", { request: { projectId: primaryProject.projectId } })
    );
    const secondaryScan = record(
      await invokeHost("git_project_scan", { request: { projectId: secondaryProject.projectId } })
    );
    const primaryRepositories = records(primaryScan.repositories);
    const secondaryRepositories = records(secondaryScan.repositories);
    expect(primaryRepositories.map(repositoryName)).toEqual([
      fixture.primaryRepository.displayName
    ]);
    expect(secondaryRepositories.map(repositoryName)).toEqual([
      fixture.secondaryRepository.displayName
    ]);
    expect(primaryRepositories.map(repositoryName)).not.toContain(
      fixture.excludedRepository.displayName
    );
    expect(primaryRepositories.map(repositoryName)).not.toContain(
      fixture.tooDeepRepository.displayName
    );

    const primaryRepository = record(primaryRepositories[0]?.repository);
    const primaryRepositoryId = text(primaryRepository.id);
    const secondaryRepositoryId = text(record(secondaryRepositories[0]?.repository).id);
    const initial = record(
      await invokeHost("git_repository_snapshot", {
        request: { projectId: primaryProject.projectId, repositoryId: primaryRepositoryId }
      })
    );
    const initialSnapshot = record(initial.snapshot);
    expect(initialSnapshot.stagedCount).toBe(1);
    expect(initialSnapshot.unstagedCount).toBe(1);
    expect(initialSnapshot.untrackedCount).toBe(1);

    const workspace = record(
      await invokeHost("git_workspace_create", { request: { name: "E2E Cross Directory" } })
    );
    workspaceId = text(workspace.id);
    await invokeHost("git_workspace_update_membership", {
      request: {
        workspaceId,
        projectIds: [primaryProject.projectId, secondaryProject.projectId]
      }
    });
    const overview = record(await invokeHost("git_overview_get", { request: { workspaceId } }));
    expect(overview.repositoryCount).toBe(2);

    await (await $("button=Projects")).click();
    frame = await $("iframe[title='Git Client plugin']");
    await expect(frame).toHaveAttribute("data-plugin-route", "/projects");
    await waitForFrameRpc(frame, "projects.list");

    const context = { projectId: primaryProject.projectId, repositoryId: primaryRepositoryId };
    const detailChanges = record(await invokeHost("git_repository_changes", { request: context }));
    expect(
      records(detailChanges.changes)
        .map((change) => text(change.path))
        .sort()
    ).toEqual(
      [
        fixture.changes.stagedPath,
        fixture.changes.stagePath,
        fixture.changes.remainUnstagedPath
      ].sort()
    );
    const untrusted = record(await invokeHost("git_repository_trust_status", { request: context }));
    expect(untrusted.trusted).toBe(false);
    await invokeHost("git_repository_trust", { request: context });
    const trusted = record(await invokeHost("git_repository_trust_status", { request: context }));
    expect(trusted.trusted).toBe(true);
    const staged = record(
      await invokeHost("git_repository_stage", {
        request: { ...context, paths: [fixture.changes.stagePath], all: false }
      })
    );
    const stagedSnapshot = record(staged.snapshot);
    expect(stagedSnapshot.stagedCount).toBe(2);
    expect(stagedSnapshot.untrackedCount).toBe(1);

    const identity = record(
      await invokeHost("git_identity_create", {
        request: {
          displayName: "E2E Identity",
          userName: "Git-Ramus E2E",
          userEmail: "e2e@example.invalid",
          gpgFormat: null,
          signingKey: null,
          signCommits: false,
          signTags: false
        }
      })
    );
    identityId = text(identity.id);
    const committed = record(
      await invokeHost("git_repository_commit", {
        request: {
          ...context,
          message: "Commit from Git Client E2E",
          identityProfileId: identityId
        }
      })
    );
    expect(text(committed.output)).toContain("Commit from Git Client E2E");
    const committedSnapshot = record(committed.snapshot);
    expect(text(committedSnapshot.headOid)).toMatch(/^[0-9a-f]{40,64}$/u);
    expect(committedSnapshot.headOid).not.toBe(initialSnapshot.headOid);
    expect(committedSnapshot.stagedCount).toBe(0);
    expect(committedSnapshot.untrackedCount).toBe(1);

    const frameId = frame.elementId;
    const activated = record(
      await invokeHost("activate_theme", {
        request: { themeId: "git-ramus.theme.compact" }
      })
    );
    expect(activated.activeThemeId).toBe("git-ramus.theme.compact");
    const shell = await $("[data-testid='app-shell']");
    await expect(shell).toHaveAttribute("data-theme-id", "git-ramus.theme.compact");
    await expect(shell).toHaveAttribute("data-theme-density", "compact");
    await expect(frame).toHaveAttribute("data-plugin-theme-id", "git-ramus.theme.compact");
    await expect(frame).toHaveAttribute("data-plugin-theme-density", "compact");
    expect(frame.elementId).toBe(frameId);

    expect(secondaryRepositoryId).not.toBe(primaryRepositoryId);
  });
});

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

function text(value: unknown): string {
  if (typeof value !== "string" || value.length === 0) {
    throw new Error("Expected a non-empty string from the production command");
  }
  return value;
}

function repositoryName(value: Record<string, unknown>): string {
  return text(record(value.repository).displayName);
}
