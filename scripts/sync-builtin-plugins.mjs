import { cp, mkdir, readFile, rm } from "node:fs/promises";
import { resolve } from "node:path";

const root = resolve(import.meta.dirname, "..");
const source = resolve(root, "plugins/builtin-welcome");
const destination = resolve(root, "apps/desktop/src-tauri/resources/plugins/git-ramus.welcome");
const manifest = JSON.parse(await readFile(resolve(source, "plugin.json"), "utf8"));

if (manifest.id !== "git-ramus.welcome" || manifest.entrypoints.ui !== "ui.html") {
  throw new Error("Welcome manifest does not match its staged location");
}

await rm(destination, { recursive: true, force: true });
await mkdir(destination, { recursive: true });
await cp(resolve(source, "plugin.json"), resolve(destination, "plugin.json"));
await cp(resolve(source, "dist/index.html"), resolve(destination, "ui.html"));
