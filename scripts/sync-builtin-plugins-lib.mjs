import { cp, lstat, mkdir, mkdtemp, readFile, rename, rm } from "node:fs/promises";
import { dirname, extname, isAbsolute, relative, resolve, sep } from "node:path";

export async function syncBuiltinPlugins({
  plugins,
  destinationRoot,
  buildPlugin,
  copyFile = cp,
  renamePath = rename
}) {
  for (const plugin of plugins) await buildPlugin(plugin);

  const transactionRoot = dirname(destinationRoot);
  await mkdir(transactionRoot, { recursive: true });
  const stagingRoot = await mkdtemp(resolve(transactionRoot, ".plugins-staging-"));
  const backupRoot = `${stagingRoot}.backup`;
  let backupHasOriginal = false;

  try {
    const destinationExists = await pathExists(destinationRoot);
    if (destinationExists) {
      await cp(destinationRoot, stagingRoot, { recursive: true });
    }
    for (const plugin of plugins) {
      await stagePlugin(plugin, stagingRoot, copyFile);
    }

    if (destinationExists) {
      await renamePath(destinationRoot, backupRoot);
      backupHasOriginal = true;
    }

    try {
      await renamePath(stagingRoot, destinationRoot);
    } catch (replaceError) {
      if (backupHasOriginal) {
        try {
          await renamePath(backupRoot, destinationRoot);
          backupHasOriginal = false;
        } catch (rollbackError) {
          throw new AggregateError(
            [replaceError, rollbackError],
            "plugin replacement and rollback both failed",
            { cause: rollbackError }
          );
        }
      }
      throw replaceError;
    }

    if (backupHasOriginal) {
      await rm(backupRoot, { recursive: true, force: true });
      backupHasOriginal = false;
    }
  } finally {
    await rm(stagingRoot, { recursive: true, force: true });
    if (!backupHasOriginal) {
      await rm(backupRoot, { recursive: true, force: true });
    }
  }
}

async function stagePlugin(plugin, stagingRoot, copyFile) {
  const manifest = JSON.parse(await readFile(resolve(plugin.source, "plugin.json"), "utf8"));
  if (manifest.id !== plugin.id || manifest.entrypoints?.ui !== "ui.html") {
    throw new Error(`${plugin.workspace} manifest does not match its staged location`);
  }

  const destination = containedPath(stagingRoot, plugin.id);
  await rm(destination, { recursive: true, force: true });
  await mkdir(destination, { recursive: true });
  await copyFile(resolve(plugin.source, "plugin.json"), resolve(destination, "plugin.json"));
  await copyFile(resolve(plugin.source, "dist/index.html"), resolve(destination, "ui.html"));

  const theme = manifest.contributions?.theme;
  if (theme !== undefined) {
    const definition = resolveThemeDefinition(theme);
    const source = containedPath(plugin.source, definition);
    const target = containedPath(destination, definition);
    await mkdir(dirname(target), { recursive: true });
    await copyFile(source, target);
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
    relation.startsWith(`..${sep}`) ||
    relation === ".." ||
    isAbsolute(relation)
  ) {
    throw new Error("staged path escapes its plugin root");
  }
  return candidate;
}

async function pathExists(path) {
  try {
    await lstat(path);
    return true;
  } catch (error) {
    if (error?.code === "ENOENT") return false;
    throw error;
  }
}
