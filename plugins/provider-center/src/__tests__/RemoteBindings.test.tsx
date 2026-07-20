import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { RemoteBindings } from "../components/RemoteBindings";

afterEach(cleanup);

describe("RemoteBindings", () => {
  it("discards scan results from the previously selected Provider context", async () => {
    const user = userEvent.setup();
    let resolveFirstScan!: (value: unknown) => void;
    const api = {
      listBindings: vi.fn(async () => ({ items: [] })),
      matchLocalRemotes: vi.fn(
        () =>
          new Promise((resolve) => {
            resolveFirstScan = resolve;
          })
      )
    };
    const firstInstance = instance("a");
    const firstAccount = account("a");
    const view = render(
      <RemoteBindings
        api={api as never}
        instance={firstInstance}
        account={firstAccount}
        accounts={[firstAccount]}
      />
    );
    await waitFor(() => expect(api.listBindings).toHaveBeenCalledWith(firstAccount.id));
    await user.click(screen.getByRole("button", { name: "Scan local remotes" }));
    const secondInstance = instance("b");
    const secondAccount = account("b");
    view.rerender(
      <RemoteBindings
        api={api as never}
        instance={secondInstance}
        account={secondAccount}
        accounts={[secondAccount]}
      />
    );
    resolveFirstScan({
      items: [
        {
          repositoryId: "7f3c0214-373c-4d43-b0c7-cdaed1cbcc50",
          remoteName: "origin-from-old-context",
          instanceId: firstInstance.id,
          status: "suggested",
          providerRepositoryId: "4242",
          fullName: "skills/private-skill",
          webUrl: "https://gitlab-a.example/skills/private-skill",
          matchedUrl: "git@gitlab-a.example:skills/private-skill.git",
          candidates: []
        }
      ]
    });
    await Promise.resolve();
    expect(screen.queryByText("origin-from-old-context")).not.toBeInTheDocument();
  });
});

function instance(suffix: "a" | "b") {
  return {
    id: instanceId(suffix),
    providerKind: "gitlab" as const,
    displayName: `GitLab ${suffix}`,
    baseUrl: `https://gitlab-${suffix}.example`,
    customCaConfigured: false,
    customCaLabel: null,
    providerEnabled: true,
    status: "connected" as const,
    lastValidatedAt: "2026-07-19T00:00:00Z",
    serverVersion: "18.0",
    createdAt: "2026-07-19T00:00:00Z",
    updatedAt: "2026-07-19T00:00:00Z"
  };
}

function account(suffix: "a" | "b") {
  return {
    id:
      suffix === "a"
        ? "7f3c0214-373c-4d43-b0c7-cdaed1cbcc50"
        : "8f3c0214-373c-4d43-b0c7-cdaed1cbcc51",
    instanceId: instanceId(suffix),
    providerUserId: suffix,
    username: `account-${suffix}`,
    displayName: `Account ${suffix}`,
    avatarUrl: null,
    isDefault: true,
    status: "connected" as const,
    lastValidatedAt: "2026-07-19T00:00:00Z"
  };
}

function instanceId(suffix: "a" | "b"): string {
  return suffix === "a"
    ? "6da75ccf-f7df-4bf2-92b7-2c158765726f"
    : "9da75ccf-f7df-4bf2-92b7-2c1587657270";
}
