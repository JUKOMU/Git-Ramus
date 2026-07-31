import "@testing-library/jest-dom/vitest";
import { act, cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { networkOperationResultSchema } from "@git-ramus/contracts";
import type { TransportProfileDeletionImpact, TransportProfileSummary } from "@git-ramus/contracts";
import type { PluginClient } from "@git-ramus/plugin-sdk";
import transportContracts from "../../../../packages/contracts/src/__fixtures__/transport-contracts.json";
import type { GitClientApi } from "../api";
import { createGitClientApi } from "../api";
import { App } from "../App";
import { TransportProfilesView, type TransportProfilesApi } from "../views/TransportProfilesView";

const projectId = "3b84198e-bb1a-4f0d-875f-d82f0c18c630";
const repositoryId = transportContracts.networkState.repositoryId;
const operationId = transportContracts.cloneResult.operationId;
const sshProfile = transportContracts.sshProfile as TransportProfileSummary;
const httpsProfile = transportContracts.httpsProfile as TransportProfileSummary;
const replacementSshProfile: TransportProfileSummary = {
  ...sshProfile,
  id: "65c95b43-e363-49f9-aaeb-80045a5f0904",
  displayName: "Backup SSH",
  boundRepositoryCount: 0
};

afterEach(cleanup);

describe("TransportProfilesView", () => {
  it("creates HTTPS and SSH profiles without rendering a secret or full key path", async () => {
    const user = userEvent.setup();
    const api = transportProfileApi([]);
    vi.mocked(api.createTransportProfile)
      .mockResolvedValueOnce(httpsProfile)
      .mockResolvedValueOnce(sshProfile);
    render(<App api={api as GitClientApi} route="/transport-identities" />);

    expect(await screen.findByText("System Git configuration")).toBeInTheDocument();
    expect(await screen.findByText("No transport profiles are configured.")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "New HTTPS profile" }));
    expect(screen.getByLabelText("Profile name")).toHaveAttribute("maxlength", "128");
    await user.type(screen.getByLabelText("Profile name"), "Work HTTPS");
    await user.type(screen.getByLabelText("HTTPS username"), "creator");
    const createHttps = screen.getByRole("button", { name: "Save profile" });
    expect(createHttps).toHaveAttribute("type", "button");
    await user.click(createHttps);
    expect(api.createTransportProfile).toHaveBeenNthCalledWith(1, {
      kind: "https",
      displayName: "Work HTTPS",
      username: "creator",
      useHttpPath: true
    });

    await user.click(screen.getByRole("button", { name: "New SSH profile" }));
    await user.type(screen.getByLabelText("Profile name"), "Work SSH");
    expect(
      screen.getByRole("checkbox", { name: "Use only the selected SSH identity" })
    ).toBeChecked();
    await user.click(screen.getByRole("button", { name: "Choose private key" }));
    expect(api.createTransportProfile).toHaveBeenNthCalledWith(2, {
      kind: "ssh",
      displayName: "Work SSH",
      identitiesOnly: true,
      sshKeyAction: "selectFile"
    });

    expect(await screen.findByText("id_ed25519")).toBeInTheDocument();
    expect(document.body).not.toHaveTextContent(/Users.*\.ssh|C:\\|\/home\//u);
    expect(screen.queryByLabelText(/password|token|passphrase/iu)).not.toBeInTheDocument();
    expect(
      screen.queryByRole("textbox", { name: /private key|key path/iu })
    ).not.toBeInTheDocument();
  });

  it("updates an HTTPS profile while keeping useHttpPath enabled", async () => {
    const user = userEvent.setup();
    const api = transportProfileApi([httpsProfile]);
    const updated = {
      ...httpsProfile,
      displayName: "Primary HTTPS",
      httpsUsername: "release-bot"
    };
    vi.mocked(api.updateTransportProfile).mockResolvedValue(updated);
    render(<TransportProfilesView api={api} />);

    await user.click(await screen.findByRole("button", { name: "Edit Work HTTPS" }));
    await user.clear(screen.getByLabelText("Profile name"));
    await user.type(screen.getByLabelText("Profile name"), updated.displayName);
    await user.clear(screen.getByLabelText("HTTPS username"));
    await user.type(screen.getByLabelText("HTTPS username"), updated.httpsUsername);
    const saveHttps = screen.getByRole("button", { name: "Save profile" });
    expect(saveHttps).toHaveAttribute("type", "button");
    await user.click(saveHttps);

    expect(api.updateTransportProfile).toHaveBeenCalledWith({
      kind: "https",
      profileId: httpsProfile.id,
      displayName: updated.displayName,
      username: updated.httpsUsername,
      useHttpPath: true
    });
    expect(await screen.findByRole("heading", { name: updated.displayName })).toBeInTheDocument();
  });

  it("preserves a disabled IdentitiesOnly setting during an SSH edit", async () => {
    const user = userEvent.setup();
    const nonExclusiveProfile = { ...sshProfile, sshIdentitiesOnly: false };
    const api = transportProfileApi([nonExclusiveProfile]);
    vi.mocked(api.updateTransportProfile).mockResolvedValue(nonExclusiveProfile);
    render(<TransportProfilesView api={api} />);

    await user.click(await screen.findByRole("button", { name: "Edit Work SSH" }));
    expect(
      screen.getByRole("checkbox", { name: "Use only the selected SSH identity" })
    ).not.toBeChecked();
    const saveSsh = screen.getByRole("button", { name: "Save profile" });
    expect(saveSsh).toHaveAttribute("type", "button");
    await user.click(saveSsh);

    expect(api.updateTransportProfile).toHaveBeenCalledWith({
      kind: "ssh",
      profileId: sshProfile.id,
      displayName: sshProfile.displayName,
      identitiesOnly: false,
      sshKeyAction: "keep"
    });
  });

  it("keeps the SSH editor open without an error when key selection is cancelled", async () => {
    const user = userEvent.setup();
    const api = transportProfileApi([]);
    vi.mocked(api.createTransportProfile).mockResolvedValue(null);
    render(<TransportProfilesView api={api} />);

    await screen.findByText("No transport profiles are configured.");
    await user.click(screen.getByRole("button", { name: "New SSH profile" }));
    await user.type(screen.getByLabelText("Profile name"), "Cancelled SSH");
    await user.click(screen.getByRole("button", { name: "Choose private key" }));

    expect(await screen.findByText("Private key selection cancelled.")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Profile name")).toHaveValue("Cancelled SSH");
    expect(screen.getByRole("button", { name: "Choose private key" })).toBeEnabled();
  });

  it("requires a resolution for every affected repository before deleting a profile", async () => {
    const user = userEvent.setup();
    const api = transportProfileApi([sshProfile, replacementSshProfile, httpsProfile]);
    const impact: TransportProfileDeletionImpact = {
      profileId: sshProfile.id,
      repositories: [
        {
          repositoryId,
          displayName: "private-skill",
          transportKind: "ssh"
        },
        {
          repositoryId: "2a2968e7-1a33-4f41-b0ed-f507cb636b51",
          displayName: "docs-site",
          transportKind: "ssh"
        }
      ]
    };
    vi.mocked(api.getTransportProfileDeletionImpact).mockResolvedValue(impact);
    vi.mocked(api.deleteTransportProfile).mockResolvedValue(undefined);
    render(<TransportProfilesView api={api} />);

    await user.click(await screen.findByRole("button", { name: "Delete Work SSH" }));
    const dialog = await screen.findByRole("dialog", { name: "Delete Work SSH" });
    const confirm = within(dialog).getByRole("button", { name: "Confirm delete" });
    expect(confirm).toBeDisabled();

    const firstResolution = within(dialog).getByLabelText("Resolution for private-skill");
    expect(within(firstResolution).queryByRole("option", { name: "Work HTTPS" })).toBeNull();
    await user.selectOptions(firstResolution, `replace:${replacementSshProfile.id}`);
    expect(confirm).toBeDisabled();
    await user.selectOptions(
      within(dialog).getByLabelText("Resolution for docs-site"),
      "unbind:keepExternal"
    );
    expect(confirm).toBeEnabled();
    await user.click(confirm);

    expect(api.deleteTransportProfile).toHaveBeenCalledWith({
      profileId: sshProfile.id,
      resolutions: [
        {
          repositoryId,
          action: "replace",
          replacementProfileId: replacementSshProfile.id
        },
        {
          repositoryId: "2a2968e7-1a33-4f41-b0ed-f507cb636b51",
          action: "unbind",
          driftResolution: "keepExternal"
        }
      ]
    });
    expect(await screen.findByText("Work SSH deleted.")).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Work SSH" })).not.toBeInTheDocument();
  });

  it("ignores a stale profile load after the API changes", async () => {
    const stale = deferred<{ items: TransportProfileSummary[] }>();
    const firstApi = transportProfileApi([]);
    vi.mocked(firstApi.listTransportProfiles).mockImplementation(() => stale.promise);
    const currentApi = transportProfileApi([httpsProfile]);
    const { rerender } = render(<TransportProfilesView api={firstApi} />);

    rerender(<TransportProfilesView api={currentApi} />);
    expect(
      await screen.findByRole("heading", { name: httpsProfile.displayName })
    ).toBeInTheDocument();
    await act(async () => {
      stale.resolve({ items: [sshProfile] });
      await stale.promise;
    });
    expect(screen.queryByRole("heading", { name: sshProfile.displayName })).not.toBeInTheDocument();
  });
});

describe("Git Client transport API", () => {
  it("validates contracts and calls every exact transport RPC route", async () => {
    const networkResult = networkOperationResultSchema.parse({
      operationId,
      repositoryId,
      remoteName: "origin",
      job: transportContracts.cloneResult.job,
      snapshot: transportContracts.cloneResult.snapshot!,
      networkState: transportContracts.networkState
    });
    const impact: TransportProfileDeletionImpact = { profileId: sshProfile.id, repositories: [] };
    const effective = {
      repositoryId,
      source: "profile" as const,
      kind: "ssh" as const,
      profile: sshProfile,
      driftStatus: "clean" as const
    };
    const responses: Record<string, unknown> = {
      "transportProfiles.list": { items: [sshProfile] },
      "transportProfiles.create": httpsProfile,
      "transportProfiles.update": httpsProfile,
      "transportProfiles.getDeletionImpact": impact,
      "transportProfiles.delete": undefined,
      "repositories.getEffectiveTransport": effective,
      "repositories.getNetworkState": transportContracts.networkState,
      "repositories.bindTransport": transportContracts.binding,
      "repositories.unbindTransport": undefined,
      "cloneIntents.get": transportContracts.intent,
      "repositories.clone": transportContracts.cloneResult,
      "repositories.fetch": networkResult,
      "repositories.pull": networkResult,
      "repositories.push": networkResult,
      "repositories.cancelNetworkOperation": undefined
    };
    const client = pluginClient(async (method) => responses[method]);
    const api = createGitClientApi(client);
    const repositoryRequest = { projectId, repositoryId };
    const signal = new AbortController().signal;

    await api.listTransportProfiles();
    await api.createTransportProfile({
      kind: "https",
      displayName: "Work HTTPS",
      username: "creator",
      useHttpPath: true
    });
    await api.updateTransportProfile({
      kind: "https",
      profileId: httpsProfile.id,
      displayName: "Work HTTPS",
      username: "creator",
      useHttpPath: true
    });
    await api.getTransportProfileDeletionImpact({ profileId: sshProfile.id });
    await api.deleteTransportProfile({ profileId: sshProfile.id, resolutions: [] });
    await api.getEffectiveRepositoryTransport(repositoryRequest);
    await api.getRepositoryNetworkState(repositoryRequest);
    await api.bindRepositoryTransport({
      ...repositoryRequest,
      transportProfileId: sshProfile.id,
      replaceExisting: false
    });
    await api.unbindRepositoryTransport({ ...repositoryRequest, driftResolution: "reject" });
    await api.getCloneIntent({ intentId: transportContracts.intent.id });
    await api.cloneRepository(
      {
        source: { kind: "intent", intentId: transportContracts.intent.id },
        transportKind: "ssh",
        profileId: sshProfile.id,
        folderName: "private-skill",
        projectTarget: { kind: "existing", projectId },
        operationId
      },
      signal
    );
    await api.fetchRepository({ ...repositoryRequest, operationId, remoteName: "origin" }, signal);
    await api.pullRepository({ ...repositoryRequest, operationId }, signal);
    await api.pushRepository({ ...repositoryRequest, operationId, target: null }, signal);
    await api.cancelNetworkOperation({ operationId });

    expect(vi.mocked(client.request).mock.calls.map(([method]) => method)).toEqual(
      Object.keys(responses)
    );
  });

  it("rejects invalid transport responses at the plugin boundary", async () => {
    const client = pluginClient(async () => ({
      items: [{ ...sshProfile, sshKeyFileName: "C:/Users/alice/.ssh/id_ed25519" }]
    }));
    const api = createGitClientApi(client);

    await expect(api.listTransportProfiles()).rejects.toThrow();
  });

  it("preserves the trusted picker cancellation response", async () => {
    const client = pluginClient(async () => null);
    const api = createGitClientApi(client);

    await expect(
      api.createTransportProfile({
        kind: "ssh",
        displayName: "Cancelled SSH",
        identitiesOnly: true,
        sshKeyAction: "selectFile"
      })
    ).resolves.toBeNull();
  });

  it("rejects forbidden profile fields before sending an RPC", async () => {
    const client = pluginClient(async () => sshProfile);
    const api = createGitClientApi(client);

    await expect(
      api.createTransportProfile({
        kind: "ssh",
        displayName: "Unsafe SSH",
        identitiesOnly: true,
        sshKeyAction: "selectFile",
        sshKeyPath: "C:/Users/alice/.ssh/id_ed25519"
      } as never)
    ).rejects.toThrow();
    expect(client.request).not.toHaveBeenCalled();
  });

  it("cancels an in-flight request with AbortError and still propagates an original rejection", async () => {
    const pending = deferred<unknown>();
    const original = new Error("native network failed");
    const client = pluginClient(async (method) => {
      if (method === "repositories.fetch") return pending.promise;
      if (method === "repositories.cancelNetworkOperation") return undefined;
      throw new Error(`unexpected method: ${method}`);
    });
    const api = createGitClientApi(client);
    const controller = new AbortController();
    const request = { projectId, repositoryId, operationId, remoteName: "origin" };
    const operation = api.fetchRepository(request, controller.signal);

    controller.abort();
    await expect(operation).rejects.toMatchObject({ name: "AbortError" });
    expect(client.request).toHaveBeenCalledWith("repositories.cancelNetworkOperation", {
      operationId
    });

    pending.reject(original);
    const rejectingClient = pluginClient(async () => {
      throw original;
    });
    await expect(
      createGitClientApi(rejectingClient).fetchRepository(request, new AbortController().signal)
    ).rejects.toBe(original);
  });
});

function transportProfileApi(items: TransportProfileSummary[]): TransportProfilesApi {
  return {
    listTransportProfiles: vi.fn(async () => ({ items })),
    createTransportProfile: vi.fn(),
    updateTransportProfile: vi.fn(),
    getTransportProfileDeletionImpact: vi.fn(),
    deleteTransportProfile: vi.fn()
  };
}

function pluginClient(
  implementation: (method: string, params: unknown) => Promise<unknown>
): PluginClient {
  const request = vi.fn(implementation) as unknown as PluginClient["request"];
  return {
    ready: Promise.resolve({
      type: "host:init",
      sessionId: "7ac93f05-6bdd-4db2-843b-ac65d62228b0",
      pluginId: "git-ramus.git-client",
      sdkVersion: "0.1.0",
      route: "/transport-identities"
    }),
    currentTheme: null,
    theme: null,
    onThemeChanged: vi.fn(() => () => undefined),
    request,
    dispose: vi.fn()
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}
