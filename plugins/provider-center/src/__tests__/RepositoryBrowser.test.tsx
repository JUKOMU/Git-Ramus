import { cleanup, render, screen, waitFor } from "@testing-library/react";
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
});

function repository(repositoryId: string, fullName: string) {
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
    updatedAt: "2026-07-19T00:00:00Z"
  };
}
