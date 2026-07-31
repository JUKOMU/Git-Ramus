import { resolve } from "node:path";
import { acquireE2eAppDataProfile } from "./app-data-profile";

const extension = process.platform === "win32" ? ".exe" : "";
const binary = resolve(
  import.meta.dirname,
  `../src-tauri/target/debug/git-ramus-desktop${extension}`
);
const tauriService = resolve(import.meta.dirname, "external-tauri-service.ts");
const appDataProfile = acquireE2eAppDataProfile();

export const config: WebdriverIO.Config = {
  runner: "local",
  specs: ["./plugin-forms.e2e.ts"],
  maxInstances: 1,
  injectGlobals: false,
  framework: "mocha",
  reporters: ["spec"],
  mochaOpts: {
    timeout: 60_000
  },
  transformRequest(requestOptions) {
    const headers = new Headers(requestOptions.headers);
    headers.delete("content-length");
    return { ...requestOptions, headers };
  },
  services: [
    [
      tauriService,
      {
        appBinaryPath: binary,
        driverProvider: "external",
        autoInstallTauriDriver: true,
        autoDownloadEdgeDriver: true,
        env: appDataProfile.env
      }
    ]
  ],
  capabilities: [
    {
      browserName: "tauri"
    }
  ]
};
