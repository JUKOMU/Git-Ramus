import assert from "node:assert/strict";
import { cp, mkdir, mkdtemp, readFile, readdir, rename, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import { basename, dirname, resolve } from "node:path";
import test from "node:test";
import { syncBuiltinPlugins } from "./sync-builtin-plugins-lib.mjs";

const failureCases = [
  {
    name: "an invalid theme path",
    prepare: async (fixture) => {
      const manifest = JSON.parse(await readFile(resolve(fixture.source, "plugin.json"), "utf8"));
      manifest.contributions.theme.definition = "../theme.json";
      await writeFile(resolve(fixture.source, "plugin.json"), JSON.stringify(manifest));
    },
    options: {},
    error: /safe relative JSON path/u
  },
  {
    name: "a missing theme definition",
    prepare: async (fixture) => rm(resolve(fixture.source, "theme.json")),
    options: {},
    error: /ENOENT/u
  },
  {
    name: "a copy failure",
    prepare: async () => undefined,
    options: {
      copyFile: async (source, target) => {
        if (basename(source) === "index.html") throw new Error("injected copy failure");
        await cp(source, target);
      }
    },
    error: /injected copy failure/u
  }
];

for (const failureCase of failureCases) {
  test(`sync preserves the complete destination after ${failureCase.name}`, async () => {
    const fixture = await createFixture();
    try {
      await failureCase.prepare(fixture);
      const before = await snapshotTree(fixture.destinationRoot);

      await assert.rejects(
        syncBuiltinPlugins({
          plugins: [fixture.plugin],
          destinationRoot: fixture.destinationRoot,
          buildPlugin: async () => undefined,
          ...failureCase.options
        }),
        failureCase.error
      );

      assert.deepEqual(await snapshotTree(fixture.destinationRoot), before);
      await assertNoTransactionResidue(fixture.destinationRoot);
    } finally {
      await rm(fixture.root, { recursive: true, force: true });
    }
  });
}

test("sync rolls the complete destination back when atomic replacement fails", async () => {
  const fixture = await createFixture();
  try {
    const before = await snapshotTree(fixture.destinationRoot);
    let replacementFailed = false;

    await assert.rejects(
      syncBuiltinPlugins({
        plugins: [fixture.plugin],
        destinationRoot: fixture.destinationRoot,
        buildPlugin: async () => undefined,
        renamePath: async (source, target) => {
          if (
            !replacementFailed &&
            basename(source).startsWith(".plugins-staging-") &&
            target === fixture.destinationRoot
          ) {
            replacementFailed = true;
            throw new Error("injected replace failure");
          }
          await rename(source, target);
        }
      }),
      /injected replace failure/u
    );

    assert.deepEqual(await snapshotTree(fixture.destinationRoot), before);
    await assertNoTransactionResidue(fixture.destinationRoot);
  } finally {
    await rm(fixture.root, { recursive: true, force: true });
  }
});

test("sync preserves unrelated resources while replacing configured plugins", async () => {
  const fixture = await createFixture();
  try {
    await syncBuiltinPlugins({
      plugins: [fixture.plugin],
      destinationRoot: fixture.destinationRoot,
      buildPlugin: async () => undefined
    });

    assert.equal(
      await readFile(resolve(fixture.destinationRoot, "sentinel.txt"), "utf8"),
      "keep-root"
    );
    assert.equal(
      await readFile(resolve(fixture.destinationRoot, "existing/sentinel.txt"), "utf8"),
      "keep-nested"
    );
    assert.deepEqual((await readdir(resolve(fixture.destinationRoot, fixture.plugin.id))).sort(), [
      "plugin.json",
      "theme.json",
      "ui.html"
    ]);
    await assertNoTransactionResidue(fixture.destinationRoot);
  } finally {
    await rm(fixture.root, { recursive: true, force: true });
  }
});

async function createFixture() {
  const root = await mkdtemp(resolve(tmpdir(), "git-ramus-sync-"));
  const source = resolve(root, "source");
  const destinationRoot = resolve(root, "resources/plugins");
  await mkdir(resolve(source, "dist"), { recursive: true });
  await mkdir(resolve(destinationRoot, "existing"), { recursive: true });
  await writeFile(resolve(destinationRoot, "sentinel.txt"), "keep-root");
  await writeFile(resolve(destinationRoot, "existing/sentinel.txt"), "keep-nested");
  await writeFile(
    resolve(source, "plugin.json"),
    JSON.stringify({
      schemaVersion: 1,
      id: "git-ramus.fixture",
      entrypoints: { ui: "ui.html" },
      contributions: {
        theme: { themeId: "git-ramus.theme.fixture", definition: "theme.json" }
      }
    })
  );
  await writeFile(resolve(source, "dist/index.html"), "<main>fixture</main>");
  await writeFile(
    resolve(source, "theme.json"),
    JSON.stringify({ themeId: "git-ramus.theme.fixture" })
  );
  return {
    root,
    source,
    destinationRoot,
    plugin: { workspace: "@git-ramus/fixture", source, id: "git-ramus.fixture" }
  };
}

async function snapshotTree(rootPath, relativePath = "") {
  const current = resolve(rootPath, relativePath);
  const snapshot = {};
  for (const entry of await readdir(current, { withFileTypes: true })) {
    const entryPath = relativePath === "" ? entry.name : `${relativePath}/${entry.name}`;
    if (entry.isDirectory()) {
      Object.assign(snapshot, await snapshotTree(rootPath, entryPath));
    } else {
      snapshot[entryPath] = (await readFile(resolve(rootPath, entryPath))).toString("base64");
    }
  }
  return snapshot;
}

async function assertNoTransactionResidue(destinationRoot) {
  const residue = (await readdir(dirname(destinationRoot))).filter((entry) =>
    entry.startsWith(".plugins-")
  );
  assert.deepEqual(residue, []);
}
