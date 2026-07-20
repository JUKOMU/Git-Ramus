import type { ErrorEnvelope } from "@git-ramus/contracts";
import type {
  ProviderAccessPromptRequest,
  ProviderCredentialPromptRequest,
  ProviderPromptPort
} from "./promptPorts";

export interface ActivePrompt<Request> {
  id: string;
  request: Request;
}

export interface PromptBroker<Request, Result> {
  request(request: Request): Promise<Result | null>;
  subscribe(listener: (request: ActivePrompt<Request> | null) => void): () => void;
  resolve(id: string, result: Result): void;
  cancel(id: string): void;
  cancelAll(): void;
}

export class ProviderPromptGate {
  private owner: string | null = null;

  tryAcquire(id: string): boolean {
    if (this.owner !== null) return false;
    this.owner = id;
    return true;
  }

  release(id: string): void {
    if (this.owner === id) this.owner = null;
  }

  isHeld(): boolean {
    return this.owner !== null;
  }
}

interface Pending<Result> {
  resolve(result: Result | null): void;
}

export function createPromptBroker<Request, Result>(
  gate: ProviderPromptGate
): PromptBroker<Request, Result> {
  let active: ActivePrompt<Request> | null = null;
  let pending: Pending<Result> | null = null;
  let sequence = 0;
  const listeners = new Set<(request: ActivePrompt<Request> | null) => void>();

  const notify = () => {
    for (const listener of listeners) listener(active);
  };

  const clear = (result: Result | null) => {
    const current = active;
    const currentPending = pending;
    if (current === null || currentPending === null) return;
    active = null;
    pending = null;
    gate.release(current.id);
    notify();
    currentPending.resolve(result);
  };

  return {
    request(request) {
      const id = nextPromptId(sequence++);
      if (!gate.tryAcquire(id)) return Promise.reject(promptError("provider.prompt-busy"));
      const prompt: ActivePrompt<Request> = { id, request };
      const result = new Promise<Result | null>((resolve) => {
        pending = { resolve };
      });
      active = prompt;
      notify();
      return result;
    },
    subscribe(listener) {
      listeners.add(listener);
      listener(active);
      return () => listeners.delete(listener);
    },
    resolve(id, result) {
      if (active?.id === id) clear(result);
    },
    cancel(id) {
      if (active?.id === id) clear(null);
    },
    cancelAll() {
      if (active !== null) clear(null);
    }
  };
}

function nextPromptId(sequence: number): string {
  const randomUuid = globalThis.crypto?.randomUUID;
  return typeof randomUuid === "function"
    ? randomUuid.call(globalThis.crypto)
    : `provider-prompt-${sequence}`;
}

export function promptError(
  code: "provider.prompt-busy" | "provider.prompt-unavailable" | "provider.prompt-canceled"
): ErrorEnvelope {
  return {
    code,
    category: "userActionRequired",
    message:
      code === "provider.prompt-busy"
        ? "Another Provider prompt is already open"
        : code === "provider.prompt-unavailable"
          ? "Provider prompt is unavailable"
          : "Provider prompt was canceled",
    operationId: null,
    pluginId: null,
    resourceId: null,
    failedStep: "provider.prompt",
    retryable: code === "provider.prompt-busy",
    retryAfterMs: code === "provider.prompt-busy" ? 250 : null,
    recoveryActions: [],
    details: null
  };
}

export const providerPromptGate = new ProviderPromptGate();
export const providerCredentialBroker = createPromptBroker<ProviderCredentialPromptRequest, string>(
  providerPromptGate
);
export const providerAccessBroker = createPromptBroker<ProviderAccessPromptRequest, string[]>(
  providerPromptGate
);

export const providerPromptBrokerPort: ProviderPromptPort = {
  requestCredential: (request) => providerCredentialBroker.request(request),
  requestAccountAccess: (request) => providerAccessBroker.request(request)
};
