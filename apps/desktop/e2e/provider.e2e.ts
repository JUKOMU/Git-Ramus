import { execFile } from "node:child_process";
import { promisify } from "node:util";
import { $, browser, expect } from "@wdio/globals";
import { resolve } from "node:path";
import {
  cleanupGitClientJourney,
  invokeHost,
  seedFixture,
  type GitClientFixture
} from "./fixture-project";
import {
  cleanupProviderFixture,
  seedProviderFixture,
  type ProviderFixture
} from "./fixture-provider";

const execFileAsync = promisify(execFile);

const defaultQuery = {
  search: "",
  visibility: null,
  namespace: null,
  archived: "all",
  sort: "name",
  direction: "asc",
  pageSize: 30
} as const;

describe("Provider discovery vertical slice", () => {
  let gitFixture: GitClientFixture | null = null;
  let providerFixture: ProviderFixture | null = null;

  after(async () => {
    const errors: unknown[] = [];
    if (providerFixture !== null) {
      try {
        await cleanupProviderFixture(providerFixture);
      } catch (error: unknown) {
        errors.push(error);
      }
    }
    try {
      await cleanupGitClientJourney({ workspaceId: null, identityId: null, fixture: gitFixture });
    } catch (error: unknown) {
      errors.push(error);
    }
    if (errors.length > 0) throw new AggregateError(errors, "Provider E2E cleanup failed");
  });

  it("discovers a private GitLab repository and confirms a local remote binding", async () => {
    gitFixture = await seedFixture();
    const primaryProject = gitFixture.projects[0];
    const scan = record(
      await invokeHost("git_project_scan", {
        request: { projectId: primaryProject.projectId }
      })
    );
    const localRepository = record(records(scan.repositories)[0]?.repository);

    providerFixture = await seedProviderFixture();
    await (await $("button=Providers")).click();
    const frame = await $("iframe[title='Providers plugin']");
    await frame.waitForDisplayed();
    await waitForFrameRpc(frame, "providers.listInstances");

    const activated = record(
      await invokeHost("activate_theme", {
        request: { themeId: "git-ramus.theme.compact" }
      })
    );
    expect(activated.activeThemeId).toBe("git-ramus.theme.compact");
    await expect(frame).toHaveAttribute("data-plugin-theme-id", "git-ramus.theme.compact");

    const page = record(
      await invokeHost("provider_repository_list", {
        request: {
          pluginId: "git-ramus.provider-center",
          accountId: providerFixture.account.id,
          query: defaultQuery,
          cursor: null,
          operationId: crypto.randomUUID()
        }
      })
    );
    expect(records(page.items).map((item) => item.fullName)).toContain("skills/private-skill");

    const suggestionPage = record(
      await invokeHost("provider_local_remote_match", {
        request: {
          pluginId: "git-ramus.provider-center",
          instanceId: providerFixture.instance.id,
          accountId: providerFixture.account.id,
          operationId: crypto.randomUUID()
        }
      })
    );
    const suggestion = records(suggestionPage.items).find((item) => item.status === "suggested");
    if (suggestion === undefined) throw new Error("Expected one verified Provider suggestion");
    const providerRepositoryId = text(suggestion.providerRepositoryId);

    await invokeHost("provider_binding_set", {
      request: {
        repositoryId: text(localRepository.id),
        remoteName: "origin",
        instanceId: providerFixture.instance.id,
        accountId: null,
        providerRepositoryId
      }
    });
    const bindingPage = record(
      await invokeHost("provider_binding_list", {
        request: { accountId: providerFixture.account.id }
      })
    );
    expect(records(bindingPage.items)).toHaveLength(1);

    const repositoryRoot = resolve(
      primaryProject.rootPath,
      gitFixture.primaryRepository.relativePath
    );
    const { stdout } = await execFileAsync("git", [
      "-C",
      repositoryRoot,
      "remote",
      "get-url",
      "origin"
    ]);
    expect(stdout.trim()).toBe("git@gitlab.example.test:skills/private-skill.git");
  });
});

async function waitForFrameRpc(frame: ReturnType<typeof $>, method: string): Promise<void> {
  await browser.waitUntil(
    async () => (await frame.getAttribute("data-plugin-rpc-methods"))?.split(",").includes(method),
    { timeout: 10_000, timeoutMsg: `Provider Center did not complete ${method}` }
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
