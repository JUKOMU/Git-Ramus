import type { RemoteRepository } from "@git-ramus/contracts";
import { act, cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { RepositoryBrowser } from "../components/RepositoryBrowser";

const account = {
  id: "7f3c0214-373c-4d43-b0c7-cdaed1cbcc50",
  instanceId: "6da75ccf-f7df-4bf2-92b7-2c158765726f",
  providerUserId: "9001",
  username: "creator",
  displayName: "Skill Creator",
  avatarUrl: null,
  isDefault: true,
  status: "connected" as const,
  lastValidatedAt: "2026-07-19T00:00:00Z"
};

afterEach(cleanup);

describe("RepositoryBrowser", () => {
  it("keeps the first page while a late search response is discarded", async () => {
    const user = userEvent.setup();
    let resolveFirst!: (value: unknown) => void;
    const api = {
      listRepositories: vi
        .fn()
        .mockReturnValueOnce(
          new Promise((resolve) => {
            resolveFirst = resolve;
          })
        )
        .mockResolvedValueOnce({
          items: [
            {
              providerKind: "gitlab",
              instanceId: account.instanceId,
              repositoryId: "4242",
              namespace: "skills",
              name: "private-skill",
              fullName: "skills/private-skill",
              webUrl: "https://gitlab.example/skills/private-skill",
              httpsUrl: "https://gitlab.example/skills/private-skill.git",
              sshUrl: "git@gitlab.example:skills/private-skill.git",
              defaultBranch: "main",
              visibility: "private",
              archived: false,
              fork: false,
              permission: "write",
              updatedAt: "2026-07-19T00:00:00Z"
            }
          ],
          nextCursor: null,
          hasMore: false,
          rateLimit: null
        }),
      cancelOperation: vi.fn()
    };
    render(<RepositoryBrowser api={api as never} account={account} />);
    const search = await screen.findByRole("searchbox", { name: "Search repositories" });
    await user.type(search, "skill");
    await waitFor(() => expect(api.listRepositories).toHaveBeenCalledTimes(2));
    resolveFirst({ items: [], nextCursor: null, hasMore: false, rateLimit: null });
    expect(await screen.findByText("skills/private-skill")).toBeInTheDocument();
  });

  it("deduplicates load-more repository IDs", async () => {
    const api = {
      listRepositories: vi
        .fn()
        .mockResolvedValueOnce({
          items: [
            {
              providerKind: "github",
              instanceId: account.instanceId,
              repositoryId: "1",
              namespace: "octo",
              name: "one",
              fullName: "octo/one",
              webUrl: "https://github.com/octo/one",
              httpsUrl: "https://github.com/octo/one.git",
              sshUrl: "git@github.com:octo/one.git",
              defaultBranch: "main",
              visibility: "public",
              archived: false,
              fork: false,
              permission: "read",
              updatedAt: "2026-07-19T00:00:00Z"
            }
          ],
          nextCursor: "cursor",
          hasMore: true,
          rateLimit: null
        })
        .mockResolvedValueOnce({
          items: [
            {
              providerKind: "github",
              instanceId: account.instanceId,
              repositoryId: "1",
              namespace: "octo",
              name: "one",
              fullName: "octo/one",
              webUrl: "https://github.com/octo/one",
              httpsUrl: "https://github.com/octo/one.git",
              sshUrl: "git@github.com:octo/one.git",
              defaultBranch: "main",
              visibility: "public",
              archived: false,
              fork: false,
              permission: "read",
              updatedAt: "2026-07-19T00:00:00Z"
            }
          ],
          nextCursor: null,
          hasMore: false,
          rateLimit: null
        }),
      cancelOperation: vi.fn()
    };
    const user = userEvent.setup();
    render(<RepositoryBrowser api={api as never} account={account} />);
    await screen.findByText("octo/one");
    await user.click(screen.getByRole("button", { name: "Load more" }));
    await waitFor(() => expect(api.listRepositories).toHaveBeenCalledTimes(2));
    expect(screen.getAllByText("octo/one")).toHaveLength(1);
  });

  it("discards and unlocks an in-flight page when the query changes", async () => {
    const user = userEvent.setup();
    let resolveStalePage!: (value: unknown) => void;
    const api = {
      listRepositories: vi
        .fn()
        .mockResolvedValueOnce({
          items: [repository("1", "octo/initial")],
          nextCursor: "cursor",
          hasMore: true,
          rateLimit: null
        })
        .mockReturnValueOnce(
          new Promise((resolve) => {
            resolveStalePage = resolve;
          })
        )
        .mockResolvedValueOnce({
          items: [repository("2", "octo/current")],
          nextCursor: "next-cursor",
          hasMore: true,
          rateLimit: null
        }),
      cancelOperation: vi.fn()
    };
    render(<RepositoryBrowser api={api as never} account={account} />);
    await screen.findByText("octo/initial");
    await user.click(screen.getByRole("button", { name: "Load more" }));
    await waitFor(() => expect(api.listRepositories).toHaveBeenCalledTimes(2));
    await user.type(screen.getByRole("searchbox", { name: "Search repositories" }), "current");
    await waitFor(() => expect(api.listRepositories).toHaveBeenCalledTimes(3));
    expect(await screen.findByText("octo/current")).toBeInTheDocument();
    resolveStalePage({
      items: [repository("3", "octo/stale")],
      nextCursor: null,
      hasMore: false,
      rateLimit: null
    });
    await Promise.resolve();
    expect(screen.queryByText("octo/stale")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Load more" })).toBeEnabled();
  });

  it("creates one Clone intent and locks only the selected repository", async () => {
    const user = userEvent.setup();
    let resolveIntent!: (value: { intentId: string }) => void;
    const createCloneIntent = vi.fn(
      () =>
        new Promise<{ intentId: string }>((resolve) => {
          resolveIntent = resolve;
        })
    );
    const openCloneIntent = vi.fn().mockResolvedValue(undefined);
    const api = {
      listRepositories: vi.fn().mockResolvedValue({
        items: [
          repository("4242", "skills/private-skill", { permission: "write" }),
          repository("4343", "skills/another-skill", { permission: "read" })
        ],
        nextCursor: null,
        hasMore: false,
        rateLimit: null
      }),
      createCloneIntent,
      openCloneIntent,
      cancelOperation: vi.fn()
    };
    render(<RepositoryBrowser api={api as never} account={account} />);

    const selected = await screen.findByRole("button", {
      name: "Clone skills/private-skill"
    });
    const other = screen.getByRole("button", { name: "Clone skills/another-skill" });
    await user.click(selected);
    await user.click(selected);

    expect(createCloneIntent).toHaveBeenCalledTimes(1);
    expect(createCloneIntent).toHaveBeenCalledWith(account.id, "4242");
    expect(selected).toBeDisabled();
    expect(other).toBeEnabled();

    resolveIntent({ intentId: "90e1e991-f93e-4e78-817e-d0ceeb06a749" });
    await waitFor(() =>
      expect(openCloneIntent).toHaveBeenCalledWith("90e1e991-f93e-4e78-817e-d0ceeb06a749")
    );
    await waitFor(() => expect(selected).toBeEnabled());
  });

  it("disables Clone for archived or no-read-access repositories", async () => {
    const noReadAccess = {
      ...repository("4343", "skills/no-access"),
      permission: "none"
    } as unknown as RemoteRepository;
    const api = {
      listRepositories: vi.fn().mockResolvedValue({
        items: [
          repository("4242", "skills/archived", { archived: true, permission: "admin" }),
          noReadAccess
        ],
        nextCursor: null,
        hasMore: false,
        rateLimit: null
      }),
      createCloneIntent: vi.fn(),
      openCloneIntent: vi.fn(),
      cancelOperation: vi.fn()
    };
    render(<RepositoryBrowser api={api as never} account={account} />);

    expect(await screen.findByRole("button", { name: "Clone skills/archived" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Clone skills/no-access" })).toBeDisabled();
  });

  it("discards a stale Clone error after the selected account changes", async () => {
    const user = userEvent.setup();
    const nextAccount = {
      ...account,
      id: "86a8dad7-93ca-45df-90ac-fbba76af90d7",
      providerUserId: "9002",
      username: "reviewer"
    };
    let rejectStaleIntent!: (reason: unknown) => void;
    const createCloneIntent = vi
      .fn()
      .mockReturnValueOnce(
        new Promise((_resolve, reject) => {
          rejectStaleIntent = reject;
        })
      )
      .mockResolvedValueOnce({ intentId: "90e1e991-f93e-4e78-817e-d0ceeb06a749" });
    const api = {
      listRepositories: vi.fn(({ accountId }: { accountId: string }) =>
        Promise.resolve({
          items: [
            accountId === account.id
              ? repository("4242", "skills/old-account")
              : repository("4343", "skills/current-account")
          ],
          nextCursor: null,
          hasMore: false,
          rateLimit: null
        })
      ),
      createCloneIntent,
      openCloneIntent: vi.fn(),
      cancelOperation: vi.fn()
    };
    const { rerender } = render(<RepositoryBrowser api={api as never} account={account} />);
    await user.click(await screen.findByRole("button", { name: "Clone skills/old-account" }));

    rerender(<RepositoryBrowser api={api as never} account={nextAccount} />);
    const current = await screen.findByRole("button", { name: "Clone skills/current-account" });
    await act(async () => {
      rejectStaleIntent(new Error("stale intent failure"));
    });

    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(current).toBeEnabled();
    await user.click(current);
    expect(createCloneIntent).toHaveBeenLastCalledWith(nextAccount.id, "4343");
  });

  it("does not open a stale Clone intent after the selected account changes", async () => {
    const user = userEvent.setup();
    const nextAccount = {
      ...account,
      id: "86a8dad7-93ca-45df-90ac-fbba76af90d7",
      providerUserId: "9002",
      username: "reviewer"
    };
    let resolveStaleIntent!: (value: { intentId: string }) => void;
    const api = {
      listRepositories: vi.fn(({ accountId }: { accountId: string }) =>
        Promise.resolve({
          items: [
            accountId === account.id
              ? repository("4242", "skills/old-account")
              : repository("4343", "skills/current-account")
          ],
          nextCursor: null,
          hasMore: false,
          rateLimit: null
        })
      ),
      createCloneIntent: vi.fn(
        () =>
          new Promise<{ intentId: string }>((resolve) => {
            resolveStaleIntent = resolve;
          })
      ),
      openCloneIntent: vi.fn(),
      cancelOperation: vi.fn()
    };
    const { rerender } = render(<RepositoryBrowser api={api as never} account={account} />);
    await user.click(await screen.findByRole("button", { name: "Clone skills/old-account" }));

    rerender(<RepositoryBrowser api={api as never} account={nextAccount} />);
    await act(async () => {
      resolveStaleIntent({ intentId: "90e1e991-f93e-4e78-817e-d0ceeb06a749" });
    });

    expect(api.openCloneIntent).not.toHaveBeenCalled();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    await screen.findByRole("button", { name: "Clone skills/current-account" });
  });

  it("surfaces Clone intent failures through the shared error notice", async () => {
    const user = userEvent.setup();
    const api = {
      listRepositories: vi.fn().mockResolvedValue({
        items: [repository("4242", "skills/private-skill")],
        nextCursor: null,
        hasMore: false,
        rateLimit: null
      }),
      createCloneIntent: vi.fn().mockRejectedValue(new Error("offline")),
      openCloneIntent: vi.fn(),
      cancelOperation: vi.fn()
    };
    render(<RepositoryBrowser api={api as never} account={account} />);

    await user.click(await screen.findByRole("button", { name: "Clone skills/private-skill" }));

    expect(await screen.findByRole("alert")).toHaveTextContent("Unable to create Clone intent");
  });
});

function repository(
  repositoryId: string,
  fullName: string,
  overrides: Partial<RemoteRepository> = {}
): RemoteRepository {
  const [namespace, name] = fullName.split("/") as [string, string];
  return {
    providerKind: "github" as const,
    instanceId: account.instanceId,
    repositoryId,
    namespace,
    name,
    fullName,
    webUrl: `https://github.com/${fullName}`,
    httpsUrl: `https://github.com/${fullName}.git`,
    sshUrl: `git@github.com:${fullName}.git`,
    defaultBranch: "main",
    visibility: "public" as const,
    archived: false,
    fork: false,
    permission: "read" as const,
    updatedAt: "2026-07-19T00:00:00Z",
    ...overrides
  };
}
