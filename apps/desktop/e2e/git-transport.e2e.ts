import { $, browser, expect } from "@wdio/globals";
import {
  cleanupProviderFixture,
  seedProviderFixture,
  type ProviderFixture
} from "./fixture-provider";
import { invokeHost, invokeHostResult } from "./fixture-project";
import {
  advanceTransportRemote,
  blockNextTransportFetch,
  cleanupTransportFixture,
  commitTransportLocal,
  seedTransportFixture,
  transportBlockStatus,
  transportRemoteHead,
  type TransportFixture
} from "./fixture-transport";

const GIT_CLIENT_PLUGIN_ID = "git-ramus.git-client";
const PROVIDER_CENTER_PLUGIN_ID = "git-ramus.provider-center";

describe("Git Transport native backend journey", function () {
  this.timeout(90_000);

  let providerFixture: ProviderFixture | null = null;
  let transportFixture: TransportFixture | null = null;
  let repositoryId: string | null = null;

  after(async () => {
    await switchToHost();
    const errors: unknown[] = [];
    if (repositoryId !== null) {
      await collectFailure(errors, "provider_binding_delete", {
        request: { repositoryId, remoteName: "origin" }
      });
    }
    if (transportFixture !== null) {
      await collectFailure(errors, "git_project_delete", {
        request: { projectId: transportFixture.projectId }
      });
    }
    if (providerFixture !== null) {
      try {
        await cleanupProviderFixture(providerFixture);
      } catch (error: unknown) {
        errors.push(error);
      }
    }
    if (transportFixture !== null) {
      try {
        await cleanupTransportFixture(transportFixture);
      } catch (error: unknown) {
        errors.push(error);
      }
    }
    if (errors.length > 0) throw new AggregateError(errors, "Git Transport E2E cleanup failed");
  });

  it("consumes a Provider intent, then Clones, Fetches, Pulls, Pushes, and cancels", async () => {
    providerFixture = await seedProviderFixture();
    transportFixture = await seedTransportFixture();

    await (await $("button=Providers")).click();
    const providerFrame = await $("iframe[title='Providers plugin']");
    await providerFrame.waitForDisplayed();
    await waitForFrameRpc(providerFrame, "providers.listInstances");
    // The production iframe has an opaque origin and must not be made DOM-readable for E2E.
    // Provider row -> RPC -> exact /clone/<intentId> navigation is covered as one integrated
    // dispatchPluginRpc/HostApi test; this native journey starts at the same authorized commands.
    expect(await providerFrame.getAttribute("data-plugin-rpc-methods")).not.toMatch(
      /path|destination|credential|secret/iu
    );

    const createdIntent = record(
      await invokeHost("git_clone_intent_create", {
        request: {
          pluginId: PROVIDER_CENTER_PLUGIN_ID,
          accountId: providerFixture.account.id,
          repositoryId: providerFixture.repository.repositoryId
        }
      })
    );
    const intentId = uuid(createdIntent.intentId);
    const openedIntent = record(
      await invokeHost("git_clone_intent_open", {
        request: { pluginId: PROVIDER_CENTER_PLUGIN_ID, intentId }
      })
    );
    expect(openedIntent.intentId).toBe(intentId);

    await (await $("button=Clone repository")).click();
    const gitClientFrame = await $("iframe[title='Git Client plugin']");
    await gitClientFrame.waitForDisplayed({ timeout: 15_000 });
    await expect(gitClientFrame).toHaveAttribute("sandbox", "allow-scripts");
    await expect(gitClientFrame).toHaveAttribute("data-plugin-route", "/clone");

    const intent = record(
      await invokeHost("git_clone_intent_get", {
        request: { pluginId: GIT_CLIENT_PLUGIN_ID, intentId }
      })
    );
    expect(record(intent.repository).fullName).toBe(providerFixture.repository.fullName);
    expect(JSON.stringify(intent)).not.toMatch(/canonicalPath|rootPath|destinationParent/iu);

    const destinationParent = nonEmptyText(
      await invokeHost("git_transport_select_destination_parent", {
        request: { pluginId: GIT_CLIENT_PLUGIN_ID }
      })
    );
    const cloneOperationId = crypto.randomUUID();
    const cloneResult = record(
      await invokeHost("git_repository_clone", {
        request: {
          pluginId: GIT_CLIENT_PLUGIN_ID,
          source: { kind: "intent", intentId },
          transportKind: "https",
          profileId: null,
          destinationParent,
          folderName: transportFixture.repositoryName,
          projectTarget: { kind: "existing", projectId: transportFixture.projectId },
          operationId: cloneOperationId,
          interactiveConfirmed: true
        }
      })
    );
    expect(cloneResult.operationId).toBe(cloneOperationId);
    expect(cloneResult.intentId).toBe(intentId);
    expect(record(cloneResult.job).status).toBe("succeeded");
    expect(record(cloneResult.project).id).toBe(transportFixture.projectId);

    const scan = record(
      await invokeHost("git_project_scan", {
        request: { projectId: transportFixture.projectId }
      })
    );
    const repositories = records(scan.repositories);
    expect(repositories).toHaveLength(1);
    const repository = record(repositories[0]?.repository);
    repositoryId = uuid(repository.id);
    expect(repository.displayName).toBe(transportFixture.repositoryName);
    const initialNetwork = await networkState(transportFixture.projectId, repositoryId);
    expect(record(records(initialNetwork.remotes)[0]).fetchUrl).toBe(
      "https://gitlab.example.test/skills/private-skill.git"
    );
    expect(initialNetwork.behind).toBe(0);
    expect(initialNetwork.ahead).toBe(0);

    await advanceTransportRemote(transportFixture);
    const fetchResult = await runNetworkCommand("git_repository_fetch", {
      projectId: transportFixture.projectId,
      workspaceId: null,
      repositoryId,
      remoteName: transportFixture.remoteName
    });
    expect(record(fetchResult.networkState).behind).toBe(1);
    expect(record(fetchResult.networkState).ahead).toBe(0);

    const pullResult = await runNetworkCommand("git_repository_pull", {
      projectId: transportFixture.projectId,
      workspaceId: null,
      repositoryId
    });
    expect(record(pullResult.networkState).behind).toBe(0);
    expect(record(pullResult.networkState).ahead).toBe(0);

    const localHead = await commitTransportLocal(transportFixture, repositoryId);
    const pushResult = await runNetworkCommand("git_repository_push", {
      projectId: transportFixture.projectId,
      workspaceId: null,
      repositoryId,
      target: null
    });
    expect(record(pushResult.networkState).ahead).toBe(0);
    expect(await transportRemoteHead(transportFixture)).toBe(localHead);

    await blockNextTransportFetch(transportFixture);
    const blockedOperationId = crypto.randomUUID();
    const detachedKey = crypto.randomUUID();
    await startDetachedHostCommand(detachedKey, "git_repository_fetch", {
      request: {
        pluginId: GIT_CLIENT_PLUGIN_ID,
        projectId: transportFixture.projectId,
        workspaceId: null,
        repositoryId,
        remoteName: transportFixture.remoteName,
        operationId: blockedOperationId,
        interactiveConfirmed: true
      }
    });
    await browser.waitUntil(async () => (await transportBlockStatus(transportFixture!)).connected, {
      timeout: 15_000,
      timeoutMsg: "blocked Fetch never connected to the guarded fixture server"
    });
    await invokeHost("git_transport_operation_cancel", {
      request: { pluginId: GIT_CLIENT_PLUGIN_ID, operationId: blockedOperationId }
    });
    const blockedResult = await waitForDetachedHostCommand(detachedKey);
    expect(blockedResult.status).toBe("rejected");
    expect(blockedResult.failureCode).toBe("git.transport.cancelled");
    await browser.waitUntil(async () => !(await transportBlockStatus(transportFixture!)).active, {
      timeout: 15_000,
      timeoutMsg: "canceled Fetch left a Git transport process connection alive"
    });

    const jobs = records(await invokeHost("list_jobs", {}));
    const canceledFetch = jobs.find(
      (job) =>
        job.id === blockedOperationId &&
        job.kind === "git.transport.fetch" &&
        job.status === "canceled"
    );
    expect(canceledFetch).toBeDefined();
    expect(
      jobs.some(
        (job) =>
          ["queued", "running"].includes(String(job.status)) &&
          String(job.kind).startsWith("git.transport.")
      )
    ).toBe(false);
  });
});

async function runNetworkCommand(command: string, request: Record<string, unknown>) {
  const operationId = crypto.randomUUID();
  const result = record(
    await invokeHost(command, {
      request: {
        pluginId: GIT_CLIENT_PLUGIN_ID,
        ...request,
        operationId,
        interactiveConfirmed: true
      }
    })
  );
  expect(result.operationId).toBe(operationId);
  expect(record(result.job).status).toBe("succeeded");
  return result;
}

async function switchToHost(): Promise<void> {
  try {
    await browser.switchFrame(null);
  } catch {
    await browser.switchToParentFrame();
  }
}

async function waitForFrameRpc(frame: ReturnType<typeof $>, method: string): Promise<void> {
  await browser.waitUntil(
    async () => (await frame.getAttribute("data-plugin-rpc-methods"))?.split(",").includes(method),
    { timeout: 10_000, timeoutMsg: `Plugin frame did not complete ${method}` }
  );
}

interface DetachedHostCommandResult {
  status: "pending" | "fulfilled" | "rejected";
  failureCode?: string | null;
  failureMessage?: string | null;
}

async function startDetachedHostCommand(
  key: string,
  command: string,
  args: unknown
): Promise<void> {
  const started = await browser.execute(
    (taskKey, commandName, commandArgs) => {
      type Scope = typeof window & {
        __GIT_RAMUS_E2E_DETACHED__?: Record<string, DetachedHostCommandResult>;
        __TAURI_INTERNALS__?: {
          invoke: (commandValue: string, argsValue?: unknown) => Promise<unknown>;
        };
      };
      const scope = window as Scope;
      const internals = scope.__TAURI_INTERNALS__;
      if (internals === undefined) return false;
      const tasks = (scope.__GIT_RAMUS_E2E_DETACHED__ ??= {});
      tasks[taskKey] = { status: "pending" };
      void internals.invoke(commandName, commandArgs).then(
        () => {
          tasks[taskKey] = { status: "fulfilled" };
        },
        (error: unknown) => {
          let failureCode: string | null = null;
          let failureMessage = String(error);
          if (typeof error === "object" && error !== null) {
            try {
              const candidate = error as Record<string, unknown>;
              if (typeof candidate.code === "string") failureCode = candidate.code;
              if (typeof candidate.message === "string") failureMessage = candidate.message;
            } catch {
              // Preserve the primitive fallback diagnostics.
            }
          }
          tasks[taskKey] = { status: "rejected", failureCode, failureMessage };
        }
      );
      return true;
    },
    key,
    command,
    args
  );
  if (!started) throw new Error("Tauri internals are unavailable for detached E2E command");
}

async function waitForDetachedHostCommand(key: string): Promise<DetachedHostCommandResult> {
  await browser.waitUntil(async () => (await detachedHostCommand(key))?.status !== "pending", {
    timeout: 15_000,
    timeoutMsg: "Detached host command did not finish after cancellation"
  });
  const result = await detachedHostCommand(key);
  if (result === null) throw new Error("Detached host command result is missing");
  return result;
}

async function detachedHostCommand(key: string): Promise<DetachedHostCommandResult | null> {
  return browser.execute((taskKey) => {
    const tasks = (
      window as typeof window & {
        __GIT_RAMUS_E2E_DETACHED__?: Record<string, DetachedHostCommandResult>;
      }
    ).__GIT_RAMUS_E2E_DETACHED__;
    return tasks?.[taskKey] ?? null;
  }, key);
}

async function networkState(projectId: string, repositoryIdValue: string) {
  return record(
    await invokeHost("git_repository_network_state", {
      request: {
        pluginId: GIT_CLIENT_PLUGIN_ID,
        projectId,
        repositoryId: repositoryIdValue
      }
    })
  );
}

async function collectFailure(errors: unknown[], command: string, args: unknown): Promise<void> {
  try {
    const result = await invokeHostResult(command, args);
    if (!result.ok && errorCode(result.error) !== "resource.not-found") errors.push(result.error);
  } catch (error: unknown) {
    errors.push(error);
  }
}

function record(value: unknown): Record<string, unknown> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) {
    throw new Error("Expected a native object in Git Transport E2E");
  }
  return value as Record<string, unknown>;
}

function records(value: unknown): Record<string, unknown>[] {
  if (!Array.isArray(value)) throw new Error("Expected a native array in Git Transport E2E");
  return value.map(record);
}

function uuid(value: unknown): string {
  if (
    typeof value !== "string" ||
    !/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/u.test(value)
  ) {
    throw new Error("Git Transport E2E ID is invalid");
  }
  return value;
}

function nonEmptyText(value: unknown): string {
  if (typeof value !== "string" || value.length === 0) {
    throw new Error("Git Transport E2E expected non-empty Host text");
  }
  return value;
}

function errorCode(value: unknown): string | null {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const code = (value as Record<string, unknown>).code;
  return typeof code === "string" ? code : null;
}
