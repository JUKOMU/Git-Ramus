import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { AccountPanel } from "../components/AccountPanel";

afterEach(cleanup);

const instance = {
  id: "6da75ccf-f7df-4bf2-92b7-2c158765726f",
  providerKind: "gitlab" as const,
  displayName: "GitLab",
  baseUrl: "https://gitlab.example",
  customCaConfigured: false,
  customCaLabel: null,
  providerEnabled: true,
  status: "connected" as const,
  lastValidatedAt: "2026-07-19T00:00:00Z",
  serverVersion: "18.0",
  createdAt: "2026-07-19T00:00:00Z",
  updatedAt: "2026-07-19T00:00:00Z"
};

const first = account("7f3c0214-373c-4d43-b0c7-cdaed1cbcc50", "First account", true);
const second = account("8f3c0214-373c-4d43-b0c7-cdaed1cbcc51", "Second account", false);

describe("AccountPanel", () => {
  it("excludes the actual deletion target from reassignment and default choices", async () => {
    const user = userEvent.setup();
    const api = {
      getAccountDeletionImpact: vi.fn(async () => ({
        accountId: second.id,
        instanceId: instance.id,
        isDefault: false,
        explicitBindingCount: 1,
        inheritedBindingCount: 0,
        siblingAccountIds: [first.id],
        requiresNewDefault: true
      }))
    };
    render(
      <AccountPanel
        api={api as never}
        instance={instance}
        accounts={[first, second]}
        selectedAccountId={first.id}
        onSelect={vi.fn()}
        onRefresh={vi.fn(async () => undefined)}
      />
    );

    expect(screen.getByRole("button", { name: "Connect account" })).toHaveAttribute(
      "type",
      "button"
    );

    await user.click(screen.getAllByRole("button", { name: "Delete…" })[1]!);
    const dialog = await screen.findByRole("dialog", { name: "Delete account" });
    await user.click(within(dialog).getByRole("radio", { name: "Reassign bindings" }));
    const reassign = within(dialog).getByRole("combobox", { name: "Reassign to" });
    expect(within(reassign).getByRole("option", { name: "First account" })).toBeInTheDocument();
    expect(within(reassign).queryByRole("option", { name: "Second account" })).toBeNull();
    const newDefault = within(dialog).getByRole("combobox", { name: "New default account" });
    expect(within(newDefault).getByRole("option", { name: "First account" })).toBeInTheDocument();
    expect(within(newDefault).queryByRole("option", { name: "Second account" })).toBeNull();
  });
});

function account(id: string, displayName: string, isDefault: boolean) {
  return {
    id,
    instanceId: instance.id,
    providerUserId: id,
    username: displayName.toLowerCase().replaceAll(" ", "-"),
    displayName,
    avatarUrl: null,
    isDefault,
    status: "connected" as const,
    lastValidatedAt: "2026-07-19T00:00:00Z"
  };
}
