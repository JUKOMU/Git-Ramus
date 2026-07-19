import {
  hostToPluginMessageSchema,
  type HostToPluginMessage,
  type PluginToHostMessage
} from "@git-ramus/contracts";
import type { PluginTransport } from "./client";

export interface BrowserTransportOptions {
  targetOrigin?: string;
  allowedOrigin?: string;
}

export function createBrowserTransport(
  currentWindow: Window = window,
  options: BrowserTransportOptions = {}
): PluginTransport {
  const targetOrigin = options.targetOrigin ?? "*";
  const allowedOrigin = options.allowedOrigin ?? (targetOrigin === "*" ? undefined : targetOrigin);
  return {
    send(message: PluginToHostMessage) {
      currentWindow.parent.postMessage(message, targetOrigin);
    },
    subscribe(listener: (message: HostToPluginMessage) => void) {
      const receive = (event: MessageEvent<unknown>) => {
        if (event.source !== currentWindow.parent) {
          return;
        }
        if (allowedOrigin !== undefined && event.origin !== allowedOrigin) {
          return;
        }
        const result = hostToPluginMessageSchema.safeParse(event.data);
        if (result.success) {
          listener(result.data);
        }
      };
      currentWindow.addEventListener("message", receive);
      return () => currentWindow.removeEventListener("message", receive);
    }
  };
}
