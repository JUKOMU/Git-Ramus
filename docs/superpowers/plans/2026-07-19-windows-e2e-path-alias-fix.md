# Windows E2E Path Alias Fix Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the Windows native E2E isolation assertion recognize DOS 8.3 aliases and canonical long paths as the same existing filesystem location without weakening cleanup safety.

**Architecture:** Add a separate asynchronous physical-path canonicalizer beside the existing lexical normalizer. Unit-test it with a junction/symlink alias, then use it only at the E2E assertion boundary; cleanup continues using the lexical path plus `lstat` guards.

**Tech Stack:** TypeScript, Node.js `fs/promises`, Vitest, WebdriverIO, Tauri E2E

---

## File map

- Modify `apps/desktop/e2e/app-data-profile.ts`: export physical canonicalization for existing E2E paths while leaving cleanup normalization unchanged.
- Modify `apps/desktop/e2e/app-data-profile.test.ts`: prove a filesystem alias and its target canonicalize identically and retain cleanup tests.
- Modify `apps/desktop/e2e/git-client.e2e.ts`: compare actual and expected app-data paths after physical canonicalization.

### Task 1: Add and verify physical E2E path canonicalization

**Files:**

- Modify: `apps/desktop/e2e/app-data-profile.ts`
- Test: `apps/desktop/e2e/app-data-profile.test.ts`

- [ ] **Step 1: Write the failing alias regression test**

Update the imports and add this test to `app-data-profile.test.ts`:

```ts
import { access, mkdir, mkdtemp, rm, symlink } from "node:fs/promises";

import {
  acquireE2eAppDataProfile,
  canonicalizeExistingE2eFsPath,
  cleanupE2eAppDataProfile,
  cleanupOwnedE2eAppDataProfile,
  createE2eAppDataProfile,
  E2E_APP_DATA_PREFIX,
  E2E_APP_DATA_ROOT_ENV
} from "./app-data-profile";

it("canonicalizes a filesystem alias to the same existing E2E path", async () => {
  const parent = await mkdtemp(join(tmpdir(), E2E_APP_DATA_PREFIX));
  const target = join(parent, "physical");
  const alias = join(parent, "alias");
  try {
    await mkdir(target);
    await symlink(target, alias, process.platform === "win32" ? "junction" : "dir");
    await expect(canonicalizeExistingE2eFsPath(alias)).resolves.toBe(
      await canonicalizeExistingE2eFsPath(target)
    );
  } finally {
    await rm(parent, { recursive: true, force: true });
  }
});
```

- [ ] **Step 2: Run the focused test and verify RED**

Run:

```powershell
npm --prefix apps/desktop test -- e2e/app-data-profile.test.ts
```

Expected: FAIL because `app-data-profile.ts` does not export `canonicalizeExistingE2eFsPath`.

- [ ] **Step 3: Implement the minimal physical canonicalizer**

Update `app-data-profile.ts`:

```ts
import { lstat, realpath, rm } from "node:fs/promises";

export async function canonicalizeExistingE2eFsPath(path: string): Promise<string> {
  return normalizeE2eFsPath(await realpath(path));
}
```

Do not change `normalizeE2eFsPath`, `cleanupE2eAppDataProfile`, or `assertSafeExistingProfile`.

- [ ] **Step 4: Run the focused tests and verify GREEN**

Run:

```powershell
npm --prefix apps/desktop test -- e2e/app-data-profile.test.ts
```

Expected: all `app-data-profile` tests PASS, including alias canonicalization and nested cleanup rejection.

- [ ] **Step 5: Commit the helper and regression test**

```powershell
git add -- apps/desktop/e2e/app-data-profile.ts apps/desktop/e2e/app-data-profile.test.ts
git commit -m "test: cover e2e filesystem aliases"
```

### Task 2: Apply canonicalization at the native E2E assertion boundary

**Files:**

- Modify: `apps/desktop/e2e/git-client.e2e.ts`

- [ ] **Step 1: Replace lexical equality with physical equality**

Change the imports and `assertIsolatedAppData` implementation:

```ts
import { lstat } from "node:fs/promises";
import { resolve } from "node:path";
import {
  canonicalizeExistingE2eFsPath,
  E2E_APP_DATA_ROOT_ENV
} from "./app-data-profile";

async function assertIsolatedAppData(): Promise<void> {
  const expectedRoot = process.env[E2E_APP_DATA_ROOT_ENV];
  if (expectedRoot === undefined) throw new Error("E2E app-data profile is unavailable");
  const paths = record(await invokeHost("e2e_app_data_paths", {}));
  const [appDataRoot, databasePath, expectedAppDataRoot, expectedDatabasePath] = await Promise.all([
    canonicalizeExistingE2eFsPath(text(paths.appDataRoot)),
    canonicalizeExistingE2eFsPath(text(paths.databasePath)),
    canonicalizeExistingE2eFsPath(expectedRoot),
    canonicalizeExistingE2eFsPath(resolve(expectedRoot, "git-ramus.db"))
  ]);
  expect(appDataRoot).toBe(expectedAppDataRoot);
  expect(databasePath).toBe(expectedDatabasePath);
  expect((await lstat(databasePath)).isFile()).toBe(true);
  console.info(`E2E app-data root=${appDataRoot} database=${databasePath}`);
}
```

- [ ] **Step 2: Run TypeScript and focused tests**

Run:

```powershell
npm --prefix apps/desktop run typecheck
npm --prefix apps/desktop test -- e2e/app-data-profile.test.ts
```

Expected: both commands PASS.

- [ ] **Step 3: Build and run the native E2E suite**

Run:

```powershell
$cargoBin = 'C:\Users\Vulon\.rustup\toolchains\1.88.0-x86_64-pc-windows-msvc\bin'
$env:PATH = "$cargoBin;$env:PATH"
npm run build:e2e --workspace @git-ramus/desktop
npm run test:e2e --workspace @git-ramus/desktop
```

Expected: Foundation and Git Client specs both PASS; the logged app-data paths are physically canonical and the isolated profile is removed.

- [ ] **Step 4: Run the repository release checks**

Run:

```powershell
npm run check
npm audit --audit-level=high
git diff --check
git status --short
```

Expected: formatting, lint, typecheck, unit tests, audit, and diff checks PASS; only the three intended E2E files and this plan's checkbox updates are modified.

- [ ] **Step 5: Commit, push, and verify the workflow**

```powershell
git add -- apps/desktop/e2e/git-client.e2e.ts docs/superpowers/plans/2026-07-19-windows-e2e-path-alias-fix.md
git commit -m "fix: canonicalize windows e2e paths"
git push origin main
```

Expected: `main` pushes successfully. The next Windows E2E job passes the app-data isolation assertion when `TEMP` uses `RUNNER~1` and the host returns `runneradmin`.
