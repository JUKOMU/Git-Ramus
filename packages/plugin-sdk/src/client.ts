import type {
  HostInit,
  HostToPluginMessage,
  PluginToHostMessage,
  RpcResult,
  ThemeDefinition
} from "@git-ramus/contracts";
import { themeDefinitionSchema } from "@git-ramus/contracts";
import { applyThemeToDocument, clearAppliedTheme } from "./theme";

export interface PluginTransport {
  send(message: PluginToHostMessage): void;
  subscribe(listener: (message: HostToPluginMessage) => void): () => void;
}

interface PendingRequest {
  resolve(value: unknown): void;
  reject(error: unknown): void;
}

export interface PluginClient {
  ready: Promise<HostInit>;
  readonly currentTheme: ThemeDefinition | null;
  readonly theme: ThemeDefinition | null;
  onThemeChanged(listener: (theme: ThemeDefinition) => void): () => void;
  request<T>(method: string, params: unknown): Promise<T>;
  dispose(): void;
}

export interface RandomSource {
  getRandomValues(bytes: Uint8Array<ArrayBuffer>): Uint8Array<ArrayBuffer>;
}

const browserRandomSource: RandomSource = {
  getRandomValues: (bytes) => crypto.getRandomValues(bytes)
};

export function createRequestId(randomSource: RandomSource = browserRandomSource): string {
  const bytes = randomSource.getRandomValues(new Uint8Array(new ArrayBuffer(16)));
  bytes[6] = (bytes[6]! & 0x0f) | 0x40;
  bytes[8] = (bytes[8]! & 0x3f) | 0x80;
  const hex = Array.from(bytes, (value) => value.toString(16).padStart(2, "0")).join("");
  return `${hex.slice(0, 8)}-${hex.slice(8, 12)}-${hex.slice(12, 16)}-${hex.slice(
    16,
    20
  )}-${hex.slice(20)}`;
}

export function createPluginClient(
  transport: PluginTransport,
  createId: () => string = () => createRequestId()
): PluginClient {
  let init: HostInit | null = null;
  let currentTheme: ThemeDefinition | null = null;
  const themeListeners = new Set<(theme: ThemeDefinition) => void>();
  let resolveReady: (message: HostInit) => void = () => undefined;
  let rejectReady: (error: unknown) => void = () => undefined;
  const ready = new Promise<HostInit>((resolve, reject) => {
    resolveReady = resolve;
    rejectReady = reject;
  });
  const pending = new Map<string, PendingRequest>();
  let disposed = false;

  const rejectPending = (error: Error) => {
    for (const request of pending.values()) request.reject(error);
    pending.clear();
  };

  const unsubscribe = transport.subscribe((message) => {
    if (message.type === "host:init") {
      if (disposed) return;
      if (init?.sessionId === message.sessionId) return;
      if (init !== null) {
        rejectPending(new Error("plugin session replaced"));
        currentTheme = null;
        clearAppliedTheme();
      }
      init = { ...message, route: message.route ?? "/" };
      transport.send({ type: "plugin:ready", sessionId: message.sessionId });
      resolveReady(init);
      return;
    }
    if (message.type === "host:theme-changed") {
      if (init === null || message.sessionId !== init.sessionId) return;
      const parsed = themeDefinitionSchema.safeParse(message.theme);
      if (!parsed.success) return;
      currentTheme = parsed.data;
      applyThemeToDocument(currentTheme);
      for (const listener of themeListeners) {
        try {
          void Promise.resolve(listener(currentTheme)).catch(() => undefined);
        } catch {
          // One plugin listener must not prevent the remaining listeners from running.
        }
      }
      return;
    }
    const request = pending.get(message.requestId);
    if (request === undefined || init === null || message.sessionId !== init.sessionId) {
      return;
    }
    pending.delete(message.requestId);
    settle(request, message);
  });

  return {
    ready,
    get currentTheme() {
      return currentTheme;
    },
    get theme() {
      return currentTheme;
    },
    onThemeChanged(listener) {
      themeListeners.add(listener);
      return () => themeListeners.delete(listener);
    },
    async request<T>(method: string, params: unknown): Promise<T> {
      if (disposed) throw new Error("plugin client disposed");
      if (init === null) await ready;
      if (disposed) throw new Error("plugin client disposed");
      const session = init;
      if (session === null) throw new Error("plugin client not initialized");
      const requestId = createId();
      const response = new Promise<unknown>((resolve, reject) => {
        pending.set(requestId, { resolve, reject });
      });
      transport.send({
        type: "rpc:request",
        requestId,
        sessionId: session.sessionId,
        method,
        params
      });
      return (await response) as T;
    },
    dispose() {
      if (disposed) return;
      disposed = true;
      unsubscribe();
      const error = new Error("plugin client disposed");
      rejectReady(error);
      rejectPending(error);
      themeListeners.clear();
    }
  };
}

function settle(request: PendingRequest, result: RpcResult) {
  if (result.ok) {
    request.resolve(result.result);
  } else {
    request.reject(result.error);
  }
}
