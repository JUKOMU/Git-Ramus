import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { App } from "../App";
import type { ProviderCenterApi } from "../api";

afterEach(cleanup);

function api(): ProviderCenterApi {
  return {
    listInstances: vi.fn(async () => ({ items: [] })),
    createInstance: vi.fn(),
    updateInstance: vi.fn(),
    validateInstance: vi.fn(),
    deleteInstance: vi.fn(),
    listAccounts: vi.fn(async () => ({ items: [] })),
    connectAccount: vi.fn(),
    rotateAccount: vi.fn(),
    validateAccount: vi.fn(),
    setDefaultAccount: vi.fn(),
    getAccountDeletionImpact: vi.fn(),
    deleteAccount: vi.fn(),
    listAuthorizedAccounts: vi.fn(async () => ({ items: [] })),
    requestReadAccess: vi.fn(),
    revokeReadAccess: vi.fn(),
    listRepositories: vi.fn(async () => ({
      items: [],
      nextCursor: null,
      hasMore: false,
      rateLimit: null
    })),
    cancelOperation: vi.fn(),
    matchLocalRemotes: vi.fn(async () => ({ items: [] })),
    listBindings: vi.fn(async () => ({ items: [] })),
    bindRemote: vi.fn(),
    unbindRemote: vi.fn()
  };
}

describe("Provider Center", () => {
  it("renders the unified instance, account, repository, and binding regions", async () => {
    render(<App api={api()} route="/providers" />);
    expect(await screen.findByRole("heading", { name: "Providers" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Provider instances" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Accounts" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Repository browser" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Remote bindings" })).toBeInTheDocument();
  });

  it("does not render a PAT input in the plugin UI", async () => {
    render(<App api={api()} route="/providers" />);
    await screen.findByRole("heading", { name: "Providers" });
    expect(screen.queryByLabelText(/personal access token/i)).not.toBeInTheDocument();
    expect(screen.queryByLabelText(/secret/i)).not.toBeInTheDocument();
  });

  it("discards an account response from the previously selected instance", async () => {
    const user = userEvent.setup();
    let resolveFirst!: (value: { items: ReturnType<typeof account>[] }) => void;
    const fake = api();
    vi.mocked(fake.listInstances).mockResolvedValue({ items: [instance("a"), instance("b")] });
    vi.mocked(fake.listAccounts).mockImplementation((instanceId: string) => {
      if (instanceId === instanceIdFor("a")) {
        return new Promise((resolve) => {
          resolveFirst = resolve;
        });
      }
      return Promise.resolve({ items: [account("b")] });
    });

    render(<App api={fake} route="/providers" />);
    const secondInstance = (await screen.findByText("GitLab b")).closest("button");
    if (secondInstance === null) throw new Error("Second Provider instance button is missing");
    await user.click(secondInstance);
    expect(await screen.findByText("Account b")).toBeInTheDocument();
    resolveFirst({ items: [account("a")] });
    await Promise.resolve();
    expect(screen.queryByText("Account a")).not.toBeInTheDocument();
  });
});

function instance(suffix: "a" | "b") {
  return {
    id: instanceIdFor(suffix),
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
    instanceId: instanceIdFor(suffix),
    providerUserId: suffix,
    username: `account-${suffix}`,
    displayName: `Account ${suffix}`,
    avatarUrl: null,
    isDefault: true,
    status: "connected" as const,
    lastValidatedAt: "2026-07-19T00:00:00Z"
  };
}

function instanceIdFor(suffix: "a" | "b"): string {
  return suffix === "a"
    ? "6da75ccf-f7df-4bf2-92b7-2c158765726f"
    : "9da75ccf-f7df-4bf2-92b7-2c1587657270";
}
