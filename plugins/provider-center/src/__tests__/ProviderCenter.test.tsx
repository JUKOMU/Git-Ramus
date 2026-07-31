import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { App } from "../App";
import type { ProviderCenterApi } from "../api";
import { InstancePanel } from "../components/InstancePanel";

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
    createCloneIntent: vi.fn(),
    openCloneIntent: vi.fn(),
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

  it("keeps native URL validation before the explicit instance command", async () => {
    const user = userEvent.setup();
    const fake = api();
    render(
      <InstancePanel
        api={fake}
        instances={[]}
        selectedInstanceId={null}
        onSelect={vi.fn()}
        onRefresh={vi.fn(async () => undefined)}
      />
    );

    await user.selectOptions(screen.getByRole("combobox", { name: "Provider type" }), "gitlab");
    const baseUrl = screen.getByRole("textbox", { name: "Base URL" });
    await user.clear(baseUrl);
    await user.type(baseUrl, "not-a-url");
    await user.click(screen.getByRole("button", { name: "Create instance" }));

    expect(fake.createInstance).not.toHaveBeenCalled();
  });

  it("creates and updates instances through explicit command buttons", async () => {
    const user = userEvent.setup();
    const fake = api();
    const selected = instance("a");
    const created = instance("b");
    vi.mocked(fake.createInstance).mockResolvedValue(created);
    vi.mocked(fake.updateInstance).mockResolvedValue(selected);
    const onRefresh = vi.fn(async () => undefined);

    render(
      <InstancePanel
        api={fake}
        instances={[selected]}
        selectedInstanceId={selected.id}
        onSelect={vi.fn()}
        onRefresh={onRefresh}
      />
    );

    const create = screen.getByRole("button", { name: "Create instance" });
    expect(create).toHaveAttribute("type", "button");
    await user.click(create);
    expect(fake.createInstance).toHaveBeenCalledWith({
      providerKind: "github",
      displayName: "GitHub",
      baseUrl: "https://github.com",
      customCaAction: "none"
    });
    expect(onRefresh).toHaveBeenCalledWith(created.id);

    const save = screen.getByRole("button", { name: "Save instance" });
    await waitFor(() => expect(save).toBeEnabled());
    expect(save).toHaveAttribute("type", "button");
    await user.click(save);
    expect(fake.updateInstance).toHaveBeenCalledWith({
      instanceId: selected.id,
      displayName: selected.displayName,
      baseUrl: selected.baseUrl,
      customCaAction: "keep"
    });
    expect(onRefresh).toHaveBeenCalledWith(selected.id, selected.id);
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
