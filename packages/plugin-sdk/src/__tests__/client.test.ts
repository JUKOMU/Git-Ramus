import type { HostToPluginMessage, PluginToHostMessage } from "@git-ramus/contracts";
import { describe, expect, it } from "vitest";
import { createPluginClient, createRequestId, type PluginTransport } from "../client";

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
});
