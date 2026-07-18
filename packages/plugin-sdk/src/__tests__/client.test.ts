import type { HostToPluginMessage, PluginToHostMessage } from "@git-ramus/contracts";
import { describe, expect, it } from "vitest";
import { createPluginClient, createRequestId, type PluginTransport } from "../client";
import { applyThemeToDocument } from "../theme";

class FakeTransport implements PluginTransport {
  readonly sent: PluginToHostMessage[] = [];
  private listener: ((message: HostToPluginMessage) => void) | null = null;

  send(message: PluginToHostMessage) {
    this.sent.push(message);
  }

  subscribe(listener: (message: HostToPluginMessage) => void) {
    this.listener = listener;
    return () => {
      this.listener = null;
    };
  }

  receive(message: HostToPluginMessage) {
    this.listener?.(message);
  }
}

describe("plugin client", () => {
  it("creates a UUID without relying on secure-context randomUUID", () => {
    const randomSource = {
      getRandomValues(bytes: Uint8Array<ArrayBuffer>) {
        bytes.set(Array.from({ length: 16 }, (_, index) => index));
        return bytes;
      }
    };

    expect(createRequestId(randomSource)).toBe("00010203-0405-4607-8809-0a0b0c0d0e0f");
  });

  it("waits for init, announces ready, and resolves an RPC result", async () => {
    const transport = new FakeTransport();
    const client = createPluginClient(transport, () => "87a31769-8aaa-47ca-bef3-47e66f0c62fc");
    transport.receive({
      type: "host:init",
      sessionId: "e3d622f1-f1f7-4f7e-8f18-3db8a1e6ffbe",
      pluginId: "git-ramus.welcome",
      sdkVersion: "0.1.0"
    });
    await client.ready;
    expect(transport.sent[0]).toEqual({
      type: "plugin:ready",
      sessionId: "e3d622f1-f1f7-4f7e-8f18-3db8a1e6ffbe"
    });
    const result = client.request<{ name: string }>("app.getInfo", {});
    transport.receive({
      type: "rpc:result",
      requestId: "87a31769-8aaa-47ca-bef3-47e66f0c62fc",
      sessionId: "e3d622f1-f1f7-4f7e-8f18-3db8a1e6ffbe",
      ok: true,
      result: { name: "Git-Ramus" }
    });
    await expect(result).resolves.toEqual({ name: "Git-Ramus" });
    client.dispose();
  });

  it("defaults legacy init messages to the root route", async () => {
    const transport = new FakeTransport();
    const client = createPluginClient(transport);
    transport.receive({
      type: "host:init",
      sessionId: "e3d622f1-f1f7-4f7e-8f18-3db8a1e6ffbe",
      pluginId: "git-ramus.welcome",
      sdkVersion: "0.1.0"
    });
    await expect(client.ready).resolves.toMatchObject({ route: "/" });
    client.dispose();
  });

  it("exposes route and applies validated theme updates", async () => {
    const transport = new FakeTransport();
    const client = createPluginClient(transport, () => "87a31769-8aaa-47ca-bef3-47e66f0c62fc");
    const themes: string[] = [];
    client.onThemeChanged((theme) => themes.push(theme.themeId));
    transport.receive({
      type: "host:init",
      sessionId: "e3d622f1-f1f7-4f7e-8f18-3db8a1e6ffbe",
      pluginId: "git-ramus.welcome",
      sdkVersion: "0.1.0",
      route: "/projects"
    });
    await expect(client.ready).resolves.toMatchObject({ route: "/projects" });
    transport.receive({
      type: "host:theme-changed",
      sessionId: "e3d622f1-f1f7-4f7e-8f18-3db8a1e6ffbe",
      theme: { themeId: "git-ramus.dark", colors: { background: "#000" } }
    });
    expect(client.currentTheme?.themeId).toBe("git-ramus.dark");
    expect(themes).toEqual(["git-ramus.dark"]);
    client.dispose();
  });

  it("rejects ready and requests when disposed before or after init", async () => {
    const transport = new FakeTransport();
    const client = createPluginClient(transport);
    const ready = client.ready;
    client.dispose();
    await expect(ready).rejects.toThrow("disposed");
    await expect(client.request("app.getInfo", {})).rejects.toThrow("disposed");
  });

  it("replaces sessions by rejecting pending calls and ignores foreign theme updates", async () => {
    const transport = new FakeTransport();
    const client = createPluginClient(transport, () => "87a31769-8aaa-47ca-bef3-47e66f0c62fc");
    transport.receive({
      type: "host:init",
      sessionId: "e3d622f1-f1f7-4f7e-8f18-3db8a1e6ffbe",
      pluginId: "git-ramus.welcome",
      sdkVersion: "0.1.0"
    });
    const pending = client.request("app.getInfo", {});
    transport.receive({
      type: "host:init",
      sessionId: "87a31769-8aaa-47ca-bef3-47e66f0c62fc",
      pluginId: "git-ramus.welcome",
      sdkVersion: "0.1.0"
    });
    await expect(pending).rejects.toThrow("session replaced");
    transport.receive({
      type: "host:theme-changed",
      sessionId: "e3d622f1-f1f7-4f7e-8f18-3db8a1e6ffbe",
      theme: { themeId: "git-ramus.old", colors: { background: "#000" } }
    });
    expect(client.currentTheme).toBeNull();
    client.dispose();
  });

  it("re-reads the active session after a pre-init request resumes", async () => {
    const transport = new FakeTransport();
    const client = createPluginClient(transport, () => "87a31769-8aaa-47ca-bef3-47e66f0c62fc");
    const request = client.request("app.getInfo", {});
    transport.receive({
      type: "host:init",
      sessionId: "e3d622f1-f1f7-4f7e-8f18-3db8a1e6ffbe",
      pluginId: "git-ramus.welcome",
      sdkVersion: "0.1.0"
    });
    transport.receive({
      type: "host:init",
      sessionId: "87a31769-8aaa-47ca-bef3-47e66f0c62fc",
      pluginId: "git-ramus.welcome",
      sdkVersion: "0.1.0"
    });
    await Promise.resolve();
    expect(transport.sent.at(-1)).toMatchObject({
      sessionId: "87a31769-8aaa-47ca-bef3-47e66f0c62fc"
    });
    transport.receive({
      type: "rpc:result",
      requestId: "87a31769-8aaa-47ca-bef3-47e66f0c62fc",
      sessionId: "87a31769-8aaa-47ca-bef3-47e66f0c62fc",
      ok: true,
      result: { ok: true }
    });
    await expect(request).resolves.toEqual({ ok: true });
    client.dispose();
  });

  it("exposes theme as a compatibility alias and isolates listener failures", async () => {
    const transport = new FakeTransport();
    const client = createPluginClient(transport);
    const seen: string[] = [];
    client.onThemeChanged(() => {
      throw new Error("listener failure");
    });
    client.onThemeChanged((theme) => seen.push(theme.themeId));
    transport.receive({
      type: "host:init",
      sessionId: "e3d622f1-f1f7-4f7e-8f18-3db8a1e6ffbe",
      pluginId: "git-ramus.welcome",
      sdkVersion: "0.1.0"
    });
    transport.receive({
      type: "host:theme-changed",
      sessionId: "e3d622f1-f1f7-4f7e-8f18-3db8a1e6ffbe",
      theme: { themeId: "git-ramus.dark", colors: { background: "#000" } }
    });
    expect(client.theme).toEqual(client.currentTheme);
    expect(seen).toEqual(["git-ramus.dark"]);
    client.dispose();
  });

  it("clears stale CSS variables when applying a sparse replacement", () => {
    const values = new Map<string, string>();
    const root = {
      style: {
        setProperty: (key: string, value: string) => values.set(key, value),
        removeProperty: (key: string) => values.delete(key)
      }
    } as unknown as HTMLElement;
    applyThemeToDocument(
      { themeId: "git-ramus.one", colors: { background: "#fff", text: "#111" } },
      root
    );
    applyThemeToDocument({ themeId: "git-ramus.two", colors: { background: "#000" } }, root);
    expect(values.get("--gr-colors-background")).toBe("#000");
    expect(values.has("--gr-colors-text")).toBe(false);
  });

  it("clears the active theme when the client is disposed", async () => {
    const values = new Map<string, string>();
    const root = {
      style: {
        setProperty: (key: string, value: string) => values.set(key, value),
        removeProperty: (key: string) => values.delete(key)
      }
    } as unknown as HTMLElement;
    Object.defineProperty(globalThis, "document", {
      configurable: true,
      value: { documentElement: root }
    });
    const transport = new FakeTransport();
    const client = createPluginClient(transport);
    transport.receive({
      type: "host:init",
      sessionId: "e3d622f1-f1f7-4f7e-8f18-3db8a1e6ffbe",
      pluginId: "git-ramus.welcome",
      sdkVersion: "0.1.0"
    });
    transport.receive({
      type: "host:theme-changed",
      sessionId: "e3d622f1-f1f7-4f7e-8f18-3db8a1e6ffbe",
      theme: { themeId: "git-ramus.dark", colors: { background: "#000" } }
    });
    expect(values.has("--gr-colors-background")).toBe(true);
    client.dispose();
    expect(client.currentTheme).toBeNull();
    expect(values.size).toBe(0);
    delete (globalThis as { document?: unknown }).document;
  });
});
