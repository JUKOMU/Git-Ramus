import providerContracts from "../../../../../packages/contracts/src/__fixtures__/provider-contracts.json";
import type { ProviderAuthorizedAccount } from "@git-ramus/contracts";
import { cleanup, fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";
import type { ProviderAccessPromptRequest, ProviderCredentialPromptRequest } from "../promptPorts";
import {
  ProviderPromptGate,
  createPromptBroker,
  providerAccessBroker,
  providerCredentialBroker
} from "../promptBroker";
import { ProviderAccessDialog } from "../ProviderAccessDialog";
import { ProviderCredentialDialog } from "../ProviderCredentialDialog";

afterEach(() => {
  cleanup();
  providerCredentialBroker.cancelAll();
  providerAccessBroker.cancelAll();
});

describe("trusted Provider prompt brokers and dialogs", () => {
  it("resolves a credential request and clears the password from the DOM", async () => {
    const user = userEvent.setup();
    const gate = new ProviderPromptGate();
    const broker = createPromptBroker<ProviderCredentialPromptRequest, string>(gate);
    const pending = broker.request({
      providerLabel: "GitLab",
      accountLabel: null,
      purpose: "connect"
    });
    render(<ProviderCredentialDialog broker={broker} />);

    const input = screen.getByLabelText("Personal access token");
    expect(input).toHaveAttribute("type", "password");
    expect(input).toHaveAttribute("autocomplete", "off");
    expect(input).toHaveAttribute("spellcheck", "false");
    await user.type(input, "glpat-transient");
    await user.click(screen.getByRole("button", { name: "Connect" }));

    await expect(pending).resolves.toBe("glpat-transient");
    expect(screen.queryByDisplayValue("glpat-transient")).not.toBeInTheDocument();
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("rejects a concurrent credential or access request with one stable busy code", async () => {
    const gate = new ProviderPromptGate();
    const credentials = createPromptBroker<ProviderCredentialPromptRequest, string>(gate);
    const access = createPromptBroker<ProviderAccessPromptRequest, string[]>(gate);
    const first = credentials.request({
      providerLabel: "GitLab",
      accountLabel: null,
      purpose: "connect"
    });

    await expect(
      access.request({ pluginId: "example.reader", accounts: [] })
    ).rejects.toMatchObject({ code: "provider.prompt-busy" });
    credentials.cancelAll();
    await expect(first).resolves.toBeNull();

    const second = access.request({ pluginId: "example.reader", accounts: [] });
    await expect(
      credentials.request({
        providerLabel: "GitLab",
        accountLabel: null,
        purpose: "rotate"
      })
    ).rejects.toMatchObject({ code: "provider.prompt-busy" });
    access.cancelAll();
    await expect(second).resolves.toBeNull();
  });

  it("returns only checked account IDs and never uses usernames as DOM metadata", async () => {
    const user = userEvent.setup();
    const gate = new ProviderPromptGate();
    const broker = createPromptBroker<ProviderAccessPromptRequest, string[]>(gate);
    const accounts = [providerContracts.authorizedAccount] as ProviderAuthorizedAccount[];
    const pending = broker.request({ pluginId: "example.reader", accounts });
    render(<ProviderAccessDialog broker={broker} />);

    expect(screen.getByRole("dialog")).toHaveTextContent("example.reader");
    expect(screen.getByLabelText("Skill Creator")).toBeInTheDocument();
    expect(screen.getByRole("checkbox")).not.toBeChecked();
    expect(screen.getByRole("button", { name: "Approve" })).toBeDisabled();
    expect(screen.queryByTestId("creator")).not.toBeInTheDocument();

    await user.click(screen.getByRole("checkbox"));
    expect(screen.getByRole("button", { name: "Approve" })).toBeEnabled();
    await user.click(screen.getByRole("button", { name: "Approve" }));

    await expect(pending).resolves.toEqual([providerContracts.authorizedAccount.account.id]);
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
  });

  it("cancels on Escape and on unmount, releasing the shared prompt gate", async () => {
    const user = userEvent.setup();
    const gate = new ProviderPromptGate();
    const broker = createPromptBroker<ProviderCredentialPromptRequest, string>(gate);
    const pending = broker.request({
      providerLabel: "GitLab",
      accountLabel: null,
      purpose: "rotate"
    });
    const rendered = render(<ProviderCredentialDialog broker={broker} />);
    await user.keyboard("{Escape}");
    await expect(pending).resolves.toBeNull();

    const second = broker.request({
      providerLabel: "GitLab",
      accountLabel: null,
      purpose: "connect"
    });
    rendered.unmount();
    await expect(second).resolves.toBeNull();
    expect(gate.isHeld()).toBe(false);
  });

  it("traps focus inside a visible prompt and restores the prior shell focus", async () => {
    const gate = new ProviderPromptGate();
    const broker = createPromptBroker<ProviderCredentialPromptRequest, string>(gate);
    const prior = document.createElement("button");
    prior.textContent = "Shell action";
    document.body.append(prior);
    prior.focus();
    const pending = broker.request({
      providerLabel: "GitLab",
      accountLabel: null,
      purpose: "connect"
    });
    render(<ProviderCredentialDialog broker={broker} />);
    await waitFor(() => expect(screen.getByLabelText("Personal access token")).toHaveFocus());

    fireEvent.keyDown(screen.getByLabelText("Personal access token"), {
      key: "Tab",
      shiftKey: true
    });
    expect(screen.getByRole("button", { name: "Cancel" })).toHaveFocus();
    broker.cancelAll();
    await expect(pending).resolves.toBeNull();
    await waitFor(() => expect(prior).toHaveFocus());
    prior.remove();
  });
});
