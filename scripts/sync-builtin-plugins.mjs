import { execFile } from "node:child_process";
import { cp, mkdir, readFile, rm } from "node:fs/promises";
import { dirname, resolve } from "node:path";
import { env, execPath, platform } from "node:process";
import { promisify } from "node:util";

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
  }
];

for (const plugin of plugins) {
  const npmArguments = ["run", "build", "--workspace", plugin.workspace];
  await run(npmCommand, npmCli === null ? npmArguments : [npmCli, ...npmArguments], { cwd: root });
  await stagePlugin(plugin);
}

async function stagePlugin(plugin) {
  const manifest = JSON.parse(await readFile(resolve(plugin.source, "plugin.json"), "utf8"));
  if (manifest.id !== plugin.id || manifest.entrypoints.ui !== "ui.html") {
    throw new Error(`${plugin.workspace} manifest does not match its staged location`);
  }

  const destination = resolve(root, "apps/desktop/src-tauri/resources/plugins", plugin.id);
  await rm(destination, { recursive: true, force: true });
  await mkdir(destination, { recursive: true });
  await cp(resolve(plugin.source, "plugin.json"), resolve(destination, "plugin.json"));
  await cp(resolve(plugin.source, "dist/index.html"), resolve(destination, "ui.html"));
}
