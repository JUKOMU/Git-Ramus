import { describe, expect, it } from "vitest";
import { createBrowserTransport } from "../browserTransport";

function fakeWindow() {
  const listeners = new Set<(event: MessageEvent<unknown>) => void>();
  const sent: Array<{ message: unknown; targetOrigin: string }> = [];
  const parent = {
    postMessage: (message: unknown, targetOrigin: string) => sent.push({ message, targetOrigin })
  };
  return {
    sent,
    parent,
    addEventListener: (_type: string, listener: (event: MessageEvent<unknown>) => void) =>
      listeners.add(listener),
    removeEventListener: (_type: string, listener: (event: MessageEvent<unknown>) => void) =>
      listeners.delete(listener),
    emit: (event: MessageEvent<unknown>) => listeners.forEach((listener) => listener(event))
  } as unknown as Window & { sent: typeof sent; emit: (event: MessageEvent<unknown>) => void };
}

describe("browser transport origin validation", () => {
  it("preserves wildcard defaults", () => {
    const currentWindow = fakeWindow();
    createBrowserTransport(currentWindow).send({
      type: "plugin:ready",
      sessionId: "e3d622f1-f1f7-4f7e-8f18-3db8a1e6ffbe"
    });
    expect(currentWindow.sent[0]?.targetOrigin).toBe("*");
  });

  it("validates configured origin while retaining parent source checks", () => {
    const currentWindow = fakeWindow();
    const transport = createBrowserTransport(currentWindow, { targetOrigin: "https://host.test" });
    const received: unknown[] = [];
    transport.subscribe((message) => received.push(message));
    const valid = {
      type: "host:init",
      sessionId: "e3d622f1-f1f7-4f7e-8f18-3db8a1e6ffbe",
      pluginId: "git-ramus.welcome",
      sdkVersion: "0.1.0"
    };
    currentWindow.emit({
      data: valid,
      source: currentWindow.parent,
      origin: "https://evil.test"
    } as MessageEvent);
    currentWindow.emit({
      data: valid,
      source: currentWindow.parent,
      origin: "https://host.test"
    } as MessageEvent);
    expect(received).toHaveLength(1);
  });
});
