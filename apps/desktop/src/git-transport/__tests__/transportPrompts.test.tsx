import { cleanup, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ProviderPromptGate, createPromptBroker } from "../../providers/promptBroker";
import type { ProviderCredentialPromptRequest } from "../../providers/promptPorts";
import { TransportConfirmationDialog } from "../TransportConfirmationDialog";
import { createCloneNavigationBroker } from "../cloneNavigationBroker";
import { createGitTransportPromptBroker, type GitTransportPromptBroker } from "../promptBroker";

let broker: GitTransportPromptBroker | null = null;
const pluginId = "git-ramus.git-client";
const operationId = "b95c216a-dac4-45d1-8169-8dbfbc0c0315";

afterEach(() => {
  cleanup();
  broker?.cancelAll();
  broker = null;
});

describe("trusted Git transport confirmation broker", () => {
  it("serializes confirmations and clears source details before settling", async () => {
    const user = userEvent.setup();
    broker = createGitTransportPromptBroker(new ProviderPromptGate());
    render(<TransportConfirmationDialog broker={broker} />);

    const first = broker.confirm({
      pluginId,
      operationId,
      kind: "network",
      operation: "fetch",
      resourceLabel: "origin"
    });
    const second = broker.confirm({
      pluginId,
      operationId,
      kind: "sourceTrust",
      operation: "clone",
      resourceLabel: "git.example.test/acme/repo"
    });

    expect(
      await screen.findByRole("alertdialog", { name: "Confirm Git network operation" })
    ).toHaveTextContent("origin");
    expect(screen.getByText(pluginId)).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Continue" }));
    await expect(first).resolves.toBe(true);

    expect(await screen.findByText("git.example.test/acme/repo")).toBeInTheDocument();
    expect(screen.queryByText("origin")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await expect(second).resolves.toBe(false);
    expect(screen.queryByText("git.example.test/acme/repo")).not.toBeInTheDocument();
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
  });

  it("ignores stale resolutions and releases the shared gate on cancel", async () => {
    broker = createGitTransportPromptBroker(new ProviderPromptGate());
    const observedIds: string[] = [];
    const unsubscribe = broker.subscribe((active) => {
      if (active !== null) observedIds.push(active.id);
    });
    const first = broker.confirm({
      pluginId,
      operationId,
      kind: "network",
      operation: "pull",
      resourceLabel: "origin/main"
    });
    const second = broker.confirm({
      pluginId,
      operationId: "90e1e991-f93e-4e78-817e-d0ceeb06a749",
      kind: "network",
      operation: "push",
      resourceLabel: "origin/main"
    });
    const firstId = observedIds[0]!;
    broker.resolve(firstId, true);
    await expect(first).resolves.toBe(true);
    const secondId = observedIds.at(-1)!;

    broker.resolve(firstId, true);
    expect(broker.current()?.id).toBe(secondId);
    broker.cancel(secondId);
    await expect(second).resolves.toBe(false);
    expect(broker.current()).toBeNull();
    unsubscribe();
  });

  it("rejects an active confirmation on dialog unmount and does not retain metadata", async () => {
    broker = createGitTransportPromptBroker(new ProviderPromptGate());
    const pending = broker.confirm({
      pluginId,
      operationId: null,
      kind: "replaceConfig",
      operation: "bindProfile",
      resourceLabel: "Repository"
    });
    const rendered = render(<TransportConfirmationDialog broker={broker} />);
    rendered.unmount();

    await expect(pending).rejects.toMatchObject({ code: "git.transport.prompt-unavailable" });
    expect(broker.current()).toBeNull();
  });

  it("shares the trusted Provider prompt gate and rejects unknown secret-bearing fields", async () => {
    const gate = new ProviderPromptGate();
    const provider = createPromptBroker<ProviderCredentialPromptRequest, string>(gate);
    const providerPending = provider.request({
      providerLabel: "GitLab",
      accountLabel: null,
      purpose: "connect"
    });
    broker = createGitTransportPromptBroker(gate);

    await expect(
      broker.confirm({
        pluginId,
        operationId,
        kind: "network",
        operation: "fetch",
        resourceLabel: "origin"
      })
    ).rejects.toMatchObject({ code: "git.transport.prompt-busy" });
    provider.cancelAll();
    await expect(providerPending).resolves.toBeNull();

    const listener = vi.fn();
    broker.subscribe(listener);
    await expect(
      broker.confirm({
        pluginId,
        operationId,
        kind: "network",
        operation: "fetch",
        resourceLabel: "origin",
        pat: "must-not-cross"
      } as never)
    ).rejects.toMatchObject({ code: "git.transport.prompt-invalid" });
    await expect(
      broker.confirm({
        pluginId,
        operationId,
        kind: "sourceTrust",
        operation: "push",
        resourceLabel: "origin"
      })
    ).rejects.toMatchObject({ code: "git.transport.prompt-invalid" });
    expect(JSON.stringify(listener.mock.calls)).not.toContain("must-not-cross");
    expect(broker.current()).toBeNull();
  });

  it("bounds pending confirmations per authenticated plugin", async () => {
    broker = createGitTransportPromptBroker(new ProviderPromptGate());
    const pending = Array.from({ length: 4 }, (_, index) =>
      broker!.confirm({
        pluginId,
        operationId,
        kind: "network",
        operation: "fetch",
        resourceLabel: `origin-${index}`
      })
    );

    await expect(
      broker.confirm({
        pluginId,
        operationId,
        kind: "network",
        operation: "fetch",
        resourceLabel: "overflow"
      })
    ).rejects.toMatchObject({ code: "git.transport.prompt-busy", retryable: true });
    broker.cancelAll();
    await expect(Promise.all(pending)).resolves.toEqual([false, false, false, false]);
  });

  it("cancels active and future confirmations for the same owned operation", async () => {
    const gate = new ProviderPromptGate();
    broker = createGitTransportPromptBroker(gate);
    const active = broker.confirm({
      pluginId,
      operationId,
      kind: "network",
      operation: "fetch",
      resourceLabel: "origin"
    });

    broker.cancelOperation(pluginId, operationId);
    await expect(active).resolves.toBe(false);
    expect(gate.isHeld()).toBe(false);
    expect(broker.isOperationCanceled(pluginId, operationId)).toBe(true);
    await expect(
      broker.confirm({
        pluginId,
        operationId,
        kind: "network",
        operation: "fetch",
        resourceLabel: "origin"
      })
    ).resolves.toBe(false);
    expect(broker.current()).toBeNull();
  });

  it("isolates a throwing listener without stranding the prompt gate", async () => {
    const gate = new ProviderPromptGate();
    broker = createGitTransportPromptBroker(gate);
    broker.subscribe((active) => {
      if (active !== null) throw new Error("broken listener");
    });
    const observed = vi.fn();
    broker.subscribe(observed);
    const pending = broker.confirm({
      pluginId,
      operationId,
      kind: "network",
      operation: "fetch",
      resourceLabel: "origin"
    });
    const id = broker.current()!.id;
    expect(observed).toHaveBeenLastCalledWith(broker.current());
    broker.resolve(id, true);

    await expect(pending).resolves.toBe(true);
    expect(gate.isHeld()).toBe(false);
  });

  it("traps focus and resolves Escape as a user cancellation", async () => {
    const user = userEvent.setup();
    broker = createGitTransportPromptBroker(new ProviderPromptGate());
    const prior = document.createElement("button");
    document.body.append(prior);
    prior.focus();
    const pending = broker.confirm({
      pluginId,
      operationId,
      kind: "network",
      operation: "push",
      resourceLabel: "origin/main"
    });
    render(<TransportConfirmationDialog broker={broker} />);

    await waitFor(() => expect(screen.getByRole("button", { name: "Cancel" })).toHaveFocus());
    await user.keyboard("{Escape}");
    await expect(pending).resolves.toBe(false);
    await waitFor(() => expect(prior).toHaveFocus());
    prior.remove();
  });
});

describe("Clone navigation broker", () => {
  it("publishes one validated route and consumes only the current event", () => {
    const navigation = createCloneNavigationBroker();
    const listener = vi.fn();
    navigation.subscribe(listener);
    const route = "/clone/90e1e991-f93e-4e78-817e-d0ceeb06a749";

    const event = navigation.publish(route);
    expect(listener).toHaveBeenLastCalledWith(event);
    const queued = navigation.publish("/clone/b95c216a-dac4-45d1-8169-8dbfbc0c0315");
    expect(navigation.current()).toEqual(event);
    navigation.consume("stale-event");
    expect(navigation.current()).toEqual(event);
    navigation.consume(event.id);
    expect(navigation.current()).toEqual(queued);
    expect(listener).toHaveBeenLastCalledWith(queued);
    navigation.consume(queued.id);
    expect(navigation.current()).toBeNull();
    expect(listener).toHaveBeenLastCalledWith(null);
  });

  it("rejects non-Clone and non-canonical UUID routes", () => {
    const navigation = createCloneNavigationBroker();
    expect(() => navigation.publish("/repositories/secret")).toThrow();
    expect(() => navigation.publish("/clone/not-a-uuid")).toThrow();
    expect(navigation.current()).toBeNull();
  });
});
