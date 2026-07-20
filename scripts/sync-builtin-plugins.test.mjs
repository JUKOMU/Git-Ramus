import assert from "node:assert/strict";
import { execFile } from "node:child_process";
import { readFile, readdir } from "node:fs/promises";
import { resolve } from "node:path";
import { execPath } from "node:process";
import { promisify } from "node:util";
import test from "node:test";

const run = promisify(execFile);
const root = resolve(import.meta.dirname, "..");
const resources = resolve(root, "apps/desktop/src-tauri/resources/plugins");

test("sync stages each built-in UI, backend Provider, and only the declared theme definition", async () => {
  await run(execPath, [resolve(import.meta.dirname, "sync-builtin-plugins.mjs")], {
    cwd: root
  });

  assert.deepEqual(await sortedEntries("git-ramus.welcome"), ["plugin.json", "ui.html"]);
  assert.deepEqual(await sortedEntries("git-ramus.git-client"), ["plugin.json", "ui.html"]);
  assert.deepEqual(await sortedEntries("git-ramus.provider-center"), ["plugin.json", "ui.html"]);
  assert.deepEqual(await sortedEntries("git-ramus.compact-theme"), [
    "plugin.json",
    "theme.json",
    "ui.html"
  ]);
  assert.deepEqual(await sortedEntries("git-ramus.provider.github"), ["plugin.json"]);
  assert.deepEqual(await sortedEntries("git-ramus.provider.gitlab"), ["plugin.json"]);
  assert.equal(
    await readFile(resolve(resources, "git-ramus.compact-theme/theme.json"), "utf8"),
    await readFile(resolve(root, "plugins/builtin-compact-theme/theme.json"), "utf8")
  );
});

async function sortedEntries(pluginId) {
  return (await readdir(resolve(resources, pluginId))).sort();
}
