import type { ErrorEnvelope } from "@git-ramus/contracts";
import { providerPromptGate } from "../providers/promptBroker";
import type { ProviderPromptGate } from "../providers/promptBroker";
import type { GitTransportPromptPort, GitTransportPromptRequest } from "./promptPorts";

export interface ActiveGitTransportPrompt {
  readonly id: string;
  readonly request: Readonly<GitTransportPromptRequest>;
}

interface PendingPrompt extends ActiveGitTransportPrompt {
  resolve(result: boolean): void;
  reject(error: ErrorEnvelope): void;
}

export interface GitTransportPromptBroker extends GitTransportPromptPort {
  subscribe(listener: (active: ActiveGitTransportPrompt | null) => void): () => void;
  resolve(id: string, result: boolean): void;
  cancel(id: string): void;
  cancelAll(): void;
  unavailable(id: string): void;
  current(): ActiveGitTransportPrompt | null;
}

export function createGitTransportPromptBroker(gate: ProviderPromptGate): GitTransportPromptBroker {
  const maxPendingTotal = 16;
  const maxPendingPerPlugin = 4;
  let active: PendingPrompt | null = null;
  const queued: PendingPrompt[] = [];
  const canceledOperations = new Map<string, number>();
  const listeners = new Set<(active: ActiveGitTransportPrompt | null) => void>();
  let sequence = 0;

  const observable = (): ActiveGitTransportPrompt | null =>
    active === null ? null : { id: active.id, request: active.request };

  const notify = () => {
    const value = observable();
    for (const listener of [...listeners]) {
      try {
        listener(value);
      } catch {
        listeners.delete(listener);
      }
    }
  };

  const activate = (pending: PendingPrompt): boolean => {
    if (!gate.tryAcquire(pending.id)) return false;
    active = pending;
    notify();
    return true;
  };

  const advance = () => {
    while (queued.length > 0) {
      const next = queued.shift()!;
      if (activate(next)) return;
      next.reject(promptError("git.transport.prompt-busy"));
    }
    notify();
  };

  const settle = (id: string, result: boolean) => {
    if (active?.id !== id) return;
    const current = active;
    active = null;
    gate.release(current.id);
    advance();
    current.resolve(result);
  };

  const rejectAll = (error: ErrorEnvelope) => {
    const current = active;
    const waiting = queued.splice(0);
    active = null;
    if (current !== null) gate.release(current.id);
    notify();
    current?.reject(error);
    for (const pending of waiting) pending.reject(error);
  };

  return {
    confirm(request) {
      let parsed: Readonly<GitTransportPromptRequest>;
      try {
        parsed = validateRequest(request);
      } catch {
        return Promise.reject(promptError("git.transport.prompt-invalid"));
      }
      const id = nextPromptId(sequence++);
      if (
        parsed.operationId !== null &&
        isCanceled(canceledOperations, parsed.pluginId, parsed.operationId)
      ) {
        return Promise.resolve(false);
      }
      const pendingTotal = (active === null ? 0 : 1) + queued.length;
      const pendingForPlugin =
        (active?.request.pluginId === parsed.pluginId ? 1 : 0) +
        queued.filter((pending) => pending.request.pluginId === parsed.pluginId).length;
      if (pendingTotal >= maxPendingTotal || pendingForPlugin >= maxPendingPerPlugin) {
        return Promise.reject(promptError("git.transport.prompt-busy"));
      }
      return new Promise<boolean>((resolve, reject) => {
        const pending: PendingPrompt = { id, request: parsed, resolve, reject };
        if (active !== null || queued.length > 0) {
          queued.push(pending);
          return;
        }
        if (!activate(pending)) reject(promptError("git.transport.prompt-busy"));
      });
    },
    subscribe(listener) {
      listeners.add(listener);
      try {
        listener(observable());
      } catch {
        listeners.delete(listener);
      }
      return () => listeners.delete(listener);
    },
    resolve(id, result) {
      settle(id, result);
    },
    cancel(id) {
      settle(id, false);
    },
    cancelAll() {
      const current = active;
      const waiting = queued.splice(0);
      active = null;
      if (current !== null) gate.release(current.id);
      notify();
      current?.resolve(false);
      for (const pending of waiting) pending.resolve(false);
    },
    unavailable(id) {
      if (active?.id === id) {
        rejectAll(promptError("git.transport.prompt-unavailable"));
      }
    },
    cancelOperation(pluginId, operationId) {
      if (!isValidPluginId(pluginId) || !isCanonicalUuid(operationId)) return;
      rememberCancellation(canceledOperations, pluginId, operationId);
      const matches = (pending: PendingPrompt) =>
        pending.request.pluginId === pluginId && pending.request.operationId === operationId;
      const canceledQueued: PendingPrompt[] = [];
      for (let index = queued.length - 1; index >= 0; index -= 1) {
        const pending = queued[index]!;
        if (matches(pending)) {
          queued.splice(index, 1);
          canceledQueued.push(pending);
        }
      }
      const current = active !== null && matches(active) ? active : null;
      if (current !== null) {
        active = null;
        gate.release(current.id);
        advance();
      }
      current?.resolve(false);
      for (const pending of canceledQueued.reverse()) pending.resolve(false);
    },
    isOperationCanceled(pluginId, operationId) {
      return isCanceled(canceledOperations, pluginId, operationId);
    },
    current: observable
  };
}

function validateRequest(value: GitTransportPromptRequest): Readonly<GitTransportPromptRequest> {
  if (typeof value !== "object" || value === null || Array.isArray(value)) throw new Error();
  const record = value as unknown as Record<string, unknown>;
  if (
    Object.keys(record).sort().join("\0") !==
      "kind\0operation\0operationId\0pluginId\0resourceLabel" ||
    !isValidPluginId(record.pluginId) ||
    (record.operationId !== null &&
      (typeof record.operationId !== "string" || !isCanonicalUuid(record.operationId))) ||
    !["network", "sourceTrust", "replaceConfig"].includes(String(record.kind)) ||
    !["clone", "fetch", "pull", "push", "bindProfile"].includes(String(record.operation)) ||
    !isCompatiblePrompt(
      record.kind as GitTransportPromptRequest["kind"],
      record.operation as GitTransportPromptRequest["operation"]
    ) ||
    typeof record.resourceLabel !== "string" ||
    record.resourceLabel.trim().length === 0 ||
    record.resourceLabel.length > 8192 ||
    Array.from(record.resourceLabel).some((character) => {
      const code = character.charCodeAt(0);
      return code < 0x20 || code === 0x7f;
    })
  ) {
    throw new Error();
  }
  return Object.freeze({
    pluginId: record.pluginId,
    operationId: record.operationId as string | null,
    kind: record.kind as GitTransportPromptRequest["kind"],
    operation: record.operation as GitTransportPromptRequest["operation"],
    resourceLabel: record.resourceLabel.trim()
  });
}

function isValidPluginId(value: unknown): value is string {
  return (
    typeof value === "string" &&
    value.length > 0 &&
    value.length <= 256 &&
    !Array.from(value).some((character) => {
      const code = character.charCodeAt(0);
      return code < 0x20 || code === 0x7f;
    })
  );
}

function isCanonicalUuid(value: string): boolean {
  return /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/u.test(value);
}

function cancellationKey(pluginId: string, operationId: string): string {
  return `${pluginId}\0${operationId}`;
}

function purgeCancellations(cancellations: Map<string, number>, now = Date.now()) {
  const cutoff = now - 10 * 60 * 1000;
  for (const [key, createdAt] of cancellations) {
    if (createdAt >= cutoff) break;
    cancellations.delete(key);
  }
}

function rememberCancellation(
  cancellations: Map<string, number>,
  pluginId: string,
  operationId: string
) {
  const now = Date.now();
  purgeCancellations(cancellations, now);
  const key = cancellationKey(pluginId, operationId);
  cancellations.delete(key);
  while (cancellations.size >= 1024) {
    const oldest = cancellations.keys().next().value as string | undefined;
    if (oldest === undefined) break;
    cancellations.delete(oldest);
  }
  cancellations.set(key, now);
}

function isCanceled(
  cancellations: Map<string, number>,
  pluginId: string,
  operationId: string
): boolean {
  purgeCancellations(cancellations);
  return cancellations.has(cancellationKey(pluginId, operationId));
}

function isCompatiblePrompt(
  kind: GitTransportPromptRequest["kind"],
  operation: GitTransportPromptRequest["operation"]
): boolean {
  if (kind === "sourceTrust") return operation === "clone";
  if (kind === "replaceConfig") return operation === "bindProfile";
  return (
    operation === "clone" || operation === "fetch" || operation === "pull" || operation === "push"
  );
}

function nextPromptId(sequence: number): string {
  const randomUuid = globalThis.crypto?.randomUUID;
  return typeof randomUuid === "function"
    ? randomUuid.call(globalThis.crypto)
    : `git-transport-prompt-${sequence}`;
}

function promptError(
  code:
    | "git.transport.prompt-busy"
    | "git.transport.prompt-invalid"
    | "git.transport.prompt-unavailable"
): ErrorEnvelope {
  const busy = code === "git.transport.prompt-busy";
  return {
    code,
    category: code === "git.transport.prompt-invalid" ? "validation" : "userActionRequired",
    message: busy
      ? "Another trusted prompt is already open"
      : code === "git.transport.prompt-invalid"
        ? "Git transport prompt request is invalid"
        : "Git transport prompt is unavailable",
    operationId: null,
    pluginId: null,
    resourceId: null,
    failedStep: "git.transport.prompt",
    retryable: busy,
    retryAfterMs: busy ? 250 : null,
    recoveryActions: [],
    details: null
  };
}

export const transportPromptBroker = createGitTransportPromptBroker(providerPromptGate);
