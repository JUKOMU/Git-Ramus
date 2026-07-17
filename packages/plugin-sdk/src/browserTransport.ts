import {
  hostToPluginMessageSchema,
  type HostToPluginMessage,
  type PluginToHostMessage
} from "@git-ramus/contracts";
import type { PluginTransport } from "./client";

export function createBrowserTransport(currentWindow: Window = window): PluginTransport {
  return {
    send(message: PluginToHostMessage) {
      currentWindow.parent.postMessage(message, "*");
    },
    subscribe(listener: (message: HostToPluginMessage) => void) {
      const receive = (event: MessageEvent<unknown>) => {
        if (event.source !== currentWindow.parent) {
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
