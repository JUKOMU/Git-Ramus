import "@testing-library/jest-dom/vitest";
import { act, cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { cloneIntentSummarySchema } from "@git-ramus/contracts";
import type { CloneResult, Project, TransportProfileSummary } from "@git-ramus/contracts";
import transportContracts from "../../../../packages/contracts/src/__fixtures__/transport-contracts.json";
import type { GitClientApi } from "../api";
import { App } from "../App";
import { CloneView, type CloneApi } from "../views/CloneView";

const intentId = transportContracts.intent.id;
const project: Project = {
  id: transportContracts.cloneResult.project.id,
  rootPath: "C:/workspace/skills",
  name: transportContracts.cloneResult.project.name,
  scanDepth: transportContracts.cloneResult.project.scanDepth,
  excludePatterns: transportContracts.cloneResult.project.excludePatterns,
  createdAt: transportContracts.cloneResult.project.createdAt,
  updatedAt: transportContracts.cloneResult.project.updatedAt
};
const sshProfile = transportContracts.sshProfile as TransportProfileSummary;
const httpsProfile = transportContracts.httpsProfile as TransportProfileSummary;
const cloneResult = transportContracts.cloneResult as CloneResult;
const cloneIntent = cloneIntentSummarySchema.parse(transportContracts.intent);

afterEach(cleanup);

describe("CloneView", () => {
  it("routes exact Clone intent paths and rejects malformed intent routes", async () => {
    const api = cloneApi();
    const { rerender } = render(<App api={api as GitClientApi} route={`/clone/${intentId}`} />);
    expect(await screen.findByText("skills/private-skill")).toBeInTheDocument();
    expect(api.getCloneIntent).toHaveBeenCalledWith({ intentId });

    vi.mocked(api.getCloneIntent).mockClear();
    rerender(<App api={api as GitClientApi} route="/clone/not-a-uuid" />);
    expect(await screen.findByRole("heading", { name: "Route unavailable" })).toBeInTheDocument();
    expect(api.getCloneIntent).not.toHaveBeenCalled();
  });

  it("consumes a Provider intent and submits a path-free Clone request", async () => {
    const user = userEvent.setup();
    const api = cloneApi();
    const onCloned = vi.fn();
    render(<CloneView api={api} intentId={intentId} onCloned={onCloned} />);

    expect(await screen.findByText("skills/private-skill")).toBeInTheDocument();
    await user.click(screen.getByLabelText("SSH"));
    await user.selectOptions(screen.getByLabelText("Transport profile"), sshProfile.id);
    await user.selectOptions(screen.getByLabelText("Project"), project.id);
    await user.click(screen.getByRole("button", { name: "Clone repository" }));

    expect(api.cloneRepository).toHaveBeenCalledWith(
      expect.objectContaining({
        source: { kind: "intent", intentId },
        transportKind: "ssh",
        profileId: sshProfile.id,
        folderName: "private-skill",
        projectTarget: { kind: "existing", projectId: project.id },
        operationId: expect.any(String)
      }),
      expect.any(AbortSignal)
    );
    const payload = JSON.stringify(vi.mocked(api.cloneRepository).mock.calls[0]);
    expect(payload).not.toMatch(/destination|sshKeyPath|[A-Za-z]:[\\/]|\/home\//u);
    expect(onCloned).toHaveBeenCalledWith(cloneResult);
  });

  it("validates manual folder names and supports a new Project target", async () => {
    const user = userEvent.setup();
    const api = cloneApi();
    render(<CloneView api={api} intentId={null} onCloned={vi.fn()} />);

    await screen.findByText("Manual Git remote");
    await user.type(
      screen.getByLabelText("Git remote URL"),
      "https://git.example.test/acme/repo.git"
    );
    await user.type(screen.getByLabelText("Folder name"), "../unsafe");
    expect(screen.getByText("Choose a safe single folder name.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Clone repository" })).toBeDisabled();

    await user.clear(screen.getByLabelText("Folder name"));
    await user.type(screen.getByLabelText("Folder name"), "repo");
    await user.click(screen.getByRole("radio", { name: "New project" }));
    await user.type(screen.getByLabelText("New project name"), "Acme repositories");
    await user.click(screen.getByRole("button", { name: "Clone repository" }));

    expect(api.cloneRepository).toHaveBeenCalledWith(
      expect.objectContaining({
        source: {
          kind: "manual",
          remoteUrl: "https://git.example.test/acme/repo.git"
        },
        projectTarget: { kind: "new", name: "Acme repositories" }
      }),
      expect.any(AbortSignal)
    );
  });

  it("treats trusted prompt cancellation as a recoverable user choice", async () => {
    const user = userEvent.setup();
    const api = cloneApi();
    vi.mocked(api.cloneRepository).mockResolvedValue(null);
    render(<CloneView api={api} intentId={intentId} onCloned={vi.fn()} />);

    await screen.findByText("skills/private-skill");
    await user.selectOptions(screen.getByLabelText("Project"), project.id);
    await user.click(screen.getByRole("button", { name: "Clone repository" }));

    expect(await screen.findByText("Clone cancelled.")).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Clone repository" })).toBeEnabled();
  });

  it("aborts one in-flight operation and excludes duplicate submits", async () => {
    const user = userEvent.setup();
    const api = cloneApi();
    vi.mocked(api.cloneRepository).mockImplementation(
      (_request, signal) =>
        new Promise((_, reject) => {
          signal.addEventListener(
            "abort",
            () => reject(new DOMException("cancelled", "AbortError")),
            { once: true }
          );
        })
    );
    render(<CloneView api={api} intentId={intentId} onCloned={vi.fn()} />);

    await screen.findByText("skills/private-skill");
    await user.selectOptions(screen.getByLabelText("Project"), project.id);
    await user.dblClick(screen.getByRole("button", { name: "Clone repository" }));
    expect(api.cloneRepository).toHaveBeenCalledOnce();
    expect(screen.getByRole("button", { name: "Cancel clone" })).toBeEnabled();
    await user.click(screen.getByRole("button", { name: "Cancel clone" }));

    expect(await screen.findByText("Clone cancelled.")).toBeInTheDocument();
    expect(vi.mocked(api.cloneRepository).mock.calls[0]![1].aborted).toBe(true);
  });

  it("shows only server-provided recovery actions for a partial result", async () => {
    const user = userEvent.setup();
    const api = cloneApi();
    vi.mocked(api.cloneRepository).mockResolvedValue({
      ...cloneResult,
      status: "partial",
      job: {
        ...cloneResult.job,
        status: "failed",
        error: {
          code: "git.transport.partial",
          category: "partialResult",
          message: "Repository cloned but registration needs repair",
          operationId: cloneResult.operationId,
          pluginId: "git-ramus.git-client",
          resourceId: `repository/${cloneResult.repository.id}`,
          failedStep: "registering",
          retryable: false,
          retryAfterMs: null,
          recoveryActions: [
            { id: "open-project", label: "Open project recovery", kind: "openSettings" }
          ],
          details: null
        }
      }
    });
    render(<CloneView api={api} intentId={intentId} onCloned={vi.fn()} />);

    await screen.findByText("skills/private-skill");
    await user.selectOptions(screen.getByLabelText("Project"), project.id);
    await user.click(screen.getByRole("button", { name: "Clone repository" }));

    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Repository cloned but registration needs repair"
    );
    expect(screen.getByRole("button", { name: "Open project recovery" })).toBeDisabled();
    expect(screen.queryByRole("button", { name: "Try again" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Clone repository" })).toBeDisabled();
    expect(screen.getByLabelText("Folder name")).toBeDisabled();

    await user.click(screen.getByRole("button", { name: "Clone repository" }));
    expect(api.cloneRepository).toHaveBeenCalledOnce();
  });

  it("ignores a stale intent after the API changes", async () => {
    const stale = deferred<Awaited<ReturnType<CloneApi["getCloneIntent"]>>>();
    const oldApi = cloneApi();
    vi.mocked(oldApi.getCloneIntent).mockImplementation(() => stale.promise);
    const currentApi = cloneApi();
    const { rerender } = render(<CloneView api={oldApi} intentId={intentId} onCloned={vi.fn()} />);

    rerender(<CloneView api={currentApi} intentId={intentId} onCloned={vi.fn()} />);
    expect(await screen.findByText("skills/private-skill")).toBeInTheDocument();
    await act(async () => {
      stale.resolve({
        ...cloneIntent,
        repository: { ...cloneIntent.repository, fullName: "stale/repository" }
      });
      await stale.promise;
    });
    expect(screen.queryByText("stale/repository")).not.toBeInTheDocument();
  });
});

function cloneApi(): CloneApi {
  return {
    listProjects: vi.fn(async () => ({ projects: [project] })),
    listTransportProfiles: vi.fn(async () => ({ items: [sshProfile, httpsProfile] })),
    getCloneIntent: vi.fn(async () => cloneIntent),
    cloneRepository: vi.fn(async () => cloneResult)
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
