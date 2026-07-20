import "@testing-library/jest-dom/vitest";
import { act, cleanup, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import type {
  DiffFile,
  EffectiveIdentity,
  IdentityProfile,
  ParsedChangeEntry
} from "@git-ramus/contracts";
import transportContracts from "../../../../packages/contracts/src/__fixtures__/transport-contracts.json";
import { IdentityPicker } from "../components/IdentityPicker";
import { RepositoryView, type RepositoryApi } from "../views/RepositoryView";

const projectId = "87a31769-8aaa-47ca-bef3-47e66f0c62fc";
const repositoryId = "a032bc9c-8759-45ac-856f-b76f9addb9d1";
const profileId = "d23957ac-5c0f-4857-9124-7f1599a41f33";
const secondProfileId = "c8f98df3-e949-48e0-a9ad-407fe371a94a";

const repository = {
  id: repositoryId,
  canonicalPath: "C:/work/demo",
  displayName: "Demo repository",
  kind: "normal" as const,
  createdAt: "2026-07-17T00:00:00Z",
  updatedAt: "2026-07-17T00:00:00Z"
};

const persistedSnapshot = {
  id: "5d497627-6613-4273-99e3-2f59c20d121f",
  repositoryId,
  capturedAt: "2026-07-17T00:00:00Z",
  headOid: "abc123",
  branch: "main",
  upstream: "origin/main",
  ahead: 0,
  behind: 0,
  dirty: true,
  stagedCount: 1,
  unstagedCount: 2,
  untrackedCount: 1,
  conflictedCount: 1,
  refreshErrorSummary: null
};

const identity: IdentityProfile = {
  id: profileId,
  displayName: "Work profile",
  userName: "Alice",
  userEmail: "alice@example.com",
  gpgFormat: "ssh",
  signingKey: "ssh-ed25519 AAAA",
  signCommits: true,
  signTags: false,
  createdAt: "2026-07-17T00:00:00Z",
  updatedAt: "2026-07-17T00:00:00Z"
};

const secondIdentity: IdentityProfile = {
  ...identity,
  id: secondProfileId,
  displayName: "Personal profile",
  userEmail: "alice@personal.test",
  signCommits: false,
  gpgFormat: null,
  signingKey: null
};

const effectiveIdentity: EffectiveIdentity = {
  repositoryId,
  profileId,
  profile: identity,
  source: "repositoryProfile",
  displayName: identity.displayName,
  userName: identity.userName,
  userEmail: identity.userEmail,
  gpgFormat: "ssh",
  signingKey: identity.signingKey,
  signCommits: true,
  signTags: false,
  drift: null
};

const staged = change("src/staged.ts", { staged: true, unstaged: false, status: "M." });
const unstaged = change("src/unstaged.ts", { staged: false, unstaged: true, status: ".M" });
const untracked = change("src/new.ts", {
  kind: "untracked",
  staged: false,
  unstaged: true,
  status: "??",
  indexStatus: "?",
  worktreeStatus: "?"
});
const conflicted = change("src/conflict.ts", {
  kind: "conflicted",
  staged: false,
  unstaged: true,
  conflicted: true,
  status: "UU",
  indexStatus: "U",
  worktreeStatus: "U"
});

afterEach(cleanup);

describe("RepositoryView", () => {
  it("composes Network operations and refreshes repository status after terminal work", async () => {
    const user = userEvent.setup();
    const api = createApi({ changes: [] });
    vi.mocked(api.getRepositoryTrustStatus).mockResolvedValue({ trusted: true });
    render(<RepositoryView api={api} context={{ projectId }} repository={repository} />);

    const fetch = await screen.findByRole("button", { name: "Fetch" });
    await waitFor(() => expect(fetch).toBeEnabled());
    const initialRefreshes = vi.mocked(api.getRepositorySnapshot).mock.calls.length;
    await user.click(fetch);

    expect(await screen.findByText("Network operation cancelled.")).toBeInTheDocument();
    await waitFor(() =>
      expect(api.getRepositorySnapshot).toHaveBeenCalledTimes(initialRefreshes + 1)
    );
  });

  it("separates all change groups and renders the selected diff", async () => {
    const user = userEvent.setup();
    const api = createApi({ changes: [staged, unstaged, untracked, conflicted] });
    render(<RepositoryView api={api} context={{ projectId }} repository={repository} />);

    expect(await screen.findByRole("heading", { name: "Staged" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Unstaged" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Untracked" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Conflicts" })).toBeInTheDocument();
    expect(screen.getByText("src/staged.ts")).toBeInTheDocument();
    expect(screen.getByText("src/unstaged.ts")).toBeInTheDocument();
    expect(screen.getByText("src/new.ts")).toBeInTheDocument();
    expect(screen.getByText("src/conflict.ts")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "View diff for src/unstaged.ts" }));
    expect(api.getRepositoryDiff).toHaveBeenCalledWith({
      projectId,
      repositoryId,
      paths: ["src/unstaged.ts"],
      staged: false
    });
    expect(await screen.findByText(diffTextMatcher)).toBeInTheDocument();
  });

  it("marks a bounded patch when the host truncated its content", async () => {
    const user = userEvent.setup();
    const api = createApi({ changes: [unstaged] });
    vi.mocked(api.getRepositoryDiff).mockResolvedValueOnce({
      ...diffResult(unstaged.path, false),
      truncated: true
    });
    render(<RepositoryView api={api} context={{ projectId }} repository={repository} />);

    await user.click(await screen.findByRole("button", { name: "View diff for src/unstaged.ts" }));

    expect(await screen.findByText(diffTextMatcher)).toBeInTheDocument();
    expect(screen.getByText("Diff content was truncated at the safe display limit.")).toBeVisible();
  });

  it("shows a partial-content reason alongside an available patch", async () => {
    const user = userEvent.setup();
    const api = createApi({ changes: [unstaged] });
    vi.mocked(api.getRepositoryDiff).mockResolvedValueOnce({
      ...diffResult(unstaged.path, false),
      contentUnavailableReason: "untrackedContentUnavailable"
    });
    render(<RepositoryView api={api} context={{ projectId }} repository={repository} />);

    await user.click(await screen.findByRole("button", { name: "View diff for src/unstaged.ts" }));

    expect(await screen.findByText(diffTextMatcher)).toBeInTheDocument();
    expect(screen.getByText("Untracked file content is unavailable.")).toBeVisible();
  });

  it.each([
    ["binary", "Binary diff content is not displayed."],
    ["untrustedRepository", "Trust the repository to view diff content."],
    ["nonUtf8Content", "Diff content is not valid UTF-8."],
    ["outputLimit", "Diff content exceeded the safe output limit."],
    ["untrackedContentUnavailable", "Untracked file content is unavailable."]
  ] as const)("explains unavailable %s diff content", async (reason, message) => {
    const user = userEvent.setup();
    const api = createApi({ changes: [unstaged] });
    const unavailable = diffResult(unstaged.path, false);
    vi.mocked(api.getRepositoryDiff).mockResolvedValueOnce({
      ...unavailable,
      patch: null,
      contentUnavailableReason: reason,
      summary: {
        ...unavailable.summary,
        binary: reason === "binary",
        files: unavailable.summary.files.map((file) => ({
          ...file,
          binary: reason === "binary"
        })),
        changes: unavailable.summary.changes.map((file) => ({
          ...file,
          binary: reason === "binary"
        })),
        entries: unavailable.summary.entries.map((file) => ({
          ...file,
          binary: reason === "binary"
        }))
      }
    });
    render(<RepositoryView api={api} context={{ projectId }} repository={repository} />);

    await user.click(await screen.findByRole("button", { name: "View diff for src/unstaged.ts" }));

    expect(await screen.findByText(message)).toBeVisible();
    expect(screen.queryByText(diffTextMatcher)).not.toBeInTheDocument();
  });

  it("loads persisted Trust before enabling writes and does not request Trust again", async () => {
    const user = userEvent.setup();
    const trustStatus = deferred<{ trusted: boolean }>();
    const api = Object.assign(createApi({ changes: [unstaged] }), {
      getRepositoryTrustStatus: vi.fn(() => trustStatus.promise)
    });
    render(<RepositoryView api={api} context={{ projectId }} repository={repository} />);

    expect(await screen.findByRole("heading", { name: "Unstaged" })).toBeInTheDocument();
    expect(screen.getByText("Checking repository Trust…")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Stage all" })).toBeDisabled();

    await act(async () => {
      trustStatus.resolve({ trusted: true });
      await trustStatus.promise;
    });

    expect(await screen.findByText("Trusted on this device")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Trust repository" })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Stage all" })).toBeEnabled();
    await user.click(screen.getByRole("button", { name: "Stage all" }));
    expect(api.trustRepository).not.toHaveBeenCalled();
    expect(api.stageRepository).toHaveBeenCalledOnce();
  });

  it("keeps Trust unknown after a status load failure and retries the host read", async () => {
    const user = userEvent.setup();
    const trustError = {
      ...errorEnvelope(),
      code: "repository.trust-status-unavailable",
      message: "Trust status temporarily unavailable.",
      failedStep: "repositories.getTrustStatus",
      recoveryActions: [{ id: "retry", label: "Retry Trust status", kind: "retry" as const }]
    };
    const api = Object.assign(createApi({ changes: [unstaged] }), {
      getRepositoryTrustStatus: vi
        .fn()
        .mockRejectedValueOnce(trustError)
        .mockResolvedValueOnce({ trusted: true })
    });
    render(<RepositoryView api={api} context={{ projectId }} repository={repository} />);

    expect(await screen.findByText("Trust status temporarily unavailable.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Stage all" })).toBeDisabled();
    expect(screen.queryByRole("button", { name: "Trust repository" })).not.toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "Retry Trust status" }));
    expect(await screen.findByText("Trusted on this device")).toBeInTheDocument();
    expect(api.getRepositoryTrustStatus).toHaveBeenCalledTimes(2);
  });

  it("retries identity state without refreshing repository status", async () => {
    const user = userEvent.setup();
    const api = createApi({ changes: [staged] });
    vi.mocked(api.listIdentities)
      .mockRejectedValueOnce({
        ...errorEnvelope(),
        code: "identity.load-unavailable",
        message: "Identity temporarily unavailable.",
        failedStep: "identities.list",
        recoveryActions: [{ id: "retry", label: "Retry identities", kind: "retry" as const }]
      })
      .mockResolvedValueOnce({
        identities: [identity, secondIdentity],
        globalIdentityProfileId: profileId
      });
    render(<RepositoryView api={api} context={{ projectId }} repository={repository} />);

    await user.click(await screen.findByRole("button", { name: "Retry identities" }));

    await vi.waitFor(() => expect(api.listIdentities).toHaveBeenCalledTimes(2));
    expect(api.getEffectiveRepositoryIdentity).toHaveBeenCalledTimes(2);
    expect(api.getRepositoryChanges).toHaveBeenCalledOnce();
    expect(screen.queryByText("Identity temporarily unavailable.")).not.toBeInTheDocument();
  });

  it("requires explicit Trust and commits the complete staged index without path parameters", async () => {
    const user = userEvent.setup();
    const api = createApi({ changes: [staged] });
    render(<RepositoryView api={api} context={{ projectId }} repository={repository} />);
    const message = await screen.findByLabelText("Commit message");
    await user.type(message, "Ship the staged change");

    const commit = screen.getByRole("button", { name: "Commit staged changes" });
    expect(commit).toBeDisabled();
    await user.click(await screen.findByRole("button", { name: "Trust repository" }));
    expect(api.trustRepository).not.toHaveBeenCalled();
    expect(screen.getByText(/Trust allows write operations/u)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Confirm trust" }));
    expect(api.trustRepository).toHaveBeenCalledWith({ projectId, repositoryId });
    expect(await screen.findByText("Trusted on this device")).toBeInTheDocument();
    expect(commit).toBeEnabled();

    await user.click(commit);
    expect(api.commitRepository).toHaveBeenCalledWith({
      projectId,
      repositoryId,
      message: "Ship the staged change",
      identityProfileId: profileId
    });
    expect(vi.mocked(api.commitRepository).mock.calls[0]?.[0]).not.toHaveProperty("paths");
    expect(api.stageRepository).not.toHaveBeenCalled();
    expect(api.getRepositoryChanges).toHaveBeenCalledTimes(2);
  });

  it("keeps Commit disabled for blank messages and repositories with no staged changes", async () => {
    const user = userEvent.setup();
    const api = createApi({ changes: [unstaged] });
    render(<RepositoryView api={api} context={{ projectId }} repository={repository} />);
    await screen.findByRole("heading", { name: "Unstaged" });

    await user.click(await screen.findByRole("button", { name: "Trust repository" }));
    await user.click(screen.getByRole("button", { name: "Confirm trust" }));
    const commit = screen.getByRole("button", { name: "Commit staged changes" });
    expect(commit).toBeDisabled();
    await user.type(screen.getByLabelText("Commit message"), "There is still nothing staged");
    expect(commit).toBeDisabled();
  });

  it("provides explicit Stage all, refreshes after writes, and exposes retryable RPC recovery", async () => {
    const user = userEvent.setup();
    const retryError = errorEnvelope();
    const api = createApi({ changes: [unstaged] });
    vi.mocked(api.getRepositoryDiff)
      .mockRejectedValueOnce(retryError)
      .mockResolvedValueOnce(diffResult("src/unstaged.ts", false));
    render(<RepositoryView api={api} context={{ projectId }} repository={repository} />);
    await screen.findByRole("heading", { name: "Unstaged" });

    await user.click(screen.getByRole("button", { name: "View diff for src/unstaged.ts" }));
    expect(await screen.findByText("Diff temporarily unavailable.")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Retry diff" }));
    expect(await screen.findByText(diffTextMatcher)).toBeInTheDocument();
    expect(api.getRepositoryDiff).toHaveBeenCalledTimes(2);

    await user.click(await screen.findByRole("button", { name: "Trust repository" }));
    await user.click(screen.getByRole("button", { name: "Confirm trust" }));
    await user.click(screen.getByRole("button", { name: "Stage all" }));
    expect(api.stageRepository).toHaveBeenCalledWith({
      projectId,
      repositoryId,
      paths: [],
      all: true
    });
    expect(api.getRepositoryChanges).toHaveBeenCalledTimes(2);
  });

  it("clears a displayed diff after Stage succeeds", async () => {
    const user = userEvent.setup();
    const api = createApi({ changes: [unstaged] });
    render(<RepositoryView api={api} context={{ projectId }} repository={repository} />);
    await screen.findByRole("heading", { name: "Unstaged" });
    await user.click(screen.getByRole("button", { name: "View diff for src/unstaged.ts" }));
    expect(await screen.findByText(diffTextMatcher)).toBeInTheDocument();
    await user.click(await screen.findByRole("button", { name: "Trust repository" }));
    await user.click(screen.getByRole("button", { name: "Confirm trust" }));

    await user.click(screen.getByRole("button", { name: "Stage all" }));

    expect(
      await screen.findByText("Select a changed path to inspect its diff.")
    ).toBeInTheDocument();
    expect(screen.queryByText(diffTextMatcher)).not.toBeInTheDocument();
  });

  it("clears a displayed diff after Unstage succeeds", async () => {
    const user = userEvent.setup();
    const api = createApi({ changes: [staged] });
    render(<RepositoryView api={api} context={{ projectId }} repository={repository} />);
    const selectedPath = await screen.findByRole("checkbox", { name: "Select src/staged.ts" });
    await user.click(screen.getByRole("button", { name: "View diff for src/staged.ts" }));
    expect(await screen.findByText(diffTextMatcher)).toBeInTheDocument();
    await user.click(selectedPath);
    await user.click(await screen.findByRole("button", { name: "Trust repository" }));
    await user.click(screen.getByRole("button", { name: "Confirm trust" }));

    await user.click(screen.getByRole("button", { name: "Unstage selected" }));

    expect(
      await screen.findByText("Select a changed path to inspect its diff.")
    ).toBeInTheDocument();
    expect(screen.queryByText(diffTextMatcher)).not.toBeInTheDocument();
  });

  it("clears a displayed diff after Commit succeeds", async () => {
    const user = userEvent.setup();
    const api = createApi({ changes: [staged] });
    render(<RepositoryView api={api} context={{ projectId }} repository={repository} />);
    await screen.findByRole("heading", { name: "Staged" });
    await user.click(screen.getByRole("button", { name: "View diff for src/staged.ts" }));
    expect(await screen.findByText(diffTextMatcher)).toBeInTheDocument();
    await user.click(await screen.findByRole("button", { name: "Trust repository" }));
    await user.click(screen.getByRole("button", { name: "Confirm trust" }));
    await user.type(screen.getByLabelText("Commit message"), "Invalidate the old diff");

    await user.click(screen.getByRole("button", { name: "Commit staged changes" }));

    expect(
      await screen.findByText("Select a changed path to inspect its diff.")
    ).toBeInTheDocument();
    expect(screen.queryByText(diffTextMatcher)).not.toBeInTheDocument();
  });

  it("does not restore an in-flight diff that resolves after a successful write", async () => {
    const user = userEvent.setup();
    const pendingDiff = deferred<Awaited<ReturnType<RepositoryApi["getRepositoryDiff"]>>>();
    const api = createApi({ changes: [unstaged] });
    vi.mocked(api.getRepositoryDiff).mockReturnValueOnce(pendingDiff.promise);
    render(<RepositoryView api={api} context={{ projectId }} repository={repository} />);
    await screen.findByRole("heading", { name: "Unstaged" });
    await user.click(await screen.findByRole("button", { name: "Trust repository" }));
    await user.click(screen.getByRole("button", { name: "Confirm trust" }));
    await user.click(screen.getByRole("button", { name: "View diff for src/unstaged.ts" }));

    await user.click(screen.getByRole("button", { name: "Stage all" }));
    await vi.waitFor(() => expect(api.getRepositoryChanges).toHaveBeenCalledTimes(2));
    await act(async () => {
      pendingDiff.resolve(diffResult(unstaged.path, false));
      await pendingDiff.promise;
    });

    expect(screen.getByText("Select a changed path to inspect its diff.")).toBeInTheDocument();
    expect(screen.queryByText(diffTextMatcher)).not.toBeInTheDocument();
  });

  it("ignores older refresh success, failure, and finally state around a write refresh", async () => {
    const user = userEvent.setup();
    const oldFailureRecord =
      deferred<Awaited<ReturnType<RepositoryApi["getRepositorySnapshot"]>>>();
    const oldFailureChanges =
      deferred<Awaited<ReturnType<RepositoryApi["getRepositoryChanges"]>>>();
    const oldSuccessRecord =
      deferred<Awaited<ReturnType<RepositoryApi["getRepositorySnapshot"]>>>();
    const oldSuccessChanges =
      deferred<Awaited<ReturnType<RepositoryApi["getRepositoryChanges"]>>>();
    const freshRecord = deferred<Awaited<ReturnType<RepositoryApi["getRepositorySnapshot"]>>>();
    const freshChanges = deferred<Awaited<ReturnType<RepositoryApi["getRepositoryChanges"]>>>();
    const fresh = change("src/fresh.ts", { staged: false, unstaged: true, status: ".M" });
    const stale = change("src/stale.ts", { staged: false, unstaged: true, status: ".M" });
    const initialRecord = {
      repository,
      snapshot: persistedSnapshot,
      changes: null,
      error: null
    };
    const initialChanges = { repositoryId, snapshot: persistedSnapshot, changes: [unstaged] };
    const api = createApi({ changes: [unstaged] });
    vi.mocked(api.getRepositorySnapshot)
      .mockResolvedValueOnce(initialRecord)
      .mockReturnValueOnce(oldFailureRecord.promise)
      .mockReturnValueOnce(oldSuccessRecord.promise)
      .mockReturnValueOnce(freshRecord.promise);
    vi.mocked(api.getRepositoryChanges)
      .mockResolvedValueOnce(initialChanges)
      .mockReturnValueOnce(oldFailureChanges.promise)
      .mockReturnValueOnce(oldSuccessChanges.promise)
      .mockReturnValueOnce(freshChanges.promise);
    vi.mocked(api.getRepositoryTrustStatus).mockResolvedValue({ trusted: true });
    const { rerender } = render(
      <RepositoryView api={api} context={{ projectId }} repository={repository} />
    );
    const selected = await screen.findByRole("checkbox", { name: "Select src/unstaged.ts" });
    await user.click(selected);
    expect(await screen.findByText("Trusted on this device")).toBeInTheDocument();
    rerender(<RepositoryView api={api} context={{ projectId }} repository={repository} />);
    await vi.waitFor(() => expect(api.getRepositoryChanges).toHaveBeenCalledTimes(2));
    rerender(<RepositoryView api={api} context={{ projectId }} repository={repository} />);
    await vi.waitFor(() => expect(api.getRepositoryChanges).toHaveBeenCalledTimes(3));

    await user.click(screen.getByRole("button", { name: "Stage all" }));
    await vi.waitFor(() => expect(api.getRepositoryChanges).toHaveBeenCalledTimes(4));
    await act(async () => {
      oldFailureRecord.reject({
        ...errorEnvelope(),
        code: "repository.old-refresh-failed",
        message: "Older refresh failed.",
        failedStep: "repositories.getSnapshot"
      });
      oldFailureChanges.resolve(initialChanges);
      await Promise.resolve();
    });

    expect(screen.getByText("Loading repository…")).toBeInTheDocument();
    expect(screen.queryByText("Older refresh failed.")).not.toBeInTheDocument();

    await act(async () => {
      freshRecord.resolve({
        ...initialRecord,
        snapshot: { ...persistedSnapshot, branch: "fresh-branch" }
      });
      freshChanges.resolve({
        repositoryId,
        snapshot: { ...persistedSnapshot, branch: "fresh-branch" },
        changes: [unstaged, fresh]
      });
      await Promise.all([freshRecord.promise, freshChanges.promise]);
    });
    expect(await screen.findByText("fresh-branch")).toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: "Select src/unstaged.ts" })).toBeChecked();

    await act(async () => {
      oldSuccessRecord.resolve({
        ...initialRecord,
        snapshot: { ...persistedSnapshot, branch: "stale-branch" }
      });
      oldSuccessChanges.resolve({
        repositoryId,
        snapshot: { ...persistedSnapshot, branch: "stale-branch" },
        changes: [stale]
      });
      await Promise.all([oldSuccessRecord.promise, oldSuccessChanges.promise]);
    });

    expect(screen.getByText("fresh-branch")).toBeInTheDocument();
    expect(screen.queryByText("stale-branch")).not.toBeInTheDocument();
    expect(screen.getByText("src/fresh.ts")).toBeInTheDocument();
    expect(screen.queryByText("src/stale.ts")).not.toBeInTheDocument();
    expect(screen.getByRole("checkbox", { name: "Select src/unstaged.ts" })).toBeChecked();
    expect(screen.queryByText("Loading repository…")).not.toBeInTheDocument();
  });

  it("does not inspect or commit a refresh failure after unmount", async () => {
    const record = deferred<Awaited<ReturnType<RepositoryApi["getRepositorySnapshot"]>>>();
    const changes = deferred<Awaited<ReturnType<RepositoryApi["getRepositoryChanges"]>>>();
    const api = createApi({ changes: [unstaged] });
    vi.mocked(api.getRepositorySnapshot).mockReturnValueOnce(record.promise);
    vi.mocked(api.getRepositoryChanges).mockReturnValueOnce(changes.promise);
    let inspected = false;
    const lateFailure = new Proxy(
      {},
      {
        get() {
          inspected = true;
          return undefined;
        }
      }
    );
    const { unmount } = render(
      <RepositoryView api={api} context={{ projectId }} repository={repository} />
    );
    await vi.waitFor(() => expect(api.getRepositorySnapshot).toHaveBeenCalledOnce());

    unmount();
    await act(async () => {
      record.reject(lateFailure);
      changes.resolve({ repositoryId, snapshot: persistedSnapshot, changes: [unstaged] });
      await Promise.resolve();
    });

    expect(inspected).toBe(false);
  });

  it("keeps a write ErrorEnvelope visible after refreshing repository status", async () => {
    const user = userEvent.setup();
    const api = createApi({ changes: [staged] });
    vi.mocked(api.commitRepository).mockRejectedValueOnce({
      ...errorEnvelope(),
      code: "repository.commit-unavailable",
      message: "Commit temporarily unavailable.",
      failedStep: "repositories.commit",
      recoveryActions: [{ id: "retry", label: "Refresh status", kind: "retry" as const }]
    });
    render(<RepositoryView api={api} context={{ projectId }} repository={repository} />);
    await screen.findByRole("heading", { name: "Staged" });
    await user.click(await screen.findByRole("button", { name: "Trust repository" }));
    await user.click(screen.getByRole("button", { name: "Confirm trust" }));
    await user.type(screen.getByLabelText("Commit message"), "Keep the failure visible");
    await user.click(screen.getByRole("button", { name: "Commit staged changes" }));

    expect(await screen.findByText("Commit temporarily unavailable.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Refresh status" })).toBeInTheDocument();
    expect(api.getRepositoryChanges).toHaveBeenCalledTimes(2);
  });

  it("unstages selected paths, refreshes, and keeps selections that still exist", async () => {
    const user = userEvent.setup();
    const api = createApi({ changes: [staged] });
    render(<RepositoryView api={api} context={{ projectId }} repository={repository} />);
    const selectedPath = await screen.findByRole("checkbox", { name: "Select src/staged.ts" });
    await user.click(selectedPath);
    await user.click(await screen.findByRole("button", { name: "Trust repository" }));
    await user.click(screen.getByRole("button", { name: "Confirm trust" }));
    await user.click(screen.getByRole("button", { name: "Unstage selected" }));

    expect(api.unstageRepository).toHaveBeenCalledWith({
      projectId,
      repositoryId,
      paths: ["src/staged.ts"]
    });
    expect(api.getRepositoryChanges).toHaveBeenCalledTimes(2);
    expect(selectedPath).toBeChecked();
  });

  it("requires a fresh explicit Trust confirmation when the host invalidates Trust", async () => {
    const user = userEvent.setup();
    const api = createApi({ changes: [unstaged] });
    vi.mocked(api.stageRepository).mockRejectedValueOnce({
      ...errorEnvelope(),
      code: "git.trust-required",
      category: "userActionRequired",
      message: "Repository Trust is required.",
      failedStep: "repositories.stage",
      retryable: false,
      recoveryActions: []
    });
    render(<RepositoryView api={api} context={{ projectId }} repository={repository} />);
    await screen.findByRole("heading", { name: "Unstaged" });
    await user.click(await screen.findByRole("button", { name: "Trust repository" }));
    await user.click(screen.getByRole("button", { name: "Confirm trust" }));
    await user.click(screen.getByRole("button", { name: "Stage all" }));

    expect(await screen.findByText("Repository Trust is required.")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Trust repository" })).toBeInTheDocument();
    expect(screen.queryByText("Trusted on this device")).not.toBeInTheDocument();
  });

  it("ignores an older diff response after a newer path is selected", async () => {
    const user = userEvent.setup();
    const other = change("src/other.ts", { staged: false, unstaged: true, status: ".M" });
    const older = deferred<Awaited<ReturnType<RepositoryApi["getRepositoryDiff"]>>>();
    const newer = deferred<Awaited<ReturnType<RepositoryApi["getRepositoryDiff"]>>>();
    const api = createApi({ changes: [unstaged, other] });
    vi.mocked(api.getRepositoryDiff).mockImplementation((request) =>
      request.paths[0] === unstaged.path ? older.promise : newer.promise
    );
    render(<RepositoryView api={api} context={{ projectId }} repository={repository} />);
    await screen.findByRole("heading", { name: "Unstaged" });

    await user.click(screen.getByRole("button", { name: "View diff for src/unstaged.ts" }));
    await user.click(screen.getByRole("button", { name: "View diff for src/other.ts" }));
    newer.resolve(diffResult(other.path, false, "newer-before", "newer-after"));
    expect(await screen.findByText(diffMatcher("newer-before\nnewer-after"))).toBeInTheDocument();

    await act(async () => {
      older.resolve(diffResult(unstaged.path, false, "older-before", "older-after"));
      await older.promise;
    });
    expect(screen.getByText(diffMatcher("newer-before\nnewer-after"))).toBeInTheDocument();
    expect(screen.queryByText(diffMatcher("older-before\nolder-after"))).not.toBeInTheDocument();
  });

  it("binds a repository identity once and refreshes the effective identity", async () => {
    const user = userEvent.setup();
    const binding = deferred<{
      repositoryId: string;
      identityProfileId: string;
      managed: boolean;
      boundAt: string;
    }>();
    const globalEffective: EffectiveIdentity = {
      ...effectiveIdentity,
      source: "globalProfile"
    };
    const repositoryEffective: EffectiveIdentity = {
      ...effectiveIdentity,
      profileId: secondIdentity.id,
      profile: secondIdentity,
      source: "repositoryProfile",
      displayName: secondIdentity.displayName,
      userName: secondIdentity.userName,
      userEmail: secondIdentity.userEmail,
      gpgFormat: secondIdentity.gpgFormat,
      signingKey: secondIdentity.signingKey,
      signCommits: secondIdentity.signCommits,
      signTags: secondIdentity.signTags
    };
    const api = Object.assign(createApi({ changes: [staged] }), {
      bindRepositoryIdentity: vi.fn(() => binding.promise),
      unbindRepositoryIdentity: vi.fn(async () => undefined)
    });
    vi.mocked(api.getEffectiveRepositoryIdentity)
      .mockResolvedValueOnce(globalEffective)
      .mockResolvedValueOnce(repositoryEffective);
    render(<RepositoryView api={api} context={{ projectId }} repository={repository} />);

    const repositoryIdentity = await screen.findByRole("combobox", {
      name: "Repository identity"
    });
    await vi.waitFor(() =>
      expect(
        within(repositoryIdentity).getByRole("option", { name: /Personal profile/u })
      ).toBeInTheDocument()
    );
    expect(repositoryIdentity).toHaveValue("");
    await user.selectOptions(repositoryIdentity, secondIdentity.id);
    await user.dblClick(screen.getByRole("button", { name: "Bind repository identity" }));

    expect(api.bindRepositoryIdentity).toHaveBeenCalledOnce();
    expect(api.bindRepositoryIdentity).toHaveBeenCalledWith({
      projectId,
      repositoryId,
      identityProfileId: secondIdentity.id
    });
    expect(screen.getByRole("button", { name: "Binding repository identity…" })).toBeDisabled();

    await act(async () => {
      binding.resolve({
        repositoryId,
        identityProfileId: secondIdentity.id,
        managed: true,
        boundAt: "2026-07-19T00:00:00Z"
      });
      await binding.promise;
    });
    await vi.waitFor(() => expect(api.getEffectiveRepositoryIdentity).toHaveBeenCalledTimes(2));
    expect(screen.getByRole("combobox", { name: "Repository identity" })).toHaveValue(
      secondIdentity.id
    );
    expect(screen.getByText("Effective source: Repository profile")).toBeInTheDocument();
  });

  it("unbinds a repository identity once and refreshes the inherited identity", async () => {
    const user = userEvent.setup();
    const unbound = deferred<void>();
    const globalEffective: EffectiveIdentity = {
      ...effectiveIdentity,
      source: "globalProfile"
    };
    const api = Object.assign(createApi({ changes: [staged] }), {
      bindRepositoryIdentity: vi.fn(),
      unbindRepositoryIdentity: vi.fn(() => unbound.promise)
    });
    vi.mocked(api.getEffectiveRepositoryIdentity)
      .mockResolvedValueOnce(effectiveIdentity)
      .mockResolvedValueOnce(globalEffective);
    render(<RepositoryView api={api} context={{ projectId }} repository={repository} />);

    const repositoryIdentity = await screen.findByRole("combobox", {
      name: "Repository identity"
    });
    await vi.waitFor(() => expect(repositoryIdentity).toHaveValue(profileId));
    await user.dblClick(screen.getByRole("button", { name: "Unbind repository identity" }));

    expect(api.unbindRepositoryIdentity).toHaveBeenCalledOnce();
    expect(api.unbindRepositoryIdentity).toHaveBeenCalledWith({ projectId, repositoryId });
    expect(screen.getByRole("button", { name: "Unbinding repository identity…" })).toBeDisabled();

    await act(async () => {
      unbound.resolve();
      await unbound.promise;
    });
    await vi.waitFor(() => expect(api.getEffectiveRepositoryIdentity).toHaveBeenCalledTimes(2));
    expect(screen.getByRole("combobox", { name: "Repository identity" })).toHaveValue("");
    expect(screen.getByText("Effective source: Global profile")).toBeInTheDocument();
  });

  it("does not refresh identity state after a pending bind resolves following unmount", async () => {
    const user = userEvent.setup();
    const binding = deferred<{
      repositoryId: string;
      identityProfileId: string;
      managed: boolean;
      boundAt: string;
    }>();
    const api = createApi({ changes: [staged] });
    vi.mocked(api.getEffectiveRepositoryIdentity).mockResolvedValue({
      ...effectiveIdentity,
      source: "globalProfile"
    });
    vi.mocked(api.bindRepositoryIdentity).mockImplementation(() => binding.promise);
    const { unmount } = render(
      <RepositoryView api={api} context={{ projectId }} repository={repository} />
    );

    const repositoryIdentity = await screen.findByRole("combobox", {
      name: "Repository identity"
    });
    await vi.waitFor(() =>
      expect(
        within(repositoryIdentity).getByRole("option", { name: /Personal profile/u })
      ).toBeInTheDocument()
    );
    await user.selectOptions(repositoryIdentity, secondIdentity.id);
    await user.click(screen.getByRole("button", { name: "Bind repository identity" }));
    unmount();

    await act(async () => {
      binding.resolve({
        repositoryId,
        identityProfileId: secondIdentity.id,
        managed: true,
        boundAt: "2026-07-19T00:00:00Z"
      });
      await binding.promise;
    });
    expect(api.getEffectiveRepositoryIdentity).toHaveBeenCalledOnce();
    expect(api.listIdentities).toHaveBeenCalledOnce();
  });

  it("ignores identity data loaded by an older API", async () => {
    const staleIdentities = deferred<{
      identities: IdentityProfile[];
      globalIdentityProfileId: string | null;
    }>();
    const staleEffective = deferred<EffectiveIdentity>();
    const oldApi = createApi({ changes: [staged] });
    vi.mocked(oldApi.listIdentities).mockImplementation(() => staleIdentities.promise);
    vi.mocked(oldApi.getEffectiveRepositoryIdentity).mockImplementation(
      () => staleEffective.promise
    );
    const currentApi = createApi({ changes: [staged] });
    vi.mocked(currentApi.listIdentities).mockResolvedValue({
      identities: [secondIdentity],
      globalIdentityProfileId: secondIdentity.id
    });
    vi.mocked(currentApi.getEffectiveRepositoryIdentity).mockResolvedValue({
      ...effectiveIdentity,
      profileId: secondIdentity.id,
      profile: secondIdentity,
      source: "globalProfile",
      displayName: secondIdentity.displayName,
      userName: secondIdentity.userName,
      userEmail: secondIdentity.userEmail,
      gpgFormat: secondIdentity.gpgFormat,
      signingKey: secondIdentity.signingKey,
      signCommits: secondIdentity.signCommits,
      signTags: secondIdentity.signTags
    });
    const { rerender } = render(
      <RepositoryView api={oldApi} context={{ projectId }} repository={repository} />
    );

    rerender(<RepositoryView api={currentApi} context={{ projectId }} repository={repository} />);
    expect(await screen.findByText("Effective source: Global profile")).toBeInTheDocument();

    await act(async () => {
      staleIdentities.resolve({
        identities: [identity],
        globalIdentityProfileId: identity.id
      });
      staleEffective.resolve(effectiveIdentity);
      await Promise.all([staleIdentities.promise, staleEffective.promise]);
    });
    expect(screen.getByRole("combobox", { name: "Repository identity" })).toHaveValue("");
    expect(screen.getByText("Effective source: Global profile")).toBeInTheDocument();
    expect(screen.queryByRole("option", { name: /Work profile/u })).not.toBeInTheDocument();
  });

  it("shows unsupported recovery actions without executing them as retry", async () => {
    const user = userEvent.setup();
    const api = createApi({ changes: [unstaged] });
    vi.mocked(api.getRepositoryDiff).mockRejectedValueOnce({
      ...errorEnvelope(),
      recoveryActions: [
        { id: "open-settings", label: "Open identity settings", kind: "openSettings" as const }
      ]
    });
    render(<RepositoryView api={api} context={{ projectId }} repository={repository} />);
    await user.click(await screen.findByRole("button", { name: "View diff for src/unstaged.ts" }));

    const unsupportedAction = await screen.findByRole("button", { name: "Open identity settings" });
    expect(unsupportedAction).toBeDisabled();
    expect(unsupportedAction).toHaveAttribute(
      "title",
      "Complete this action in the Git-Ramus host"
    );
    expect(api.getRepositoryDiff).toHaveBeenCalledTimes(1);
  });
});

describe("IdentityPicker", () => {
  it("shows the effective source, global profile badge, and signing state", () => {
    render(
      <IdentityPicker
        identities={[identity, secondIdentity]}
        globalIdentityProfileId={profileId}
        effectiveIdentity={effectiveIdentity}
        selectedIdentityProfileId={profileId}
        onChange={vi.fn()}
      />
    );

    expect(screen.getByText("Effective source: Repository profile")).toBeInTheDocument();
    expect(screen.getByText("Global")).toBeInTheDocument();
    expect(screen.getByText("Signing enabled · SSH")).toBeInTheDocument();
    expect(screen.getByRole("combobox", { name: "Commit identity" })).toHaveValue(profileId);
  });
});

function createApi({ changes }: { changes: ParsedChangeEntry[] }): RepositoryApi {
  return {
    getRepositorySnapshot: vi.fn(async () => ({
      repository,
      snapshot: persistedSnapshot,
      changes: null,
      error: null
    })),
    getRepositoryChanges: vi.fn(async () => ({
      repositoryId,
      snapshot: persistedSnapshot,
      changes
    })),
    getRepositoryDiff: vi.fn(async (request) =>
      diffResult(request.paths[0] ?? "all", request.staged)
    ),
    getRepositoryTrustStatus: vi.fn(async () => ({ trusted: false })),
    stageRepository: vi.fn(async () => ({
      repositoryId,
      snapshot: persistedSnapshot,
      output: null
    })),
    unstageRepository: vi.fn(async () => ({
      repositoryId,
      snapshot: persistedSnapshot,
      output: null
    })),
    commitRepository: vi.fn(async () => ({
      repositoryId,
      snapshot: persistedSnapshot,
      output: "def456"
    })),
    trustRepository: vi.fn(async () => ({
      trust: {
        repositoryId,
        trustedAt: "2026-07-17T00:00:00Z",
        trustVersion: 1
      }
    })),
    listIdentities: vi.fn(async () => ({
      identities: [identity, secondIdentity],
      globalIdentityProfileId: profileId
    })),
    bindRepositoryIdentity: vi.fn(async () => ({
      repositoryId,
      identityProfileId: profileId,
      managed: true,
      boundAt: "2026-07-17T00:00:00Z"
    })),
    unbindRepositoryIdentity: vi.fn(async () => undefined),
    getEffectiveRepositoryIdentity: vi.fn(async () => effectiveIdentity),
    listTransportProfiles: vi.fn(async () => ({
      items: [transportContracts.httpsProfile] as never
    })),
    getEffectiveRepositoryTransport: vi.fn(async () => ({
      repositoryId,
      source: "systemGit" as const,
      kind: null,
      profile: null,
      driftStatus: null
    })),
    getRepositoryNetworkState: vi.fn(
      async () =>
        ({
          ...transportContracts.networkState,
          repositoryId
        }) as never
    ),
    bindRepositoryTransport: vi.fn(async () => null),
    unbindRepositoryTransport: vi.fn(async () => undefined),
    fetchRepository: vi.fn(async () => null),
    pullRepository: vi.fn(async () => null),
    pushRepository: vi.fn(async () => null)
  };
}

function change(path: string, overrides: Partial<ParsedChangeEntry>): ParsedChangeEntry {
  return {
    path,
    originalPath: null,
    kind: "modified",
    staged: false,
    unstaged: true,
    conflicted: false,
    binary: false,
    old: null,
    new: null,
    oldPath: null,
    newPath: null,
    status: ".M",
    indexStatus: ".",
    worktreeStatus: "M",
    additions: 1,
    deletions: 1,
    ...overrides
  };
}

function diffResult(path: string, stagedDiff: boolean, old = "before", next = "after") {
  const file: DiffFile = {
    path,
    oldPath: null,
    newPath: path,
    binary: false,
    additions: 1,
    deletions: 1,
    old: `a/${path}`,
    new: `b/${path}`
  };
  return {
    repositoryId,
    staged: stagedDiff,
    patch: `${old}\n${next}`,
    truncated: false,
    contentUnavailableReason: null,
    summary: {
      files: [file],
      changes: [file],
      entries: [file],
      binary: false,
      additions: 1,
      deletions: 1
    }
  };
}

function errorEnvelope() {
  return {
    code: "repository.diff-unavailable",
    category: "retryable" as const,
    message: "Diff temporarily unavailable.",
    operationId: null,
    pluginId: "git-ramus.git-client",
    resourceId: repositoryId,
    failedStep: "repositories.getDiff",
    retryable: true,
    retryAfterMs: null,
    recoveryActions: [{ id: "retry", label: "Retry diff", kind: "retry" as const }],
    details: null
  };
}

function diffTextMatcher(_content: string, element: Element | null) {
  return element?.tagName === "PRE" && element.textContent === "before\nafter";
}

function diffMatcher(expected: string) {
  return (_content: string, element: Element | null) =>
    element?.tagName === "PRE" && element.textContent === expected;
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
