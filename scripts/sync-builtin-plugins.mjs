import { execFile } from "node:child_process";
import { cp, mkdir, readFile, rm } from "node:fs/promises";
import { dirname, extname, isAbsolute, relative, resolve } from "node:path";
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
  },
  {
    workspace: "@git-ramus/builtin-compact-theme",
    source: resolve(root, "plugins/builtin-compact-theme"),
    id: "git-ramus.compact-theme"
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

  const theme = manifest.contributions?.theme;
  if (theme !== undefined) {
    const definition = resolveThemeDefinition(theme);
    const source = containedPath(plugin.source, definition);
    const target = containedPath(destination, definition);
    await mkdir(dirname(target), { recursive: true });
    await cp(source, target);
  }
}

function resolveThemeDefinition(theme) {
  if (typeof theme !== "object" || theme === null || Array.isArray(theme)) {
    throw new Error("theme contribution must be an object");
  }
  const keys = Object.keys(theme);
  if (keys.some((key) => !["themeId", "definition", "definitionPath"].includes(key))) {
    throw new Error("theme contribution contains an unknown field");
  }
  if (typeof theme.themeId !== "string" || !/^[a-z0-9]+(?:[.-][a-z0-9]+)+$/u.test(theme.themeId)) {
    throw new Error("theme contribution has an invalid themeId");
  }
  if (
    theme.definition !== undefined &&
    theme.definitionPath !== undefined &&
    theme.definition !== theme.definitionPath
  ) {
    throw new Error("theme definition and definitionPath must match");
  }
  const definition = theme.definition ?? theme.definitionPath;
  if (!isSafeRelativePath(definition) || extname(definition).toLowerCase() !== ".json") {
    throw new Error("theme definition must be a safe relative JSON path");
  }
  if (definition === "plugin.json" || definition === "ui.html") {
    throw new Error("theme definition cannot replace a staged host file");
  }
  return definition;
}

function isSafeRelativePath(value) {
  return (
    typeof value === "string" &&
    value.length > 0 &&
    !isAbsolute(value) &&
    !/^(?:[\\/]|[A-Za-z]:)/u.test(value) &&
    !/^[A-Za-z][A-Za-z0-9+.-]*:/u.test(value) &&
    !Array.from(value).some((character) => {
      const code = character.charCodeAt(0);
      return code < 0x20 || code === 0x7f;
    }) &&
    !value.split(/[\\/]/u).includes("..")
  );
}

function containedPath(rootPath, relativePath) {
  const candidate = resolve(rootPath, relativePath);
  const relation = relative(rootPath, candidate);
  if (
    relation === "" ||
    relation.startsWith(`..${separator()}`) ||
    relation === ".." ||
    isAbsolute(relation)
  ) {
    throw new Error("staged path escapes its plugin root");
  }
  return candidate;
}

function separator() {
  return platform === "win32" ? "\\" : "/";
}
