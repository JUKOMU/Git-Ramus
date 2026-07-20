import "@testing-library/jest-dom/vitest";
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  networkOperationResultSchema,
  repositoryNetworkStateSchema,
  repositoryTransportBindingSchema
} from "@git-ramus/contracts";
import type {
  EffectiveTransport,
  RepositoryNetworkState,
  TransportProfileSummary
} from "@git-ramus/contracts";
import transportContracts from "../../../../packages/contracts/src/__fixtures__/transport-contracts.json";
import {
  RepositoryNetworkPanel,
  type RepositoryNetworkApi
} from "../components/RepositoryNetworkPanel";

const projectId = transportContracts.cloneResult.project.id;
const repository = {
  id: transportContracts.networkState.repositoryId,
  displayName: "private-skill"
};
const sshProfile = transportContracts.sshProfile as TransportProfileSummary;
const httpsProfile = transportContracts.httpsProfile as TransportProfileSummary;
const baseNetworkState = repositoryNetworkStateSchema.parse(transportContracts.networkState);
const transportBinding = repositoryTransportBindingSchema.parse(transportContracts.binding);

afterEach(cleanup);

describe("RepositoryNetworkPanel", () => {
  it("disables unsafe Pull and asks for a Push target only when upstream is absent", async () => {
    const user = userEvent.setup();
    const api = networkApi({ upstream: null });
    const onCompleted = vi.fn();
    render(
      <RepositoryNetworkPanel
        api={api}
        repository={repository}
        context={{ projectId }}
        trusted
        onCompleted={onCompleted}
      />
    );

    expect(await screen.findByRole("combobox", { name: "Fetch remote" })).toHaveValue("origin");
    expect(screen.getByRole("button", { name: "Pull" })).toBeDisabled();
    await user.click(screen.getByRole("button", { name: "Push" }));
    await user.selectOptions(screen.getByLabelText("Remote"), "origin");
    await user.clear(screen.getByLabelText("Remote branch"));
    await user.type(screen.getByLabelText("Remote branch"), "main");
    await user.click(screen.getByRole("button", { name: "Set upstream and push" }));

    expect(api.pushRepository).toHaveBeenCalledWith(
      expect.objectContaining({
        projectId,
        repositoryId: repository.id,
        operationId: expect.any(String),
        target: { remoteName: "origin", branchName: "main" }
      }),
      expect.any(AbortSignal)
    );
    expect(onCompleted).toHaveBeenCalledOnce();
  });

  it("fetches the selected Remote and reports known ff-only divergence", async () => {
    const user = userEvent.setup();
    const api = networkApi({
      ahead: 2,
      behind: 3,
      remotes: [
        ...baseNetworkState.remotes,
        {
          name: "upstream",
          fetchUrl: "https://git.example.test/upstream/private-skill.git",
          pushUrl: null,
          kind: "https" as const
        }
      ]
    });
    render(
      <RepositoryNetworkPanel
        api={api}
        repository={repository}
        context={{ projectId }}
        trusted
        onCompleted={vi.fn()}
      />
    );

    expect(await screen.findByText(/fast-forward only/iu)).toBeInTheDocument();
    expect(screen.getAllByText("HTTPS transport").length).toBeGreaterThan(0);
    expect(screen.getByText("Upstream: origin/main")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Pull" })).toBeDisabled();
    await user.selectOptions(screen.getByLabelText("Fetch remote"), "upstream");
    await user.click(screen.getByRole("button", { name: "Fetch" }));
    expect(api.fetchRepository).toHaveBeenCalledWith(
      expect.objectContaining({ remoteName: "upstream", operationId: expect.any(String) }),
      expect.any(AbortSignal)
    );
  });

  it("offers explicit reapply or keep-external actions for drift", async () => {
    const user = userEvent.setup();
    const api = networkApi({}, { driftStatus: "drifted" });
    render(
      <RepositoryNetworkPanel
        api={api}
        repository={repository}
        context={{ projectId }}
        trusted
        onCompleted={vi.fn()}
      />
    );

    expect(await screen.findByText("Transport configuration drift detected.")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Reapply profile" }));
    expect(api.unbindRepositoryTransport).toHaveBeenCalledWith({
      projectId,
      repositoryId: repository.id,
      driftResolution: "reapply"
    });

    vi.mocked(api.getEffectiveRepositoryTransport).mockResolvedValueOnce(
      effectiveTransport({ driftStatus: "drifted" })
    );
    await user.click(await screen.findByRole("button", { name: "Keep external configuration" }));
    expect(api.unbindRepositoryTransport).toHaveBeenLastCalledWith({
      projectId,
      repositoryId: repository.id,
      driftResolution: "keepExternal"
    });
  });

  it("aborts an operation, excludes duplicate clicks, and refreshes after terminal work", async () => {
    const user = userEvent.setup();
    const api = networkApi();
    vi.mocked(api.fetchRepository).mockImplementation(
      (_request, signal) =>
        new Promise((_, reject) => {
          signal.addEventListener(
            "abort",
            () => reject(new DOMException("cancelled", "AbortError")),
            { once: true }
          );
        })
    );
    const onCompleted = vi.fn();
    render(
      <RepositoryNetworkPanel
        api={api}
        repository={repository}
        context={{ projectId }}
        trusted
        onCompleted={onCompleted}
      />
    );

    await screen.findByRole("combobox", { name: "Fetch remote" });
    await user.dblClick(screen.getByRole("button", { name: "Fetch" }));
    expect(api.fetchRepository).toHaveBeenCalledOnce();
    await user.click(screen.getByRole("button", { name: "Cancel network operation" }));
    expect(await screen.findByText("Network operation cancelled.")).toBeInTheDocument();
    expect(vi.mocked(api.fetchRepository).mock.calls[0]![1].aborted).toBe(true);
    expect(onCompleted).toHaveBeenCalledOnce();
    expect(api.getRepositoryNetworkState).toHaveBeenCalledTimes(2);
  });

  it("retries the exact failed network operation when the server offers retry", async () => {
    const user = userEvent.setup();
    const api = networkApi();
    vi.mocked(api.fetchRepository).mockRejectedValueOnce({
      code: "git.transport.remote-unavailable",
      category: "retryable",
      message: "Remote is temporarily unavailable",
      operationId: transportContracts.cloneResult.operationId,
      pluginId: "git-ramus.git-client",
      resourceId: `repository/${repository.id}`,
      failedStep: "transferring",
      retryable: true,
      retryAfterMs: 250,
      recoveryActions: [{ id: "retry-fetch", label: "Retry Fetch", kind: "retry" }],
      details: null
    });
    render(
      <RepositoryNetworkPanel
        api={api}
        repository={repository}
        context={{ projectId }}
        trusted
        onCompleted={vi.fn()}
      />
    );

    await screen.findByRole("combobox", { name: "Fetch remote" });
    await user.click(screen.getByRole("button", { name: "Fetch" }));
    await user.click(await screen.findByRole("button", { name: "Retry Fetch" }));

    expect(api.fetchRepository).toHaveBeenCalledTimes(2);
    expect(await screen.findByText("Fetch completed.")).toBeInTheDocument();
  });

  it("binds a compatible profile and disables all network writes for an untrusted repository", async () => {
    const user = userEvent.setup();
    const api = networkApi(
      {},
      { source: "systemGit", kind: null, profile: null, driftStatus: null }
    );
    const { rerender } = render(
      <RepositoryNetworkPanel
        api={api}
        repository={repository}
        context={{ projectId }}
        trusted
        onCompleted={vi.fn()}
      />
    );

    await screen.findByText("System Git configuration");
    await user.selectOptions(
      screen.getByLabelText("Repository transport profile"),
      httpsProfile.id
    );
    await user.click(screen.getByRole("button", { name: "Apply transport profile" }));
    expect(api.bindRepositoryTransport).toHaveBeenCalledWith({
      projectId,
      repositoryId: repository.id,
      transportProfileId: httpsProfile.id,
      replaceExisting: true
    });

    rerender(
      <RepositoryNetworkPanel
        api={api}
        repository={repository}
        context={{ projectId }}
        trusted={false}
        onCompleted={vi.fn()}
      />
    );
    expect(screen.getByRole("button", { name: "Fetch" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Pull" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Push" })).toBeDisabled();
  });
});

function networkApi(
  stateOverrides: Partial<RepositoryNetworkState> = {},
  transportOverrides: Partial<EffectiveTransport> = {}
): RepositoryNetworkApi {
  const state: RepositoryNetworkState = { ...baseNetworkState, ...stateOverrides };
  const effective = effectiveTransport(transportOverrides);
  const result = networkOperationResultSchema.parse({
    operationId: transportContracts.cloneResult.operationId,
    repositoryId: repository.id,
    remoteName: "origin",
    job: transportContracts.cloneResult.job,
    snapshot: transportContracts.cloneResult.snapshot!,
    networkState: state
  });
  return {
    listTransportProfiles: vi.fn(async () => ({ items: [sshProfile, httpsProfile] })),
    getEffectiveRepositoryTransport: vi.fn(async () => effective),
    getRepositoryNetworkState: vi.fn(async () => state),
    bindRepositoryTransport: vi.fn(async () => transportBinding),
    unbindRepositoryTransport: vi.fn(async () => undefined),
    fetchRepository: vi.fn(async () => result),
    pullRepository: vi.fn(async () => result),
    pushRepository: vi.fn(async () => result)
  };
}

function effectiveTransport(overrides: Partial<EffectiveTransport> = {}): EffectiveTransport {
  return {
    repositoryId: repository.id,
    source: "profile",
    kind: "https",
    profile: httpsProfile,
    driftStatus: "clean",
    ...overrides
  } as EffectiveTransport;
}
