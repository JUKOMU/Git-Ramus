import { execFile } from "node:child_process";
import { dirname, resolve } from "node:path";
import { env, execPath, platform } from "node:process";
import { promisify } from "node:util";
import { syncBuiltinPlugins } from "./sync-builtin-plugins-lib.mjs";

const root = resolve(import.meta.dirname, "..");
const run = promisify(execFile);
const npmCli =
  env.npm_execpath ??
  (platform === "win32" ? resolve(dirname(execPath), "node_modules/npm/bin/npm-cli.js") : null);
const npmCommand = npmCli === null ? "npm" : execPath;
const plugins = [
  {
    workspace: "@git-ramus/builtin-welcome",
    source: resolve(root, "plugins/builtin-welcome"),
    id: "git-ramus.welcome"
  },
  {
    workspace: "@git-ramus/git-client",
    source: resolve(root, "plugins/git-client"),
    id: "git-ramus.git-client"
  },
  {
    workspace: "@git-ramus/builtin-compact-theme",
    source: resolve(root, "plugins/builtin-compact-theme"),
    id: "git-ramus.compact-theme"
  },
  {
    workspace: null,
    source: resolve(root, "plugins/provider-github"),
    id: "git-ramus.provider.github"
  },
  {
    workspace: null,
    source: resolve(root, "plugins/provider-gitlab"),
    id: "git-ramus.provider.gitlab"
  }
];

await syncBuiltinPlugins({
  plugins,
  destinationRoot: resolve(root, "apps/desktop/src-tauri/resources/plugins"),
  buildPlugin: async (plugin) => {
    if (plugin.workspace === null) return;
    const npmArguments = ["run", "build", "--workspace", plugin.workspace];
    await run(npmCommand, npmCli === null ? npmArguments : [npmCli, ...npmArguments], {
      cwd: root
    });
  }
});
