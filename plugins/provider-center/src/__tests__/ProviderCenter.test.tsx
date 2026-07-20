import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { App } from "../App";

afterEach(cleanup);

function api() {
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
});
