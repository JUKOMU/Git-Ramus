import { resolve } from "node:path";

const extension = process.platform === "win32" ? ".exe" : "";
const binary = resolve(
  import.meta.dirname,
  `../src-tauri/target/debug/git-ramus-desktop${extension}`
);
const tauriService = resolve(import.meta.dirname, "basic-tauri-service.ts");

export const config: WebdriverIO.Config = {
  runner: "local",
  specs: ["./foundation.e2e.ts", "./git-client.e2e.ts"],
  maxInstances: 1,
  injectGlobals: false,
  framework: "mocha",
  reporters: ["spec"],
  mochaOpts: {
    timeout: 15000
  },
  transformRequest(requestOptions) {
    const headers = new Headers(requestOptions.headers);
    // Node 26 can expose a cross-Undici dispatcher that rejects WDIO's manual value.
    headers.delete("content-length");
    return { ...requestOptions, headers };
  },
  services: [
    [
      tauriService,
      {
        appBinaryPath: binary,
        driverProvider: "embedded",
        embeddedPort: 4445
      }
    ]
  ],
  capabilities: [
    {
      browserName: "tauri"
    }
  ]
};
