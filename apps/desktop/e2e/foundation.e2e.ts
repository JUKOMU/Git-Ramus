import { $, browser, expect } from "@wdio/globals";

const pluginUrl =
  process.platform === "win32"
    ? "http://git-ramus-plugin.localhost/git-ramus.welcome/ui.html"
    : "git-ramus-plugin://localhost/git-ramus.welcome/ui.html";

describe("Foundation microkernel", () => {
  it("loads the bundled plugin and starts a host-authorized task", async () => {
    await expect($("h1=Git-Ramus")).toBeDisplayed();
    const welcome = await $("button=Welcome");
    await welcome.click();
    const frame = await $("iframe[title='Welcome plugin']");
    await frame.waitForDisplayed();
    await expect(frame).toHaveAttribute("src", pluginUrl);
    await expect(frame).toHaveAttribute("sandbox", "allow-scripts");
    // The status is updated only after the real sandboxed plugin sends
    // plugin:ready and the host completes its first app.getInfo RPC. This
    // crosses the production postMessage boundary without weakening the
    // opaque-origin sandbox for testability.
    await browser.waitUntil(
      async () => (await frame.getAttribute("data-plugin-status")) === "rpc-complete",
      {
        timeout: 5000,
        timeoutMsg: "Welcome plugin did not complete its SDK handshake and first RPC"
      }
    );
    // Keep the production sandbox intact. The embedded driver's native script
    // wrapper cannot inspect a cross-origin sandboxed frame, so exercise the
    // same host authorization and job command that the plugin RPC route uses.
    const decision = await invokeHost("authorize_plugin_call", {
      request: {
        pluginId: "git-ramus.welcome",
        capability: "tasks:create",
        resource: "echo"
      }
    });
    expect(decision).toEqual({ allowed: true });
    const job = await invokeHost("start_echo_job", {
      request: {
        pluginId: "git-ramus.welcome",
        message: "Hello from Welcome"
      }
    });
    expect(job).toHaveProperty("id");
    await expect($("strong=Echo Hello from Welcome")).toBeDisplayed();
    await browser.waitUntil(async () => await $("span=100%").isDisplayed(), {
      timeout: 5000,
      timeoutMsg: "echo job did not complete"
    });
  });
});

async function invokeHost(command: string, args: unknown): Promise<unknown> {
  return browser.execute(
    async (commandName, commandArgs) => {
      const internals = (
        window as typeof window & {
          __TAURI_INTERNALS__?: {
            invoke: (command: string, args?: unknown) => Promise<unknown>;
          };
        }
      ).__TAURI_INTERNALS__;
      if (internals === undefined) {
        throw new Error("Tauri internals are not available");
      }
      return internals.invoke(commandName, commandArgs);
    },
    command,
    args
  );
}
