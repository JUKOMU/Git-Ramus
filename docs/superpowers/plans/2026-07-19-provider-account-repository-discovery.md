# Provider Account and Repository Discovery Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the first secure Provider vertical slice for GitHub.com, GitLab.com, and HTTPS self-managed GitLab: multiple PAT accounts, repository discovery, scoped plugin access, and confirmed bindings to existing local Git remotes.

**Architecture:** Rust owns Provider persistence, system-keychain access, scoped HTTP/TLS, pagination, cancellation, adapter execution, and binding invariants. GitHub and GitLab are backend-only built-in plugins registered through typed manifest contributions; one sandboxed Provider Center plugin uses typed RPC, while PAT and Provider-access prompts remain in trusted Shell UI outside the iframe.

**Tech Stack:** Tauri 2.11, Rust 1.88/edition 2024, rusqlite, reqwest 0.13.4 with platform TLS verification, Tokio, React 19, TypeScript 6, Zod 4, Vite single-file plugins, Vitest/Testing Library, Rust httpmock, WebdriverIO Tauri E2E

---

## Scope boundary and working rules

This plan implements only [the approved Provider account and repository discovery specification](../specs/2026-07-19-provider-account-repository-discovery-design.md).

It does not implement Git Clone/Fetch/Pull/Push, SSH/GCM transport profiles, GitHub Enterprise Server, OAuth/Device Flow, Release APIs, Skills Manager, or external native Provider adapters. Do not add disabled buttons or empty interfaces for those later slices. Keep `ReleaseProvider` out of production code until its own approved plan.

At execution time, use `superpowers:using-git-worktrees` to create `D:/Git-Ramus/.worktrees/provider-account-discovery` on `codex/provider-account-discovery`. Start from the commit containing this plan and the approved design. Never run Provider tests against real accounts or real PATs; unit/integration tests use in-memory secrets and local mock adapters/servers.

Every production behavior follows red-green-refactor:

1. Write one focused failing test.
2. Run the exact focused command and observe the expected failure.
3. Add the smallest production implementation that satisfies it.
4. Run the focused test and the relevant workspace checks.
5. Commit only the files named by that task.

Repository-wide release commands:

```powershell
npm run check
npm audit --audit-level=high
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npm run build:e2e --workspace @git-ramus/desktop
npm run test:e2e --workspace @git-ramus/desktop
```

Provider API implementation must follow the current official behavior documented by [GitHub authenticated repositories](https://docs.github.com/en/rest/repos/repos), [GitHub REST authentication](https://docs.github.com/en/rest/authentication/authenticating-to-the-rest-api), [GitLab REST authentication](https://docs.gitlab.com/api/rest/authentication/), [GitLab projects](https://docs.gitlab.com/api/projects/), and [GitLab pagination](https://docs.gitlab.com/api/rest/). Pin GitHub's request header to `X-GitHub-Api-Version: 2026-03-10`; changing that date later requires a focused compatibility test.

## File map

### Shared contracts

- Create `packages/contracts/src/provider.ts`: public Provider instance, account, repository, query, page, binding, matching, deletion-impact, and request schemas.
- Create `packages/contracts/src/__fixtures__/provider-contracts.json`: secret-free canonical instance/account/page/binding/error round-trip values shared with Rust tests.
- Modify `packages/contracts/src/plugin.ts`: typed Provider contributions and backend-only built-in plugin entrypoints.
- Modify `packages/contracts/src/index.ts`: export Provider contracts.
- Modify `packages/contracts/src/__tests__/contracts.test.ts`: cross-language manifest and Provider DTO tests.

### Rust plugin runtime and persistence

- Create `apps/desktop/src-tauri/migrations/0003_provider_discovery.sql`: Provider instances, accounts, bindings, and secret-cleanup queue.
- Modify `apps/desktop/src-tauri/src/db/migrations.rs`: transactional v3 upgrade.
- Modify `apps/desktop/src-tauri/src/db/mod.rs`: v2-to-v3, constraint, and idempotency tests.
- Modify `apps/desktop/src-tauri/src/plugins/manifest.rs`: Rust mirror of optional UI and Provider contributions.
- Modify `apps/desktop/src-tauri/src/plugins/registry.rs`: discover UI and backend-only built-ins without inventing an iframe URL.
- Modify `apps/desktop/src-tauri/src/plugins/protocol.rs`: return 404 for backend-only plugin UI requests.
- Modify `apps/desktop/src-tauri/src/plugins/permissions.rs`: declared-permission checks, dynamic account grants, listing, and revocation.

### Rust Provider domain

- Create `apps/desktop/src-tauri/src/providers/mod.rs`: focused module exports.
- Create `apps/desktop/src-tauri/src/providers/model.rs`: internal models and public serializable summaries.
- Create `apps/desktop/src-tauri/src/providers/store.rs`: all SQL for instances, accounts, bindings, impacts, and cleanup records.
- Create `apps/desktop/src-tauri/src/providers/url.rs`: instance and Git remote normalization/detection.
- Create `apps/desktop/src-tauri/src/providers/http.rs`: same-origin bounded HTTP/TLS client.
- Create `apps/desktop/src-tauri/src/providers/cursor.rs`: one-use opaque cursor and operation-cancellation registries.
- Create `apps/desktop/src-tauri/src/providers/adapter.rs`: object-safe discovery adapter contract and built-in registry.
- Create `apps/desktop/src-tauri/src/providers/github.rs`: GitHub REST mapping.
- Create `apps/desktop/src-tauri/src/providers/gitlab.rs`: GitLab REST v4 mapping.
- Create `apps/desktop/src-tauri/src/providers/service.rs`: instance/account lifecycle, discovery, filtering, matching, binding, and secret compensation.
- Create `apps/desktop/src-tauri/src/providers/e2e_adapter.rs`: debug+e2e-only deterministic Provider adapter.
- Modify `apps/desktop/src-tauri/src/secrets.rs`: zeroizing sensitive-string wrapper and test doubles.
- Modify `apps/desktop/src-tauri/src/error.rs`: stable redacted Provider failures.
- Modify `apps/desktop/src-tauri/src/app_state.rs`: construct Provider services with production/e2e secret stores.
- Modify `apps/desktop/src-tauri/src/commands.rs`: typed Provider Tauri commands.
- Modify `apps/desktop/src-tauri/src/lib.rs`: export/register Provider modules and commands.
- Modify `apps/desktop/src-tauri/Cargo.toml` and `apps/desktop/src-tauri/Cargo.lock`: HTTP, cancellation, zeroization, and test dependencies.
- Create `apps/desktop/src-tauri/tests/provider_integration.rs`: black-box adapter/service tests with local servers.

### Existing Git remote synchronization

- Modify `apps/desktop/src-tauri/src/git/parser.rs`: parse remote URL configuration.
- Modify `apps/desktop/src-tauri/src/git/repository.rs`: transactionally replace current remote rows.
- Modify `apps/desktop/src-tauri/src/git/service.rs`: refresh remotes alongside repository state.
- Modify `apps/desktop/src-tauri/tests/git_service_integration.rs`: prove real local remotes reach SQLite.

### Desktop Shell and RPC

- Create `apps/desktop/src/providers/promptBroker.ts`: one-at-a-time trusted prompt broker.
- Create `apps/desktop/src/providers/ProviderCredentialDialog.tsx`: PAT entry outside plugin iframes.
- Create `apps/desktop/src/providers/ProviderAccessDialog.tsx`: account-scoped external-plugin authorization.
- Create `apps/desktop/src/providers/__tests__/providerPrompts.test.tsx`: prompt isolation, clearing, and cancellation tests.
- Modify `apps/desktop/src/lib/hostApi.ts`: typed Provider invokes, native CA selection, and trusted prompt ports.
- Modify `apps/desktop/src/lib/__tests__/hostApi.test.ts`: exact commands, schemas, and secret-boundary tests.
- Modify `apps/desktop/src/plugins/rpcRouter.ts`: Provider routes with all-of/any-of dynamic permission requirements.
- Modify `apps/desktop/src/plugins/__tests__/rpcRouter.test.ts`: authorization-order and account-scope tests.
- Modify `apps/desktop/src/plugins/PluginHost.tsx`: refuse to mount a backend-only descriptor as an iframe.
- Modify `apps/desktop/src/plugins/__tests__/PluginFrame.test.tsx`: backend-only descriptor regression test.
- Create `apps/desktop/src/providers/promptPorts.ts`: trusted prompt/file interfaces and a temporary unavailable implementation.
- Modify `apps/desktop/src/App.tsx`: mount trusted prompts outside `PluginHost`.
- Modify `apps/desktop/src/app.css`: trusted prompt overlays using semantic Shell tokens.
- Modify `apps/desktop/src/__tests__/App.test.tsx` and `apps/desktop/src/__tests__/FoundationFlow.test.tsx`: backend-only descriptors and trusted prompt placement.

### Built-in plugins and tooling

- Create `plugins/provider-github/plugin.json`: backend-only GitHub contribution.
- Create `plugins/provider-gitlab/plugin.json`: backend-only GitLab contribution.
- Create `plugins/provider-center/package.json`, `plugins/provider-center/tsconfig.json`, `plugins/provider-center/vite.config.ts`, `plugins/provider-center/index.html`, and `plugins/provider-center/plugin.json`: Provider Center workspace and manifest.
- Create `plugins/provider-center/src/main.tsx`, `plugins/provider-center/src/App.tsx`, `plugins/provider-center/src/api.ts`, and `plugins/provider-center/src/style.css`: plugin entry, route, API, and token-based styles.
- Create `plugins/provider-center/src/components/InstancePanel.tsx`, `plugins/provider-center/src/components/AccountPanel.tsx`, `plugins/provider-center/src/components/RepositoryBrowser.tsx`, and `plugins/provider-center/src/components/RemoteBindings.tsx`: focused UI components.
- Create `plugins/provider-center/src/__tests__/api.test.ts`, `plugins/provider-center/src/__tests__/ProviderCenter.test.tsx`, and `plugins/provider-center/src/__tests__/RepositoryBrowser.test.tsx`: API cancellation and user-flow tests.
- Modify `scripts/sync-builtin-plugins.mjs` and `scripts/sync-builtin-plugins-lib.mjs`: build/copy UI plugins and copy backend-only manifests.
- Modify `scripts/sync-builtin-plugins.test.mjs`: exact staged resource test.
- Modify `package-lock.json`: Provider Center workspace lock entries.

### Native journey and documentation

- Modify `.github/workflows/ci.yml`: add the Provider release-boundary proof while the existing Windows/Ubuntu E2E matrix picks up the new serial spec.
- Modify `apps/desktop/src-tauri/src/e2e.rs`: deterministic Provider fixture and matching Git remote.
- Create `apps/desktop/e2e/fixture-provider.ts`: strict Provider fixture parsing and production-command cleanup.
- Create `apps/desktop/e2e/provider.e2e.ts`: Provider Center native journey.
- Modify `apps/desktop/e2e/wdio.conf.ts`: include the Provider spec.
- Modify `docs/development.md`: mock Provider tests, PAT boundary, and self-managed GitLab smoke steps.

---

### Task 1: Add Provider contracts and backend-only plugin contributions

**Files:**

- Create: `packages/contracts/src/provider.ts`
- Create: `packages/contracts/src/__fixtures__/provider-contracts.json`
- Modify: `packages/contracts/src/plugin.ts`
- Modify: `packages/contracts/src/index.ts`
- Test: `packages/contracts/src/__tests__/contracts.test.ts`

- [ ] **Step 1: Write failing contract tests**

Add imports for the new schemas and these focused cases:

```ts
it("accepts a backend-only built-in Provider contribution", () => {
  const parsed = pluginManifestSchema.parse({
    schemaVersion: 1,
    id: "git-ramus.provider.gitlab",
    name: "GitLab Provider",
    version: "0.1.0",
    publisher: "git-ramus",
    description: "GitLab API adapter.",
    kind: "builtin",
    sdkVersion: "^0.1.0",
    entrypoints: {},
    contributions: {
      navigation: [],
      providers: [{
        providerId: "gitlab",
        adapterId: "git-ramus.provider.gitlab",
        displayName: "GitLab",
        icon: "gitlab",
        instanceModes: ["cloud", "selfHosted"],
        capabilities: ["repositoryDiscovery", "customCa"]
      }]
    },
    permissions: []
  });
  expect(parsed.entrypoints.ui).toBeUndefined();
});

it("rejects external backend adapters and navigation without a UI", () => {
  expect(() => pluginManifestSchema.parse({ ...backendManifest, kind: "external" })).toThrow();
  expect(() => pluginManifestSchema.parse({
    ...backendManifest,
    contributions: {
      ...backendManifest.contributions,
      navigation: [{ id: "bad", label: "Bad", route: "/bad", icon: "x" }]
    }
  })).toThrow();
  expect(() => pluginManifestSchema.parse({
    ...externalUiManifest,
    permissions: [{ capability: "providers:manage", resources: ["providers"] }]
  })).toThrow();
});

it("parses Provider pages without accepting a secret field", () => {
  const page = providerRepositoryPageSchema.parse({
    items: [{
      providerKind: "gitlab",
      instanceId,
      repositoryId: "42",
      namespace: "group",
      name: "skill-set",
      fullName: "group/skill-set",
      webUrl: "https://gitlab.example/group/skill-set",
      httpsUrl: "https://gitlab.example/group/skill-set.git",
      sshUrl: "git@gitlab.example:group/skill-set.git",
      defaultBranch: "main",
      visibility: "private",
      archived: false,
      fork: false,
      permission: "write",
      updatedAt: "2026-07-19T00:00:00Z"
    }],
    nextCursor: null,
    hasMore: false,
    rateLimit: null
  });
  expect(page.items[0]?.fullName).toBe("group/skill-set");
  expect(() => providerAccountSummarySchema.parse({ ...accountSummary, secretRef: "leak" })).toThrow();
});
```

Define `backendManifest`, `externalUiManifest`, `instanceId`, and `accountSummary` as complete immutable fixtures in the test file; use valid UUIDs and RFC 3339 timestamps.
Create `provider-contracts.json` with top-level keys `instance`, `authorizedAccount`, `repositoryPage`, `binding`, and `error`; parse each value through its strict TypeScript schema and assert recursively that no key matches `/pat|secretRef|authorization|customCaPath/iu`.

- [ ] **Step 2: Run the contract test and verify RED**

Run:

```powershell
npm run test --workspace @git-ramus/contracts -- contracts
```

Expected: FAIL because Provider contribution and Provider DTO schemas do not exist and `entrypoints.ui` is still required.

- [ ] **Step 3: Implement the Provider schema surface**

Create `provider.ts` with strict Zod objects and inferred types. Use these exact public enums and request boundaries:

```ts
export const providerKindSchema = z.enum(["github", "gitlab"]);
export const providerConnectionStatusSchema = z.enum([
  "connected", "actionRequired", "rateLimited", "unavailable"
]);
export const providerVisibilitySchema = z.enum(["public", "internal", "private"]);
export const providerPermissionSchema = z.enum(["read", "write", "admin"]);
export const providerInstanceSchema = z.object({
  id: uuid,
  providerKind: providerKindSchema,
  displayName: z.string().min(1).max(128),
  baseUrl: httpsUrl,
  customCaConfigured: z.boolean(),
  customCaLabel: z.string().min(1).max(255).nullable(),
  providerEnabled: z.boolean(),
  status: providerConnectionStatusSchema,
  lastValidatedAt: timestamp.nullable(),
  serverVersion: z.string().min(1).max(128).nullable(),
  createdAt: timestamp,
  updatedAt: timestamp
}).strict();
export const providerAccountSummarySchema = z.object({
  id: uuid,
  instanceId: uuid,
  providerUserId: z.string().min(1).max(256),
  username: z.string().min(1).max(256),
  displayName: z.string().min(1).max(256).nullable(),
  avatarUrl: z.string().url().nullable(),
  isDefault: z.boolean(),
  status: providerConnectionStatusSchema,
  lastValidatedAt: timestamp
}).strict();
export const remoteRepositorySchema = z.object({
  providerKind: providerKindSchema,
  instanceId: uuid,
  repositoryId: z.string().min(1).max(256),
  namespace: z.string().min(1).max(1024),
  name: z.string().min(1).max(512),
  fullName: z.string().min(1).max(1536),
  webUrl: httpsUrl,
  httpsUrl,
  sshUrl: z.string().min(1).max(4096),
  defaultBranch: z.string().min(1).max(1024).nullable(),
  visibility: providerVisibilitySchema,
  archived: z.boolean(),
  fork: z.boolean(),
  permission: providerPermissionSchema,
  updatedAt: timestamp
}).strict();
```

Define the manifest contribution with closed enums:

```ts
export const providerContributionSchema = z.object({
  providerId: providerKindSchema,
  adapterId: z.string().regex(/^[a-z0-9]+(?:[.-][a-z0-9]+)+$/u),
  displayName: z.string().min(1).max(64),
  icon: z.enum(["github", "gitlab"]),
  instanceModes: z.array(z.enum(["cloud", "selfHosted"])).min(1),
  capabilities: z.array(z.enum(["repositoryDiscovery", "customCa"])).min(1)
}).strict();
```

Refine both arrays to unique values; GitHub accepts only `cloud` plus `repositoryDiscovery`, while GitLab accepts `cloud`/`selfHosted` and may add `customCa`.

Also define and export strict schemas/types for:

- `ProviderInstanceCreateRequest`: `providerKind`, `displayName`, `baseUrl`, `customCaAction: "none" | "selectFile"`.
- `ProviderInstanceUpdateRequest`: `instanceId`, `displayName`, `baseUrl`, `customCaAction: "keep" | "remove" | "selectFile"`.
- Account connect/rotate/validate/default/delete-impact/delete requests; plugin-visible connect/rotate requests contain IDs only and no PAT.
- `ProviderRepositoryQuery`: trimmed `search` up to 256 characters, nullable visibility, nullable trimmed namespace from 1 through 1,024 characters, `archived: "all" | "active" | "archived"`, `sort: "name" | "updated"`, `direction: "asc" | "desc"`, and `pageSize` from 1 through 100.
- Repository-list requests with `accountId`, query, nullable UUID cursor, and UUID `operationId`; cancellation requests contain that same `accountId` and `operationId` pair.
- `ProviderRateLimitState`, `ProviderRepositoryPage`, `ProviderBinding`, `ProviderBindingSuggestion`, `ProviderAccountDeletionImpact`, and list-response wrappers.
- `ProviderAuthorizedAccount`, containing one safe instance summary and one safe account summary, so external readers never need an unscoped instance/account listing route.
- An account-scoped binding-list request with `accountId`; it returns explicit bindings for that account plus inherited bindings only when that account is currently the instance default.
- Bind/unbind requests that use repository/remote IDs and never accept filesystem paths or PATs.

Modify `plugin.ts` so `entrypoints.ui` is optional, add a strict `providerContributionSchema`, add `providers: z.array(providerContributionSchema).default([])` to contributions, and add a final manifest refinement:

```ts
.superRefine((manifest, context) => {
  const providers = manifest.contributions.providers ?? [];
  if (providers.some(({ adapterId }) => adapterId !== manifest.id)) {
    context.addIssue({ code: "custom", message: "Provider adapterId must match plugin id" });
  }
  if (providers.length > 0 && manifest.kind !== "builtin") {
    context.addIssue({ code: "custom", message: "Provider adapters must be built in" });
  }
  if (manifest.entrypoints.ui === undefined && providers.length === 0) {
    context.addIssue({ code: "custom", message: "plugin has no entrypoint or Provider" });
  }
  if (manifest.entrypoints.ui === undefined && manifest.contributions.navigation.length > 0) {
    context.addIssue({ code: "custom", message: "navigation requires a UI entrypoint" });
  }
  if (manifest.kind !== "builtin" && manifest.permissions.some(
    ({ capability }) => capability === "providers:manage"
  )) {
    context.addIssue({ code: "custom", message: "Provider management is built-in only" });
  }
})
```

Change `pluginDescriptorSchema.uiUrl` to `z.string().regex(/^(?:git-ramus-plugin:\/\/localhost|https?:\/\/git-ramus-plugin\.localhost)\/[a-z0-9.-]+\/ui\.html$/u).nullable()` and export `provider.ts` from `index.ts`.

- [ ] **Step 4: Run focused validation and verify GREEN**

Run:

```powershell
npx prettier --write packages/contracts/src
npm run typecheck --workspace @git-ramus/contracts
npm run test --workspace @git-ramus/contracts -- contracts
```

Expected: typecheck PASS and all contract tests PASS, including rejection of secret fields and external Provider contributions.

- [ ] **Step 5: Commit the contract**

```powershell
git add -- packages/contracts/src/provider.ts packages/contracts/src/plugin.ts packages/contracts/src/index.ts packages/contracts/src/__fixtures__/provider-contracts.json packages/contracts/src/__tests__/contracts.test.ts
git commit -m "feat: add provider discovery contracts"
```

### Task 2: Load and stage backend-only built-in Provider plugins

**Files:**

- Modify: `apps/desktop/src-tauri/src/plugins/manifest.rs`
- Modify: `apps/desktop/src-tauri/src/plugins/registry.rs`
- Modify: `apps/desktop/src-tauri/src/plugins/protocol.rs`
- Create: `plugins/provider-github/plugin.json`
- Create: `plugins/provider-gitlab/plugin.json`
- Modify: `apps/desktop/src/plugins/PluginHost.tsx`
- Modify: `apps/desktop/src/plugins/__tests__/PluginFrame.test.tsx`
- Modify: `scripts/sync-builtin-plugins.mjs`
- Modify: `scripts/sync-builtin-plugins-lib.mjs`
- Test: `scripts/sync-builtin-plugins.test.mjs`

- [ ] **Step 1: Write failing Rust and staging tests**

In `registry.rs`, add a helper that writes this backend-only manifest and assert discovery succeeds with no UI:

```rust
#[test]
fn discovers_a_backend_only_builtin_provider_without_an_iframe_url() {
    let directory = tempdir().expect("temp directory creates");
    write_backend_provider(directory.path(), "git-ramus.provider.gitlab", "gitlab");
    let registry = PluginRegistry::discover(directory.path()).expect("provider discovers");
    let descriptor = registry.get("git-ramus.provider.gitlab").expect("descriptor exists");
    assert!(descriptor.ui_url.is_none());
    assert!(descriptor.ui_html.is_none());
    assert_eq!(descriptor.manifest.contributions.providers.len(), 1);
}
```

In `protocol.rs`, request `/git-ramus.provider.gitlab/ui.html` and assert `StatusCode::NOT_FOUND`.

In `sync-builtin-plugins.test.mjs`, extend the expected resources:

```js
assert.deepEqual(await sortedEntries("git-ramus.provider.github"), ["plugin.json"]);
assert.deepEqual(await sortedEntries("git-ramus.provider.gitlab"), ["plugin.json"]);
```

- [ ] **Step 2: Run tests and verify RED**

```powershell
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml plugins::
node --test scripts/sync-builtin-plugins.test.mjs
```

Expected: Rust fails because `ui` and `ui_url` are mandatory; Node fails because backend-only manifests are neither declared nor stageable.

- [ ] **Step 3: Mirror the manifest contract in Rust**

Use optional UI fields and typed Provider contributions:

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct PluginEntrypoints {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProviderContribution {
    pub provider_id: ProviderContributionId,
    pub adapter_id: String,
    pub display_name: String,
    pub icon: String,
    pub instance_modes: Vec<ProviderInstanceMode>,
    pub capabilities: Vec<ProviderCapability>,
}
```

Add `providers: Vec<ProviderContribution>` with `#[serde(default)]`. Enforce the same three refinements as the TypeScript contract and validate that `adapter_id == manifest.id`, lists are non-empty/unique, and only a built-in manifest can contribute an adapter.

Change `PluginDescriptor.ui_url` and `ui_html` to `Option<String>`. In discovery, canonicalize/read an entrypoint only inside `if let Some(ui)`. In `build_plugin_response`, return 404 when `ui_html` is `None`; never synthesize empty HTML.

Update `PluginHost` to narrow the descriptor before rendering `PluginFrame`:

```tsx
if (descriptor.uiUrl === null) {
  return (
    <section className="empty-state">
      <h2>Plugin has no user interface</h2>
      <p>This built-in plugin contributes a trusted backend capability.</p>
    </section>
  );
}
const uiDescriptor: PluginDescriptor & { uiUrl: string } = {
  ...descriptor,
  uiUrl: descriptor.uiUrl
};
return <PluginFrame descriptor={uiDescriptor} hostApi={hostApi} route={route} theme={theme} />;
```

Change `PluginFrameProps.descriptor` to `PluginDescriptor & { uiUrl: string }`. Add a component test proving a null-URL backend descriptor creates no iframe.

- [ ] **Step 4: Add exact backend manifests and safe staging**

Create the GitHub manifest:

```json
{
  "schemaVersion": 1,
  "id": "git-ramus.provider.github",
  "name": "GitHub Provider",
  "version": "0.1.0",
  "publisher": "git-ramus",
  "description": "GitHub.com repository discovery adapter.",
  "kind": "builtin",
  "sdkVersion": "^0.1.0",
  "entrypoints": {},
  "contributions": {
    "navigation": [],
    "providers": [{
      "providerId": "github",
      "adapterId": "git-ramus.provider.github",
      "displayName": "GitHub",
      "icon": "github",
      "instanceModes": ["cloud"],
      "capabilities": ["repositoryDiscovery"]
    }]
  },
  "permissions": []
}
```

Create the GitLab manifest with ID `git-ramus.provider.gitlab`, modes `cloud` and `selfHosted`, and capabilities `repositoryDiscovery` and `customCa`.

Add both entries to `sync-builtin-plugins.mjs` with `workspace: null`. Change the build callback to return immediately for a null workspace. In `stagePlugin`, always copy `plugin.json`; only require `entrypoints.ui === "ui.html"`, build output, and `dist/index.html` when `ui` exists. Reject a backend-only manifest without a non-empty Provider contribution.

- [ ] **Step 5: Verify and commit**

```powershell
npx prettier --write plugins/provider-github plugins/provider-gitlab scripts
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml plugins::
node --test scripts/sync-builtin-plugins.test.mjs
npm run typecheck --workspace @git-ramus/desktop
npm run test --workspace @git-ramus/desktop -- PluginFrame
git add -- apps/desktop/src-tauri/src/plugins apps/desktop/src/plugins/PluginHost.tsx apps/desktop/src/plugins/__tests__/PluginFrame.test.tsx plugins/provider-github plugins/provider-gitlab scripts/sync-builtin-plugins.mjs scripts/sync-builtin-plugins-lib.mjs scripts/sync-builtin-plugins.test.mjs
git commit -m "feat: register backend-only provider plugins"
```

### Task 3: Persist Provider instances, accounts, bindings, and cleanup records

**Files:**

- Create: `apps/desktop/src-tauri/migrations/0003_provider_discovery.sql`
- Modify: `apps/desktop/src-tauri/src/db/migrations.rs`
- Modify: `apps/desktop/src-tauri/src/db/mod.rs`
- Create: `apps/desktop/src-tauri/src/providers/mod.rs`
- Create: `apps/desktop/src-tauri/src/providers/model.rs`
- Create: `apps/desktop/src-tauri/src/providers/store.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`

- [ ] **Step 1: Write failing v3 migration and store tests**

Add database tests for version 3 and table names, then add `providers/store.rs` tests covering the invariants:

```rust
#[test]
fn one_default_account_and_same_instance_bindings_are_enforced() {
    let store = ProviderStore::new(Database::open_in_memory().expect("database opens"));
    let github = store.insert_instance(instance("github", "https://github.com")).unwrap();
    let gitlab = store.insert_instance(instance("gitlab", "https://gitlab.com")).unwrap();
    let first = store.insert_account(new_account(&github.id, "1")).unwrap();
    let second = store.insert_account(new_account(&github.id, "2")).unwrap();
    assert!(first.is_default);
    assert!(!second.is_default);
    store.set_default_account(&github.id, &second.id).unwrap();
    let foreign = store.insert_account(new_account(&gitlab.id, "9")).unwrap();
    seed_local_remote(store.database(), REPOSITORY_ID, "origin");
    assert!(store.upsert_binding(binding(REPOSITORY_ID, "origin", &github.id, Some(&foreign.id))).is_err());
    assert_eq!(store.list_accounts(&github.id).unwrap().len(), 2);
}

#[test]
fn deleting_a_local_remote_cascades_only_its_provider_binding() {
    let fixture = StoreFixture::new();
    fixture.bind("origin");
    fixture.bind("upstream");
    fixture.delete_remote("origin");
    assert!(fixture.store.get_binding(REPOSITORY_ID, "origin").unwrap().is_none());
    assert!(fixture.store.get_binding(REPOSITORY_ID, "upstream").unwrap().is_some());
}
```

Also test v2 upgrade preservation, first-account default assignment transaction, binding impact counts, and cleanup records refusing to delete a still-referenced SecretRef.

- [ ] **Step 2: Run focused tests and verify RED**

```powershell
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml db::
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml providers::store
```

Expected: version remains 2 and the Provider module/store do not exist.

- [ ] **Step 3: Add the transactional v3 migration**

Create the migration with these concrete tables and constraints:

```sql
BEGIN IMMEDIATE;

CREATE TABLE provider_instances (
    id TEXT PRIMARY KEY NOT NULL,
    provider_kind TEXT NOT NULL CHECK (provider_kind IN ('github','gitlab')),
    display_name TEXT NOT NULL,
    base_url TEXT NOT NULL,
    api_base_url TEXT NOT NULL,
    custom_ca_path TEXT,
    last_validated_at TEXT,
    server_version TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(provider_kind, base_url)
);

CREATE TABLE provider_accounts (
    id TEXT PRIMARY KEY NOT NULL,
    instance_id TEXT NOT NULL,
    provider_user_id TEXT NOT NULL,
    username TEXT NOT NULL,
    display_name TEXT,
    avatar_url TEXT,
    secret_ref TEXT NOT NULL UNIQUE,
    is_default INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0,1)),
    last_validated_at TEXT NOT NULL,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    UNIQUE(instance_id, provider_user_id),
    UNIQUE(id, instance_id),
    FOREIGN KEY(instance_id) REFERENCES provider_instances(id) ON DELETE RESTRICT
);
CREATE UNIQUE INDEX idx_provider_accounts_one_default
    ON provider_accounts(instance_id) WHERE is_default = 1;

CREATE TABLE provider_repository_bindings (
    repository_id TEXT NOT NULL,
    remote_name TEXT NOT NULL,
    provider_instance_id TEXT NOT NULL,
    provider_account_id TEXT,
    provider_repository_id TEXT NOT NULL,
    full_name TEXT NOT NULL,
    web_url TEXT NOT NULL,
    matched_url TEXT NOT NULL,
    binding_source TEXT NOT NULL CHECK (binding_source IN ('auto','manual')),
    bound_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    PRIMARY KEY(repository_id, remote_name),
    FOREIGN KEY(repository_id, remote_name)
      REFERENCES repository_remotes(repository_id, name) ON DELETE CASCADE,
    FOREIGN KEY(provider_instance_id)
      REFERENCES provider_instances(id) ON DELETE RESTRICT,
    FOREIGN KEY(provider_account_id, provider_instance_id)
      REFERENCES provider_accounts(id, instance_id) ON DELETE RESTRICT
);

CREATE TABLE provider_secret_cleanup (
    secret_ref TEXT PRIMARY KEY NOT NULL,
    created_at TEXT NOT NULL,
    last_attempt_at TEXT,
    attempt_count INTEGER NOT NULL DEFAULT 0 CHECK (attempt_count >= 0),
    last_error_code TEXT
);

PRAGMA user_version = 3;
COMMIT;
```

Add useful indexes on binding instance/account and account instance. Update `migrations.rs` to run `MIGRATION_3` only when the current version is below 3.

- [ ] **Step 4: Implement focused models and SQL store methods**

Define `ProviderKind`, `ProviderInstance`, `ProviderInstanceSummary`, `NewProviderAccount`, `ProviderAccount`, `ProviderAccountSummary`, `ProviderAuthorizedAccount`, `ProviderBinding`, `BindingSource`, `AccountDeletionImpact`, and `SecretCleanupRecord` with camelCase Serde output where public. `NewProviderAccount` has no caller-controlled default flag. Keep `secret_ref` on the internal `ProviderAccount` only; `ProviderAuthorizedAccount` combines safe instance/account summaries and contains no internal fields.

Implement this concrete store surface with parameterized SQL:

```rust
impl ProviderStore {
    pub fn insert_instance(&self, value: ProviderInstance) -> Result<ProviderInstance, AppError>;
    pub fn update_instance(&self, value: ProviderInstance) -> Result<ProviderInstance, AppError>;
    pub fn get_instance(&self, id: &str) -> Result<ProviderInstance, AppError>;
    pub fn list_instances(&self) -> Result<Vec<ProviderInstance>, AppError>;
    pub fn delete_instance(&self, id: &str) -> Result<(), AppError>;

    pub fn insert_account(&self, value: NewProviderAccount) -> Result<ProviderAccount, AppError>;
    pub fn update_account_secret(&self, id: &str, secret_ref: &str, validated_at: &str) -> Result<(), AppError>;
    pub fn set_default_account(&self, instance_id: &str, account_id: &str) -> Result<(), AppError>;
    pub fn get_account(&self, id: &str) -> Result<ProviderAccount, AppError>;
    pub fn list_accounts(&self, instance_id: &str) -> Result<Vec<ProviderAccount>, AppError>;
    pub fn account_deletion_impact(&self, id: &str) -> Result<AccountDeletionImpact, AppError>;
    pub fn delete_account_with_resolution(
        &self,
        id: &str,
        resolution: &AccountDeletionResolution,
        new_default_account_id: Option<&str>,
    ) -> Result<(), AppError>;

    pub fn upsert_binding(&self, value: ProviderBinding) -> Result<ProviderBinding, AppError>;
    pub fn get_binding(&self, repository_id: &str, remote_name: &str) -> Result<Option<ProviderBinding>, AppError>;
    pub fn list_bindings(&self, instance_id: Option<&str>) -> Result<Vec<ProviderBinding>, AppError>;
    pub fn delete_binding(&self, repository_id: &str, remote_name: &str) -> Result<(), AppError>;

    pub fn enqueue_secret_cleanup(&self, secret_ref: &str) -> Result<(), AppError>;
    pub fn list_secret_cleanup(&self) -> Result<Vec<SecretCleanupRecord>, AppError>;
    pub fn record_cleanup_attempt(
        &self,
        secret_ref: &str,
        succeeded: bool,
        error_code: Option<&str>,
    ) -> Result<(), AppError>;
    pub fn secret_ref_is_referenced(&self, secret_ref: &str) -> Result<bool, AppError>;
}
```

`insert_account` runs under `BEGIN IMMEDIATE`, computes `is_default` from whether the instance currently has any account, and inserts in that same transaction; concurrent first-account connects therefore cannot create two defaults or leave the first account non-default. Add a raw-SQL constraint test proving the partial unique index independently rejects a second default. `set_default_account` must clear/set inside one transaction and verify the target belongs to the instance. Map uniqueness/FK failures through `map_constraint_error` without including URLs or account names.

`delete_account_with_resolution` must reassign, inherit, or delete every explicit binding affected by the account, choose and validate a replacement default when deleting a default that still has sibling accounts, delete the account, and set `revoked_at` on exact `permission_grants` rows whose resource equals `provider-account/{account-id}` in the same SQLite transaction. It must reject a replacement account from another instance. When deleting the last account, inherited bindings remain with a missing default and surface as `actionRequired`; they are deleted only when the user explicitly selects unbind. `delete_instance` must refuse while accounts or bindings remain. These store operations do not touch the keychain; Task 8 wraps them with secret compensation.

Cleanup records store only a SecretRef, timestamps/counts, and a stable redacted error code. A successful attempt deletes the queue row; a failed attempt increments the count and updates `last_error_code`. Reject arbitrary error text at this boundary.

- [ ] **Step 5: Verify and commit**

```powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml db::
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml providers::store
git add -- apps/desktop/src-tauri/migrations/0003_provider_discovery.sql apps/desktop/src-tauri/src/db apps/desktop/src-tauri/src/providers apps/desktop/src-tauri/src/lib.rs
git commit -m "feat: persist provider discovery state"
```

### Task 4: Synchronize local Git remotes and normalize Provider URLs

**Files:**

- Modify: `apps/desktop/src-tauri/src/git/parser.rs`
- Modify: `apps/desktop/src-tauri/src/git/repository.rs`
- Modify: `apps/desktop/src-tauri/src/git/service.rs`
- Modify: `apps/desktop/src-tauri/tests/git_service_integration.rs`
- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/Cargo.lock`
- Create: `apps/desktop/src-tauri/src/providers/url.rs`
- Modify: `apps/desktop/src-tauri/src/providers/mod.rs`

- [ ] **Step 1: Write failing parser, persistence, and URL tests**

Add the exact remote-config fixture and assertions:

```rust
#[test]
fn parses_effective_fetch_and_push_urls_without_splitting_dotted_remote_names() {
    let input = b"remote.origin.url\nhttps://gitlab.example/group/repo.git\0\
remote.origin.pushurl\ngit@gitlab.example:group/fork.git\0\
remote.team.upstream.url\nssh://git@gitlab.example/group/upstream.git\0";
    let remotes = parse_remote_config("repository-id", input).unwrap();
    assert_eq!(remotes[0].name, "origin");
    assert_eq!(remotes[0].fetch_url.as_deref(), Some("https://gitlab.example/group/repo.git"));
    assert_eq!(remotes[0].push_url.as_deref(), Some("git@gitlab.example:group/fork.git"));
    assert_eq!(remotes[1].name, "team.upstream");
    assert_eq!(remotes[1].push_url, remotes[1].fetch_url);
}
```

Add URL cases for HTTPS, `ssh://`, SCP syntax, default ports, `.git`, credentials/query stripping, GitLab relative roots, and rejection of HTTP instance URLs:

```rust
assert_eq!(
    normalize_remote_url("git@GitLab.Example:group/repo.git").unwrap(),
    NormalizedRemoteUrl { host: "gitlab.example".into(), port: None, path: "group/repo".into() }
);
assert!(normalize_instance_base("http://gitlab.example", ProviderKind::Gitlab).is_err());
assert!(normalize_instance_base("https://user:pass@gitlab.example", ProviderKind::Gitlab).is_err());
```

Normalize `https://token@gitlab.example/group/repo.git?x=secret#fragment` to host/path only and assert neither the normalized `Debug` output nor its stable error envelope contains `token`, `x=secret`, or the original URL. For SSH/SCP, accept only the conventional username portion needed to parse the host and never retain it in `NormalizedRemoteUrl`.

In `git_service_integration.rs`, create `origin`, rescan, and assert `RepositoryRepository::list_remotes` contains the effective URLs.

- [ ] **Step 2: Run focused tests and verify RED**

```powershell
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml git::parser::tests::parses_effective_fetch_and_push_urls
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml providers::url
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --test git_service_integration remote
```

Expected: missing parser, URL module, and remote refresh behavior cause failure.

- [ ] **Step 3: Implement remote parsing and transactional replacement**

Add:

```rust
pub fn parse_remote_config(repository_id: &str, bytes: &[u8]) -> Result<Vec<Remote>, AppError>;
```

Reuse `parse_git_config`; identify keys by stripping `remote.` and the final `.url`/`.pushurl` suffix, not by splitting on dots. Reject empty names and control characters. Use the first URL/pushURL value Git returns and fall back from absent pushURL to fetch URL.

Add this repository method:

```rust
pub fn replace_remotes(&self, repository_id: &str, remotes: &[Remote]) -> Result<(), AppError> {
    self.db.with_transaction(|tx| {
        tx.execute("DELETE FROM repository_remotes WHERE repository_id=?1", [repository_id])?;
        for remote in remotes {
            tx.execute(
                "INSERT INTO repository_remotes(repository_id,name,fetch_url,push_url) VALUES(?1,?2,?3,?4)",
                params![remote.repository_id, remote.name, remote.fetch_url, remote.push_url],
            )?;
        }
        Ok(())
    })
}
```

During repository refresh, run this bounded argument-array command and replace rows only after successful parsing:

```rust
git config --local --null --get-regexp ^remote\..*\.(url|pushurl)$
```

Git exits with code 1 when there are no matching keys; treat that as an empty remote set. Other failures preserve the previous rows and do not make status refresh fail.

- [ ] **Step 4: Implement strict instance/remote normalization**

Add `url = "2.5.8"` as a direct Cargo dependency before importing `url::Url`; do not rely on a transitive Tauri dependency.

In `providers/url.rs`, define:

```rust
pub struct NormalizedRemoteUrl { pub host: String, pub port: Option<u16>, pub path: String }
pub struct NormalizedInstance { pub base_url: String, pub api_base_url: String, pub host: String, pub root_path: String }
pub fn normalize_instance_base(input: &str, kind: ProviderKind) -> Result<NormalizedInstance, AppError>;
pub fn normalize_remote_url(input: &str) -> Result<NormalizedRemoteUrl, AppError>;
pub fn detect_remote(instance: &NormalizedInstance, remote: &NormalizedRemoteUrl) -> Option<RemoteRepositoryIdentity>;
```

For GitHub, require exactly `https://github.com` and derive `https://api.github.com`. For GitLab, require HTTPS, preserve a relative installation root, append `/api/v4`, reject credentials/query/fragment/control characters, lowercase only the host, remove default ports, and preserve repository-path case. `detect_remote` removes the GitLab instance root before producing namespace/path.

- [ ] **Step 5: Verify and commit**

```powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml git::parser
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml providers::url
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --test git_service_integration remote
git add -- apps/desktop/src-tauri/Cargo.toml apps/desktop/src-tauri/Cargo.lock apps/desktop/src-tauri/src/git apps/desktop/src-tauri/src/providers/url.rs apps/desktop/src-tauri/src/providers/mod.rs apps/desktop/src-tauri/tests/git_service_integration.rs
git commit -m "feat: synchronize and normalize repository remotes"
```

### Task 5: Build the bounded HTTP/TLS, cursor, cancellation, and error foundation

**Files:**

- Modify: `apps/desktop/src-tauri/Cargo.toml`
- Modify: `apps/desktop/src-tauri/Cargo.lock`
- Modify: `apps/desktop/src-tauri/src/secrets.rs`
- Modify: `apps/desktop/src-tauri/src/error.rs`
- Modify: `apps/desktop/src-tauri/src/providers/model.rs`
- Create: `apps/desktop/src-tauri/src/providers/http.rs`
- Create: `apps/desktop/src-tauri/src/providers/cursor.rs`
- Create: `apps/desktop/src-tauri/src/providers/adapter.rs`
- Modify: `apps/desktop/src-tauri/src/providers/mod.rs`
- Create: `apps/desktop/src-tauri/tests/provider_integration.rs`

- [ ] **Step 1: Write failing security and lifecycle tests**

Add unit tests with these assertions:

```rust
#[test]
fn sensitive_strings_never_debug_their_contents() {
    let value = SensitiveString::new("glpat-super-secret".to_owned());
    assert_eq!(format!("{value:?}"), "SensitiveString([REDACTED])");
}

#[test]
fn cursor_is_one_use_and_bound_to_plugin_provider_instance_account_and_query() {
    let store = CursorStore::default();
    let cursor = store.insert(cursor_entry(
        "plugin-a", ProviderKind::Gitlab, INSTANCE_ID, ACCOUNT_ID, query("skill")
    ));
    assert!(store.take(&cursor, "plugin-b", ProviderKind::Gitlab, INSTANCE_ID, ACCOUNT_ID, &query("skill")).is_err());
    assert!(store.take(&cursor, "plugin-a", ProviderKind::Github, INSTANCE_ID, ACCOUNT_ID, &query("skill")).is_err());
    assert!(store.take(&cursor, "plugin-a", ProviderKind::Gitlab, OTHER_INSTANCE_ID, ACCOUNT_ID, &query("skill")).is_err());
    assert!(store.take(&cursor, "plugin-a", ProviderKind::Gitlab, INSTANCE_ID, ACCOUNT_ID, &query("other")).is_err());
    assert!(store.take(&cursor, "plugin-a", ProviderKind::Gitlab, INSTANCE_ID, ACCOUNT_ID, &query("skill")).is_ok());
    assert!(store.take(&cursor, "plugin-a", ProviderKind::Gitlab, INSTANCE_ID, ACCOUNT_ID, &query("skill")).is_err());
}

#[tokio::test]
async fn operation_cancel_only_cancels_the_owning_plugin_request() {
    let registry = OperationRegistry::default();
    let guard = registry.start("plugin-a", ACCOUNT_ID, OPERATION_ID).unwrap();
    assert!(!registry.cancel("plugin-b", ACCOUNT_ID, OPERATION_ID));
    assert!(!registry.cancel("plugin-a", OTHER_ACCOUNT_ID, OPERATION_ID));
    assert!(registry.cancel("plugin-a", ACCOUNT_ID, OPERATION_ID));
    guard.token().cancelled().await;
}

#[test]
fn provider_failures_serialize_without_tokens_or_urls() {
    let error = ErrorEnvelope::from(AppError::Provider(ProviderFailure::authentication()));
    let json = serde_json::to_string(&error).unwrap();
    assert_eq!(error.code, "provider.authentication-required");
    assert!(!json.contains("glpat-"));
    assert!(!json.contains("gitlab.example/private"));
}
```

In `provider_integration.rs`, add async tests proving that a same-origin redirect is followed, a cross-origin redirect is not followed and receives no authorization header, a body over the configured maximum fails before unbounded accumulation, a delayed response respects total timeout/cancellation, a missing CA file fails before network access, a self-signed server fails without its CA, and the same server succeeds with its CA. Add a scripted server that returns 503 then 200 and assert exactly two calls, a server that remains 503 and assert exactly three calls followed by `provider.instance-unreachable`, plus a 429 response with `Retry-After` and assert exactly one call.

- [ ] **Step 2: Run focused tests and verify RED**

```powershell
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml secrets::tests::sensitive_strings_never_debug
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml providers::cursor
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml error::tests::provider_failures
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --test provider_integration scoped_http
```

Expected: the sensitive type, Provider error, cursor/operation registries, and HTTP client are absent.

- [ ] **Step 3: Add pinned dependencies and redacted Provider failures**

Add direct dependencies already compatible with Rust 1.88:

```toml
futures-util = "0.3.32"
reqwest = { version = "0.13.4", features = ["json", "query", "stream"] }
tokio-util = { version = "0.7.18", features = ["rt"] }
zeroize = "1.9.0"
```

Extend Tokio features with `io-util`, `net`, and `sync`. Add test-only dependencies:

```toml
httpmock = "0.8.3"
rcgen = "0.14.8"
tokio-rustls = "0.26.4"
```

Implement a `SensitiveString(String)` that exposes `as_str()` only to trusted Rust code, implements a constant redacted `Debug`, and calls `Zeroize::zeroize` in `Drop`. Do not derive `Serialize`, `Clone`, or `Deserialize` for it.

Add `AppError::Provider(ProviderFailure)` where `ProviderFailure` stores only stable code/category, retry metadata, recovery actions, optional plugin ID/operation UUID context, resource ID, and failed step. Define constructors for:

```rust
ProviderFailure::authentication()
ProviderFailure::permission()
ProviderFailure::rate_limited(retry_after_ms)
ProviderFailure::unreachable(retryable)
ProviderFailure::tls()
ProviderFailure::invalid_cursor()
ProviderFailure::partial()
ProviderFailure::invalid_response()
ProviderFailure::canceled()
ProviderFailure::busy(retry_after_ms)
```

Add `with_request_context(plugin_id, operation_id)` for the service layer; repository list/match/cancel failures must populate both existing `ErrorEnvelope` fields. Map constructors to the exact `provider.*` codes in the specification plus `provider.request-canceled` and retryable `provider.request-busy`; never retain a `reqwest::Error`, response body, URL, token, or header inside `ProviderFailure`.

- [ ] **Step 4: Implement the scoped streaming HTTP client**

Define a client that accepts only relative API paths and structured query pairs:

```rust
pub struct ScopedHttpClient {
    origin: reqwest::Url,
    client: reqwest::Client,
    max_body_bytes: usize,
}

impl ScopedHttpClient {
    pub fn build(instance: &ProviderInstance) -> Result<Self, AppError>;
    pub async fn get(
        &self,
        relative_path: &str,
        query: &[(&str, String)],
        headers: reqwest::header::HeaderMap,
        cancellation: &CancellationToken,
    ) -> Result<BoundedResponse, AppError>;
}
```

`build` must:

- Parse only the stored `api_base_url`.
- Use reqwest's platform verifier and system proxy defaults.
- Merge, not replace, a PEM/DER extra CA selected for the instance.
- Reject missing/non-file/over-1-MiB CA paths before creating a client.
- Apply 10-second connect, 30-second read, and 45-second total timeouts.
- Use `Policy::custom` with at most five redirects and same scheme/host/effective-port checks.

Keep production limits in an immutable `HttpLimits::production()` value. A crate-private `build_with_limits` exists only under `cfg(test)` so timeout, body-size, and retry-delay cases run in milliseconds without weakening production defaults.

`get` must reject absolute/scheme-relative paths, re-check the joined URL origin, and use `tokio::select!` with the cancellation token. Read `bytes_stream()` chunk by chunk and stop before exceeding 2 MiB. Parse status and safe headers into `BoundedResponse`; never include the request URL or response body in errors.

Retry only idempotent GET attempts that fail with a transient transport error or HTTP 502/503/504. Allow at most two retries (three attempts total), with bounded 100 ms then 250 ms delays plus 0–25 ms jitter derived from `SystemTime`; cancellation must interrupt both request and backoff. Never auto-retry 401/403/404/429, TLS failures, schema failures, redirects rejected by policy, or an oversized body. Return parsed `Retry-After` metadata for 429 instead of sleeping. Keep a test-only zero-delay policy so retry-count tests remain deterministic.

- [ ] **Step 5: Implement one-use cursors, cancellation, and the adapter contract**

Use bounded host memory only:

```rust
pub struct CursorEntry {
    pub plugin_id: String,
    pub provider_kind: ProviderKind,
    pub instance_id: String,
    pub account_id: String,
    pub query: ProviderRepositoryQuery,
    pub adapter_cursor: Option<AdapterCursor>,
    pub buffered: Vec<RemoteRepository>,
    pub expires_at: std::time::Instant,
}

pub struct OperationGuard {
    key: (String, String, String),
    token: CancellationToken,
    registry: OperationRegistry,
}
```

Generate cursor keys with UUID v4. Cap cursors at 512 entries, purge expired entries before insert, evict the entry with the nearest expiration when still full, expire entries after ten minutes, remove them on successful `take`, and reject mismatched plugin/provider/instance/account/query. The service derives provider and instance from the current stored account rather than trusting cursor input from the plugin.

`OperationRegistry::start` rejects duplicate `(pluginId, accountId, operationId)` triples and rejects admission above 1,024 global or 64 operations for one account with a stable retryable busy failure. Cancel requires the same triple, and `Drop` removes a completed operation and notifies idle waiters. Add `cancel_for_plugin_account`, `cancel_for_account`, `wait_for_plugin_account_idle`, and `wait_for_account_idle`; waits accept a five-second deadline and return `provider.request-busy` rather than hanging revocation/deletion if an adapter violates cancellation. Tests fill both limits and prove rejected operations never allocate a cancellation token retained by the registry.

Define the object-safe adapter interface with boxed futures:

```rust
pub struct AdapterAccountContext<'a> {
    pub client: &'a ScopedHttpClient,
    pub secret: &'a str,
    pub cancellation: &'a CancellationToken,
}

pub trait RepositoryDiscoveryProvider: Send + Sync {
    fn kind(&self) -> ProviderKind;
    fn validate_instance<'a>(&'a self, client: &'a ScopedHttpClient)
        -> BoxFuture<'a, Result<InstanceMetadata, AppError>>;
    fn authenticate_account<'a>(&'a self, client: &'a ScopedHttpClient, secret: &'a str)
        -> BoxFuture<'a, Result<AccountIdentity, AppError>>;
    fn list_repositories<'a>(&'a self, context: AdapterAccountContext<'a>, request: AdapterListRequest)
        -> BoxFuture<'a, Result<AdapterPage, AppError>>;
    fn get_repository<'a>(&'a self, context: AdapterAccountContext<'a>, identity: RemoteRepositoryIdentity)
        -> BoxFuture<'a, Result<RemoteRepository, AppError>>;
    fn detect_remote(&self, instance: &NormalizedInstance, remote: &NormalizedRemoteUrl)
        -> Option<RemoteRepositoryIdentity>;
}
```

- [ ] **Step 6: Add a real self-signed TLS integration helper and verify GREEN**

In `provider_integration.rs`, generate a localhost certificate with `rcgen::generate_simple_self_signed`, serve one bounded HTTP response through `tokio_rustls::TlsAcceptor`, write the CA PEM to a `tempfile`, and build two `ProviderInstance` values: one without `custom_ca_path`, one with it. Assert the first returns `provider.tls-failed` and the second returns status 200/body JSON. Keep the test server to one accepted connection per assertion so it cannot hang the suite.

Run:

```powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml secrets::
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml providers::cursor
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml error::
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --test provider_integration scoped_http
cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets -- -D warnings
```

Expected: all focused tests and Clippy PASS; cross-origin mock receives zero authenticated calls, oversized input is rejected, and custom CA succeeds without a skip-verification option.

- [ ] **Step 7: Commit the Provider foundation**

```powershell
git add -- apps/desktop/src-tauri/Cargo.toml apps/desktop/src-tauri/Cargo.lock apps/desktop/src-tauri/src/secrets.rs apps/desktop/src-tauri/src/error.rs apps/desktop/src-tauri/src/providers apps/desktop/src-tauri/tests/provider_integration.rs
git commit -m "feat: add secure provider http foundation"
```

### Task 6: Implement the GitHub repository-discovery adapter

**Files:**

- Create: `apps/desktop/src-tauri/src/providers/github.rs`
- Modify: `apps/desktop/src-tauri/src/providers/mod.rs`
- Modify: `apps/desktop/src-tauri/tests/provider_integration.rs`

- [ ] **Step 1: Write failing GitHub adapter tests**

Use `httpmock::MockServer::start_async()` and `ScopedHttpClient::for_test_http`. Mount `/user` and `/user/repos` with exact headers:

```rust
when.method(GET)
    .path("/user")
    .header("authorization", "Bearer github-test-token")
    .header("accept", "application/vnd.github+json")
    .header("x-github-api-version", "2026-03-10");
```

Return minimal complete account/repository fixtures and assert:

```rust
assert_eq!(identity.provider_user_id, "7");
assert_eq!(page.items[0].full_name, "octo/private-skill");
assert_eq!(page.items[0].visibility, ProviderVisibility::Private);
assert_eq!(page.items[0].permission, ProviderPermission::Write);
assert_eq!(page.next_cursor, Some(AdapterCursor::Page(2)));
assert_eq!(page.rate_limit.unwrap().remaining, Some(4999));
```

Add account-affiliation fixtures for an owner repository, an organization-member private repository, an archived repository, and a fork. Mount `/search/repositories`, assert it receives zero calls, and prove an unrelated public fixture never enters results. Add tests for `/repos/octo/private-skill`, a 401 mapping to authentication-required, a 403 with `x-ratelimit-remaining: 0` mapping to rate-limited, a 403 without exhaustion mapping to permission-insufficient, a privacy-preserving 404 mapping to permission-insufficient during verification, and response JSON containing unknown extra fields.

- [ ] **Step 2: Run the tests and verify RED**

```powershell
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --test provider_integration github_
```

Expected: FAIL because `GithubProvider` does not exist.

- [ ] **Step 3: Implement exact GitHub API mapping**

Add a zero-sized `GithubProvider`. Build every request with:

```rust
headers.insert(AUTHORIZATION, HeaderValue::from_str(&format!("Bearer {secret}"))?);
headers.insert(ACCEPT, HeaderValue::from_static("application/vnd.github+json"));
headers.insert("x-github-api-version", HeaderValue::from_static("2026-03-10"));
headers.insert(USER_AGENT, HeaderValue::from_static("Git-Ramus/0.1"));
```

Use:

- `GET /user` for authentication.
- `GET /user/repos` with `affiliation=owner,collaborator,organization_member`, `visibility`, `sort`, `direction`, `per_page=100`, and the current page.
- `GET /repositories/{id}` when identity has an external ID; use `GET /repos/{owner}/{repo}` for a path identity.
- Rate data from `x-ratelimit-limit`, `x-ratelimit-remaining`, and `x-ratelimit-reset`.
- Same-origin `Link rel="next"` parsing that extracts only a positive page number and rejects another origin.

Map host sort `name` to GitHub `full_name` and `updated` to `updated`; map nullable visibility to `all`. The host still reapplies every filter after mapping.

Deserialize into private response structs with only the fields required by `RemoteRepository`; ignore unknown response fields. Convert integer IDs to strings before leaving the adapter. Never deserialize or retain `temp_clone_token`.

Map a verification 404 to the same `provider.permission-insufficient` envelope as an inaccessible repository so the caller cannot distinguish private existence. Let the HTTP foundation exhaust bounded 5xx retries before mapping to `provider.instance-unreachable`.

- [ ] **Step 4: Verify and commit**

```powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --test provider_integration github_
cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets -- -D warnings
git add -- apps/desktop/src-tauri/src/providers/github.rs apps/desktop/src-tauri/src/providers/mod.rs apps/desktop/src-tauri/tests/provider_integration.rs
git commit -m "feat: discover github repositories"
```

### Task 7: Implement the GitLab.com and self-managed GitLab adapter

**Files:**

- Create: `apps/desktop/src-tauri/src/providers/gitlab.rs`
- Modify: `apps/desktop/src-tauri/src/providers/mod.rs`
- Modify: `apps/desktop/src-tauri/tests/provider_integration.rs`

- [ ] **Step 1: Write failing GitLab adapter tests**

Mount a mock under a relative installation root such as `/gitlab/api/v4`. Require `PRIVATE-TOKEN: gitlab-test-token` on authenticated requests. Return `/user` and two paginated `/projects` responses with `X-Next-Page`, `RateLimit-*`, and these fields:

```json
{
  "id": 42,
  "name": "Skill Set",
  "path": "skill-set",
  "path_with_namespace": "group/subgroup/skill-set",
  "default_branch": "main",
  "visibility": "internal",
  "ssh_url_to_repo": "git@gitlab.example:group/subgroup/skill-set.git",
  "http_url_to_repo": "https://gitlab.example/group/subgroup/skill-set.git",
  "web_url": "https://gitlab.example/group/subgroup/skill-set",
  "archived": false,
  "forked_from_project": null,
  "permissions": { "project_access": { "access_level": 30 }, "group_access": null },
  "last_activity_at": "2026-07-19T00:00:00Z"
}
```

Assert the request includes `membership=true`, `simple=true`, `per_page=100`, page/sort fields, and a server-side `search` term when supplied. Reject any `/projects` call lacking `membership=true` and prove an unrelated public fixture never enters results. Use personal, nested-group private/internal, archived, and fork fixtures; assert visibility, effective permission, pagination, and relative-root URLs map correctly. Add 401, 403, privacy-preserving 404, 429+Retry-After, malformed JSON, and cross-origin Link tests.

- [ ] **Step 2: Run the tests and verify RED**

```powershell
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --test provider_integration gitlab_
```

Expected: FAIL because `GitlabProvider` does not exist.

- [ ] **Step 3: Implement exact GitLab REST v4 mapping**

Use `PRIVATE-TOKEN` for PAT authentication and `Accept: application/json`. Implement:

- Unauthenticated `GET /version` for instance reachability; accept 200 with a bounded version string or 401/403 as a reachable API with unknown version.
- `GET /user` for account identity.
- `GET /projects?membership=true&simple=true` for account-affiliated projects.
- `GET /projects/{percent-encoded-id-or-path}` for verification.
- Offset pagination from same-origin `Link`, falling back to `X-Next-Page`; never generate a URL from an untrusted header.
- Rate state from `RateLimit-Limit`, `RateLimit-Remaining`, `RateLimit-Reset`, and `Retry-After`.
- Effective permission as the greater of project/group access: `<30 => read`, `30..39 => write`, `>=40 => admin`.

Map host sort `name` to GitLab `name` and `updated` to `last_activity_at`; pass direction as `sort=asc|desc`. Visibility and archive filtering remain host-enforced even when the API also accepts a compatible filter.

Keep `membership=true` even when `search` is present so global unrelated public projects never enter results. Preserve `internal` visibility and map `forked_from_project != null` to `fork: true`.

Map a project-verification 404 to `provider.permission-insufficient` without echoing the path, and let exhausted 5xx retries surface as `provider.instance-unreachable`.

- [ ] **Step 4: Verify and commit**

```powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --test provider_integration gitlab_
cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets -- -D warnings
git add -- apps/desktop/src-tauri/src/providers/gitlab.rs apps/desktop/src-tauri/src/providers/mod.rs apps/desktop/src-tauri/tests/provider_integration.rs
git commit -m "feat: discover gitlab repositories"
```

### Task 8: Orchestrate instance and PAT-account lifecycle with compensation

**Files:**

- Modify: `apps/desktop/src-tauri/src/providers/adapter.rs`
- Create: `apps/desktop/src-tauri/src/providers/service.rs`
- Modify: `apps/desktop/src-tauri/src/providers/mod.rs`
- Modify: `apps/desktop/src-tauri/src/secrets.rs`
- Modify: `apps/desktop/src-tauri/src/app_state.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/tests/provider_integration.rs`

- [ ] **Step 1: Write failing registry and account-lifecycle tests**

Create a deterministic `FakeProvider` and a scripted `SecretStore`. Cover these exact behaviors:

```rust
#[tokio::test]
async fn connecting_the_first_account_uses_a_random_secret_ref_and_sets_default() {
    let fixture = ServiceFixture::gitlab();
    let instance = fixture.create_instance().await.unwrap();
    let account = fixture.service.connect_account(&instance.id, SensitiveString::new("token-a".into())).await.unwrap();
    assert!(account.is_default);
    assert!(!fixture.serialized_database().contains("token-a"));
    assert_eq!(fixture.secrets.values().len(), 1);
    assert!(!fixture.secrets.keys()[0].contains(&account.username));
}

#[tokio::test]
async fn failed_account_insert_deletes_the_new_secret_or_queues_cleanup() {
    let fixture = ServiceFixture::with_duplicate_identity_and_delete_failure();
    let error = fixture.service.connect_account(&fixture.instance_id, SensitiveString::new("token-b".into())).await.unwrap_err();
    assert!(matches!(error, AppError::InvalidInput(_)));
    assert_eq!(fixture.store.list_secret_cleanup().unwrap().len(), 1);
}

#[tokio::test]
async fn rotation_rejects_a_token_for_another_provider_user() {
    let fixture = ServiceFixture::connected();
    fixture.adapter.set_next_user("different-user");
    assert!(fixture.service.rotate_account(&fixture.account_id, SensitiveString::new("other".into())).await.is_err());
    assert_eq!(fixture.current_secret(), "original-token");
}
```

Also test GitHub fixed URLs, HTTPS GitLab normalization, first/second/default account behavior, custom CA label exposure without full path, and deletion-impact rules. A scripted keychain-delete failure must leave the account, grants, and bindings untouched; a later SQLite failure must restore the just-deleted keychain value. Test startup cleanup skipping referenced secrets.
Toggle the built-in Provider installation's `enabled` column off and back on in the fixture: calls while disabled must return the stable disabled state without deleting rows, and the next call after re-enable must succeed without recreating the instance/account.
Add a file-backed restart test: create an instance, two account summaries, and one binding; drop/reopen the database and reconstruct `ProviderService` with the same scripted secret store; assert all summaries/bindings remain and `validate_account` reads the PAT through its SecretRef rather than SQLite.

- [ ] **Step 2: Run focused tests and verify RED**

```powershell
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml providers::service::tests::account_
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --test provider_integration service_
```

Expected: FAIL because the adapter registry and Provider service do not exist.

- [ ] **Step 3: Gate the compiled built-in adapters by enabled state**

Implement `ProviderAdapterRegistry::from_plugins(database, plugin_registry)` so it:

1. Reads the enabled bit from `plugin_installations`.
2. Accepts only the exact built-in contribution pairs `github/git-ramus.provider.github` and `gitlab/git-ramus.provider.gitlab`.
3. Constructs at most one `Arc<dyn RepositoryDiscoveryProvider>` per supported Provider kind and gates every lookup through the current enabled bit.
4. Rejects duplicate or mismatched contributions.
5. Reports a disabled/missing Provider without deleting persisted domain rows.

Expose `get(kind)` and `is_enabled(kind)`. Keep only the two compile-time adapter constructors in memory, but re-read the installation's enabled flag before returning an adapter so an external plugin-management slice can disable/re-enable it without reconstructing domain state. Do not allow runtime code or a manifest path to construct an arbitrary adapter.

- [ ] **Step 4: Implement instance creation/update/validation**

Construct `ProviderService` from `ProviderStore`, `Arc<dyn SecretStore>`, adapter registry, cursor/operation registries, and an in-memory health map.

```rust
pub async fn create_instance(&self, input: CreateInstanceInput) -> Result<ProviderInstanceSummary, AppError>;
pub async fn update_instance(&self, input: UpdateInstanceInput) -> Result<ProviderInstanceSummary, AppError>;
pub async fn validate_instance(&self, instance_id: &str) -> Result<ProviderInstanceSummary, AppError>;
pub fn list_instances(&self) -> Result<Vec<ProviderInstanceSummary>, AppError>;
pub fn delete_instance(&self, instance_id: &str) -> Result<(), AppError>;
```

Canonicalize a selected CA path, expose only `file_name()` as `customCaLabel`, build the scoped client, call the adapter's validation, and persist only after successful TLS/API validation. Updating/removing CA must validate the replacement configuration before committing it.

- [ ] **Step 5: Implement account connect, rotate, default, validation, and deletion**

Add the concrete lifecycle surface:

```rust
pub fn list_accounts(&self, instance_id: &str) -> Result<Vec<ProviderAccountSummary>, AppError>;
pub async fn connect_account(&self, instance_id: &str, pat: SensitiveString) -> Result<ProviderAccountSummary, AppError>;
pub async fn rotate_account(&self, account_id: &str, pat: SensitiveString) -> Result<ProviderAccountSummary, AppError>;
pub async fn validate_account(&self, account_id: &str) -> Result<ProviderAccountSummary, AppError>;
pub fn set_default_account(&self, instance_id: &str, account_id: &str) -> Result<ProviderAccountSummary, AppError>;
pub fn account_deletion_impact(&self, account_id: &str) -> Result<AccountDeletionImpact, AppError>;
pub async fn delete_account(&self, input: DeleteAccountInput) -> Result<(), AppError>;
pub async fn cancel_plugin_account_operations(&self, plugin_id: &str, account_id: &str) -> Result<(), AppError>;
```

Use a fresh `provider/account/{account-id}/{secret-id}` key for every connect/rotation. The connect sequence is exactly:

```rust
self.secrets.set(&new_ref, pat.as_str())?;
let identity = match adapter.authenticate_account(&client, pat.as_str()).await {
    Ok(identity) => identity,
    Err(error) => { self.compensate_new_secret(&new_ref); return Err(error); }
};
match self.store.insert_account(account_from(identity, new_ref.clone())) {
    Ok(account) => Ok(self.account_summary(account)),
    Err(error) => { self.compensate_new_secret(&new_ref); Err(error) }
}
```

Rotation validates the same `provider_user_id`, commits the new SecretRef, then deletes/queues the old one. Deletion first cancels all operations for the account and awaits registry idleness, then reads the old value into `SensitiveString`, deletes the keyring entry, applies the requested reassign/inherit/unbind resolution and account deletion in one SQLite transaction, and restores the keyring value if that transaction fails. Revoke all `provider-account/{account-id}` grants inside the same database transaction.

At startup, retry `provider_secret_cleanup` records only after checking `secret_ref_is_referenced == false`.

- [ ] **Step 6: Wire AppState and verify GREEN**

Build the secret store before `ProviderService`, then add `pub providers: ProviderService` to `AppState`. Production uses `KeyringSecretStore`; debug E2E will be switched to memory in Task 14. Call safe cleanup retry after construction and before returning state.

Run:

```powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml providers::service
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --test provider_integration service_
cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets -- -D warnings
```

Expected: all lifecycle/compensation tests PASS and serialized database/error payloads contain no test PAT.

- [ ] **Step 7: Commit account lifecycle**

```powershell
git add -- apps/desktop/src-tauri/src/providers apps/desktop/src-tauri/src/secrets.rs apps/desktop/src-tauri/src/app_state.rs apps/desktop/src-tauri/src/lib.rs apps/desktop/src-tauri/tests/provider_integration.rs
git commit -m "feat: manage provider instances and accounts"
```

### Task 9: Add repository paging, cancellation, remote matching, and bindings

**Files:**

- Modify: `apps/desktop/src-tauri/src/providers/cursor.rs`
- Modify: `apps/desktop/src-tauri/src/providers/service.rs`
- Modify: `apps/desktop/src-tauri/src/providers/store.rs`
- Modify: `apps/desktop/src-tauri/src/providers/model.rs`
- Modify: `apps/desktop/src-tauri/tests/provider_integration.rs`

- [ ] **Step 1: Write failing discovery and matching tests**

Use a fake adapter with three upstream pages. The first contains only filtered-out repositories, the second contains two matches, and the third contains another match. Assert one UI page is filled across upstream pages and its cursor carries the remainder:

```rust
let first = service.list_repositories("git-ramus.provider-center", request(ACCOUNT_ID, query("skill", 2))).await.unwrap();
assert_eq!(names(&first.items), ["group/skill-a", "group/skill-b"]);
assert!(first.has_more);
let second = service.list_repositories("git-ramus.provider-center", request_with_cursor(ACCOUNT_ID, query("skill", 2), first.next_cursor.unwrap())).await.unwrap();
assert_eq!(names(&second.items), ["group/skill-c"]);
assert!(!second.has_more);
```

Add tests that:

- The same cursor fails for another plugin/account/query and on replay.
- `cancel_operation(pluginId, accountId, operationId)` stops only that account-scoped in-flight adapter request and returns `provider.request-canceled`.
- Discovery and matching share a per-account concurrency limit of four; a fifth request waits for a permit, and canceling it while queued returns `provider.request-canceled` without calling the adapter.
- An account-level rate limit updates health and carries `retryAfterMs`.
- A remote whose repository was never in the loaded list still matches through `detect_remote` + `get_repository`.
- HTTPS and SSH URLs matching the same repository produce one suggestion.
- Different fetch/push repositories, overlapping instances, or multiple candidates produce `ambiguous` without a write.
- `bind_remote` re-reads the current remote, derives `matched_url` in Rust, and never changes `.git/config`.
- An explicit account must belong to the binding instance; null inherits the default.
- Account-scoped binding listing includes that account's explicit bindings and its current inherited bindings, but never another account's explicit binding.
- Switching the instance default moves only inherited bindings into the new default account's view; explicit bindings remain attached to their selected account.

- [ ] **Step 2: Run focused tests and verify RED**

```powershell
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml providers::service::tests::discovery_
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml providers::service::tests::matching_
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --test provider_integration binding_
```

Expected: list, cancellation, matching, and binding methods are absent.

- [ ] **Step 3: Implement consistent filtering and opaque pagination**

Add:

```rust
pub async fn list_repositories(
    &self,
    plugin_id: &str,
    input: ListRepositoriesInput,
) -> Result<ProviderRepositoryPage, AppError>;
pub fn cancel_operation(
    &self,
    plugin_id: &str,
    account_id: &str,
    operation_id: &str,
) -> Result<(), AppError>;
```

Register the account-scoped operation before reading the secret or waiting for concurrency. Maintain a lazily-created `Arc<Semaphore>` per account with four permits; acquire it with `tokio::select!` against the operation token. Restore a prior cursor entry only after verifying plugin/account/query. Request upstream pages until the output has `query.page_size` matching items or no next adapter page remains. Reapply filters in the host even when an adapter used server filtering: Unicode-lowercase substring match over `full_name` for non-empty search, Unicode-lowercase exact match for namespace, exact visibility, and exact archived state. Preserve adapter order supplied by the mapped upstream sort, store unused matching items in a new one-use cursor, and keep the cursor only in memory.

On an adapter failure while consuming a continuation cursor, return `provider.partial-result` with the failed step and safe recovery action; the Provider Center keeps already-rendered items. On the initial page, preserve the specific normalized failure instead. Update account health to rate-limited/action-required/unavailable without persisting raw error text.

- [ ] **Step 4: Implement matching and binding from current local state**

Add:

```rust
pub async fn match_local_remotes(
    &self,
    plugin_id: &str,
    instance_id: &str,
    account_id: &str,
    operation_id: &str,
) -> Result<Vec<ProviderBindingSuggestion>, AppError>;
pub async fn bind_remote(&self, input: BindRemoteInput) -> Result<ProviderBinding, AppError>;
pub fn list_bindings_for_account(&self, account_id: &str) -> Result<Vec<ProviderBinding>, AppError>;
pub fn unbind_remote(&self, repository_id: &str, remote_name: &str) -> Result<(), AppError>;
```

For each stored local remote, normalize effective fetch/push URLs, retain only candidates under the selected instance, deduplicate identities, and call `get_repository` for verification. The caller always supplies the account used for authorization and verification; the UI may pass the instance's current default account explicitly, while a later binding can still persist `provider_account_id = NULL` to inherit. Return `suggested`, `ambiguous`, `unverified`, or `none`. A suggestion is never persisted automatically.

`bind_remote` must re-read `repository_remotes`, re-run normalization, resolve an inherited binding to the current default account, and asynchronously call the selected adapter's `get_repository` before persistence. Verify Provider repository/instance/account consistency and derive the sanitized matched URL in Rust. Store `binding_source=auto` only when the chosen stable repository ID appears in a current verified suggestion; otherwise store `manual`. Do not call Git or write local config.

`list_bindings_for_account` returns explicit bindings for that account plus inherited bindings only when the account is the current instance default. It never returns another explicit account's binding, even to a caller that guesses the binding's local repository ID.

- [ ] **Step 5: Verify and commit**

```powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml providers::service
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --test provider_integration binding_
cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets -- -D warnings
git add -- apps/desktop/src-tauri/src/providers apps/desktop/src-tauri/tests/provider_integration.rs
git commit -m "feat: discover and bind provider repositories"
```

### Task 10: Expose typed Rust commands and account-scoped permission grants

**Files:**

- Modify: `apps/desktop/src-tauri/src/plugins/permissions.rs`
- Modify: `apps/desktop/src-tauri/src/plugins/registry.rs`
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/error.rs`
- Test: `apps/desktop/src-tauri/src/plugins/permissions.rs`
- Test: `apps/desktop/src-tauri/src/commands.rs`

- [ ] **Step 1: Write failing dynamic-grant and command tests**

Extend permission tests with one built-in Provider Center and one external test manifest:

```rust
#[test]
fn an_external_provider_request_is_not_a_grant_until_the_user_selects_an_account() {
    let fixture = PermissionFixture::external_provider_reader();
    assert!(fixture.registry.manifest_requests(
        "example.reader", "providers:read", "providers"
    ));
    assert!(!fixture.gateway.is_allowed(
        "example.reader", "providers:read", &format!("provider-account/{ACCOUNT_ID}")
    ).unwrap());
    fixture.gateway.grant_dynamic(
        "example.reader", "providers:read", &format!("provider-account/{ACCOUNT_ID}")
    ).unwrap();
    assert!(fixture.gateway.is_allowed(
        "example.reader", "providers:read", &format!("provider-account/{ACCOUNT_ID}")
    ).unwrap());
    fixture.gateway.revoke(
        "example.reader", "providers:read", &format!("provider-account/{ACCOUNT_ID}")
    ).unwrap();
    assert!(!fixture.gateway.is_allowed(
        "example.reader", "providers:read", &format!("provider-account/{ACCOUNT_ID}")
    ).unwrap());
}
```

Add command serialization tests that call the command adapters over a `ServiceFixture`, assert `ProviderAccountSummary` has no `secretRef`, and serialize an authentication failure after passing `pat = "glpat-never-serialize"`; the token must not appear in the JSON or `Debug` output. Do not derive `Debug`, `Clone`, or `Serialize` on secret-bearing command request structs.

Load `packages/contracts/src/__fixtures__/provider-contracts.json` with `include_str!`, deserialize its instance/account/page/binding/error values into the Rust public DTOs, serialize them back to `serde_json::Value`, and assert exact equality with the canonical fixture. This is the cross-language field-name/nullability guard.

- [ ] **Step 2: Run focused tests and verify RED**

```powershell
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml plugins::permissions
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml commands::tests::provider_
```

Expected: dynamic grant APIs, manifest-request inspection, and Provider commands do not exist.

- [ ] **Step 3: Add declared and dynamic permission operations**

Add these exact APIs:

```rust
impl PluginRegistry {
    pub fn manifest_requests(&self, plugin_id: &str, capability: &str, resource: &str) -> bool;
}

impl PermissionGateway {
    pub fn grant_dynamic(&self, plugin_id: &str, capability: &str, resource: &str) -> Result<(), AppError>;
    pub fn list_active_resources(&self, plugin_id: &str, capability: &str, prefix: &str) -> Result<Vec<String>, AppError>;
    pub fn revoke_dynamic(&self, plugin_id: &str, capability: &str, resource: &str) -> Result<(), AppError>;
    pub fn revoke_resource_for_all(&self, capability: &str, resource: &str) -> Result<(), AppError>;
}
```

`grant_dynamic` must verify a matching enabled `plugin_installations` row exists; its caller must already have checked the manifest request. Both revoke methods update `revoked_at` for only an exact resource string—never a caller-supplied prefix or wildcard. Account deletion uses the store transaction from Task 3 to revoke that exact resource alongside the account; `revoke_resource_for_all` exists for safe repair/cleanup paths. Never call `grant_manifest_permissions` for external plugins. Keep existing built-in first-install grants and revoked-grant persistence unchanged.

`provider_permission_revoke_account` marks the calling plugin's exact grant revoked first, then calls `cancel_for_plugin_account` and awaits `wait_for_plugin_account_idle` before returning. Add a test with a blocked fake adapter proving revocation cancels the in-flight request and the next call is denied before network access.

- [ ] **Step 4: Add strict Provider command request types**

Define `#[serde(rename_all = "camelCase", deny_unknown_fields)]` structs and command functions for these command names:

```text
provider_instance_list
provider_instance_create
provider_instance_update
provider_instance_validate
provider_instance_delete
provider_account_list
provider_account_connect
provider_account_rotate
provider_account_validate
provider_account_set_default
provider_account_deletion_impact
provider_account_delete
provider_repository_list
provider_operation_cancel
provider_local_remote_match
provider_binding_list
provider_binding_set
provider_binding_delete
provider_permission_is_declared
provider_permission_list_authorized_accounts
provider_permission_grant_accounts
provider_permission_revoke_account
```

Secret-bearing host-only structs contain `pat: String` but derive only `Deserialize`; immediately move the string into `SensitiveString` at the top of the command. Plugin-facing instance create/update types use `customCaAction`; the trusted TypeScript Host API translates that into a separate internal command struct with `custom_ca_path: Option<String>`.

Every repository-list, match, and cancel request carries `plugin_id`, `account_id`, and `operation_id`; cancel forwards the exact triple to `OperationRegistry`. `provider_binding_list` also requires `account_id` and filters inherited bindings through the current default account. Permission grant commands verify:

1. The target plugin manifest requests `providers:read/providers`.
2. Every selected account exists.
3. The concrete resource is exactly `provider-account/{uuid}`.
4. No wildcard, path traversal, or caller-provided prefix is accepted.

Register all commands in `invoke_handlers!`; do not add a generic Provider request or raw HTTP command.

- [ ] **Step 5: Verify and commit**

```powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml plugins::permissions
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml commands::tests::provider_
cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets -- -D warnings
git add -- apps/desktop/src-tauri/src/plugins/permissions.rs apps/desktop/src-tauri/src/plugins/registry.rs apps/desktop/src-tauri/src/commands.rs apps/desktop/src-tauri/src/lib.rs apps/desktop/src-tauri/src/error.rs
git commit -m "feat: expose scoped provider commands"
```

### Task 11: Route typed Provider RPC through dynamic authorization and trusted prompt ports

**Files:**

- Create: `apps/desktop/src/providers/promptPorts.ts`
- Modify: `apps/desktop/src/lib/hostApi.ts`
- Modify: `apps/desktop/src/lib/__tests__/hostApi.test.ts`
- Modify: `apps/desktop/src/plugins/rpcRouter.ts`
- Modify: `apps/desktop/src/plugins/__tests__/rpcRouter.test.ts`
- Modify: `apps/desktop/src/__tests__/FoundationFlow.test.tsx`
- Modify: `apps/desktop/src/__tests__/App.test.tsx`

- [ ] **Step 1: Write failing Host API secret-boundary tests**

Create an injected prompt port and assert PAT/CA are added only after the plugin request has crossed into the trusted host:

```ts
const prompts: ProviderPromptPort = {
  requestCredential: vi.fn(async () => "glpat-host-only"),
  requestAccountAccess: vi.fn(async ({ accounts }) => [accounts[0]!.id])
};
const api = createTauriHostApi({ prompts, files: { selectCertificate: vi.fn(async () => "C:/ca/root.pem") } });

await api.connectProviderAccount("git-ramus.provider-center", { instanceId });
expect(invoke).toHaveBeenCalledWith("provider_account_connect", {
  request: { instanceId, pat: "glpat-host-only" }
});
expect(JSON.stringify({ instanceId })).not.toContain("glpat-host-only");
```

Test credential cancellation returns `null` and does not invoke Rust. Test `customCaAction: "selectFile"` invokes the native file picker with certificate filters and sends the path only to the internal Tauri request; the returned DTO contains only `customCaConfigured`/`customCaLabel`.

- [ ] **Step 2: Write failing multi-requirement RPC tests**

Add a fake Provider HostApi and these cases:

```ts
it("authorizes account discovery by exact account or built-in Provider family", async () => {
  const params = { accountId, query, cursor: null, operationId };
  await dispatchPluginRpc(pluginId, request("providers.listRepositories", params), hostApi);
  expect(hostApi.authorizePluginCall).toHaveBeenNthCalledWith(1, {
    pluginId,
    capability: "providers:read",
    resource: `provider-account/${accountId}`
  });
  expect(hostApi.authorizePluginCall).toHaveBeenNthCalledWith(2, {
    pluginId,
    capability: "providers:read",
    resource: "providers"
  });
});

it("requires both Provider and repository read grants before matching", async () => {
  await dispatchPluginRpc(pluginId, request("providers.matchLocalRemotes", params), hostApi);
  expectGrantedBefore(hostApi.matchLocalProviderRemotes, [
    ["providers:read", `provider-account/${accountId}`],
    ["repositories:read", "repositories"]
  ]);
});
```

Add negative tests: denied calls never open a credential/access prompt, malformed UUIDs never call authorization, an external plugin can call `providers.requestReadAccess` only when `provider_permission_is_declared` returns true, and revoking an exact account makes the next repository call fail.

- [ ] **Step 3: Run TypeScript tests and verify RED**

```powershell
npm run test --workspace @git-ramus/desktop -- hostApi rpcRouter
```

Expected: missing Provider HostApi methods, prompt port, request schemas, and multi-requirement route handling cause failure.

- [ ] **Step 4: Implement a factory-backed trusted Host API**

Create `apps/desktop/src/providers/promptPorts.ts` with these ports; neither is exported to plugin code:

```ts
export interface ProviderPromptPort {
  requestCredential(request: ProviderCredentialPromptRequest): Promise<string | null>;
  requestAccountAccess(request: ProviderAccessPromptRequest): Promise<string[] | null>;
}

export interface HostFileSelectionPort {
  selectCertificate(): Promise<string | null>;
}

export const unavailableProviderPromptPort: ProviderPromptPort;
export const nativeCertificateFileSelectionPort: HostFileSelectionPort;
```

The unavailable prompt port rejects with stable `provider.prompt-unavailable` errors, so Task 11's production build is internally complete before dialogs exist. The native certificate port wraps `@tauri-apps/plugin-dialog` with `multiple: false` and PEM/CRT/CER certificate filters, rejects an array or empty result, and returns a path only to trusted Host API code.

In `hostApi.ts`, extend `HostApi` with this exact method in addition to its existing `authorizePluginCall`:

```ts
authorizePluginPermissionRequest(request: AuthorizationRequest): Promise<AuthorizationDecision>;
```

Then add the factory:

```ts
export function createTauriHostApi(dependencies: {
  prompts: ProviderPromptPort;
  files: HostFileSelectionPort;
}): HostApi;
```

Add typed HostApi methods corresponding to all Provider RPC operations. Parse every Rust response through the new contract schemas. `connectProviderAccount` and `rotateProviderAccount` request a PAT from `ProviderPromptPort`, invoke Rust, then overwrite the local variable before returning. `requestProviderReadAccess` must:

1. Ask Rust for authorizable account summaries.
2. Open the trusted access prompt with plugin ID and summaries.
3. Grant only selected UUIDs.
4. Return the now-authorized `ProviderAuthorizedAccount` wrappers, each containing only its safe instance and account summaries.

Create the default `tauriHostApi` with `unavailableProviderPromptPort` and `nativeCertificateFileSelectionPort`. Tests inject a fake prompt port. Task 12 replaces only the default prompt dependency with the singleton broker-backed port after the trusted dialogs exist.

- [ ] **Step 5: Generalize the route authorization table without weakening existing routes**

Replace one fixed capability/resource with explicit requirements:

```ts
interface AuthorizationRequirement<T> {
  check: "granted" | "declared";
  capability: string;
  resources(params: T): string[];
  mode: "all" | "any";
}
```

For existing Git routes, wrap their single fixed requirement and retain exact behavior. `check: "granted"` calls `authorizePluginCall`; `check: "declared"` calls `authorizePluginPermissionRequest`. Complete every requirement before invoking a HostApi handler. Use this exact Provider mapping:

| Routes | Requirements |
| --- | --- |
| `listInstances`, `listAccounts`, instance/account mutations, account deletion impact | granted `providers:manage/providers` |
| `listAuthorizedAccounts`, `requestReadAccess`, `revokeReadAccess` | declared `providers:read/providers`; HostApi returns/grants/revokes only the calling plugin's exact resources |
| `listRepositories`, `cancelOperation` | granted `providers:read` in any mode over exact `provider-account/{accountId}` and built-in family `providers` |
| `matchLocalRemotes`, `listBindings` | the account read any-of above and granted `repositories:read/repositories` |
| `bindRemote`, `unbindRemote` | granted `providers:manage/providers` and granted `repositories:read/repositories` |

Register explicit plugin methods:

```text
providers.listInstances / createInstance / updateInstance / validateInstance / deleteInstance
providers.listAccounts / connectAccount / rotateAccount / validateAccount / setDefaultAccount
providers.getAccountDeletionImpact / deleteAccount
providers.listAuthorizedAccounts / requestReadAccess / revokeReadAccess
providers.listRepositories / cancelOperation
providers.matchLocalRemotes
providers.listBindings / bindRemote / unbindRemote
```

No schema may contain `pat`, `secretRef`, `customCaPath`, a filesystem path, or an arbitrary URL for the HTTP client.

- [ ] **Step 6: Verify and commit**

```powershell
npx prettier --write apps/desktop/src/providers/promptPorts.ts apps/desktop/src/lib apps/desktop/src/plugins apps/desktop/src/__tests__
npm run typecheck --workspace @git-ramus/desktop
npm run test --workspace @git-ramus/desktop -- hostApi rpcRouter FoundationFlow App
git add -- apps/desktop/src/providers/promptPorts.ts apps/desktop/src/lib apps/desktop/src/plugins apps/desktop/src/__tests__
git commit -m "feat: route provider rpc through trusted host"
```

### Task 12: Add trusted PAT and Provider-access dialogs outside plugin iframes

**Files:**

- Create: `apps/desktop/src/providers/promptBroker.ts`
- Create: `apps/desktop/src/providers/ProviderCredentialDialog.tsx`
- Create: `apps/desktop/src/providers/ProviderAccessDialog.tsx`
- Create: `apps/desktop/src/providers/__tests__/providerPrompts.test.tsx`
- Modify: `apps/desktop/src/App.tsx`
- Modify: `apps/desktop/src/app.css`
- Modify: `apps/desktop/src/__tests__/App.test.tsx`
- Modify: `apps/desktop/src/lib/hostApi.ts`

- [ ] **Step 1: Write failing broker and dialog tests**

Add tests that:

```ts
const pending = providerPromptBroker.requestCredential({
  providerLabel: "GitLab",
  accountLabel: null,
  purpose: "connect"
});
render(<ProviderCredentialDialog broker={providerPromptBroker} />);
await user.type(screen.getByLabelText("Personal access token"), "glpat-transient");
await user.click(screen.getByRole("button", { name: "Connect" }));
await expect(pending).resolves.toBe("glpat-transient");
expect(screen.queryByDisplayValue("glpat-transient")).not.toBeInTheDocument();
```

Also assert:

- A second concurrent prompt rejects with stable code `provider.prompt-busy` instead of replacing the first.
- An access request while a credential prompt is active (and the inverse) is rejected by the same global prompt gate, so two trusted Provider dialogs can never overlap.
- Cancel resolves `null` and clears the password.
- The credential input has `type="password"`, `autoComplete="off"`, and `spellCheck={false}`.
- The access dialog shows plugin ID plus account checkboxes and returns only selected UUIDs.
- Neither dialog is rendered inside `PluginHost` or any iframe.
- Unmount resolves an active prompt as canceled and zeroes component state.

- [ ] **Step 2: Run tests and verify RED**

```powershell
npm run test --workspace @git-ramus/desktop -- providerPrompts App
```

Expected: prompt broker/dialog components and App integration do not exist.

- [ ] **Step 3: Implement one-at-a-time prompt brokers**

Implement a generic broker with subscribe/request/resolve/cancel semantics:

```ts
export interface PromptBroker<Request, Result> {
  request(request: Request): Promise<Result | null>;
  subscribe(listener: (request: ActivePrompt<Request> | null) => void): () => void;
  resolve(id: string, result: Result): void;
  cancel(id: string): void;
}
```

Use generated UUIDs and one shared `ProviderPromptGate` across the credential and access brokers. A request atomically acquires the gate; `resolve`, `cancel`, or unmount clears broker state and releases it before settling the promise. Listeners receive request metadata but never past results. Import the port types from `promptPorts.ts`; export singleton credential/access brokers and `providerPromptBrokerPort`, which delegates credential and access requests to those two gated brokers. Change only the default `tauriHostApi` prompt dependency from `unavailableProviderPromptPort` to `providerPromptBrokerPort`; retain the native certificate-file port from Task 11.

- [ ] **Step 4: Implement and mount the dialogs**

`ProviderCredentialDialog` keeps the PAT only in local component state. On submit, move it to the broker, immediately set state to `""`, and close. Display provider/account label, least-privilege guidance, Connect/Rotate wording, and Cancel.

`ProviderAccessDialog` renders only account summaries passed by the trusted Host API, starts with no account selected, and disables Approve until at least one is selected. It never renders usernames into logs or data attributes.

Mount both dialogs as siblings of `AppShell`/`PluginHost` in `App.tsx`:

```tsx
return (
  <>
    <AppShell
      version={version}
      plugins={plugins}
      selectedPluginId={selection?.pluginId ?? null}
      selectedRoute={selection?.route ?? null}
      jobs={jobs}
      hostApi={hostApi}
      themeCatalog={themeCatalog}
      themeState={themeState}
      themeActivationPending={themeActivationPending}
      onActivateTheme={activateTheme}
      onSelectPlugin={(pluginId, route) => setSelection({ pluginId, route })}
    >
      <PluginHost
        descriptor={selected}
        hostApi={hostApi}
        route={selection?.route ?? "/"}
        theme={themeState?.theme ?? null}
      />
    </AppShell>
    <ProviderCredentialDialog broker={providerCredentialBroker} />
    <ProviderAccessDialog broker={providerAccessBroker} />
  </>
);
```

Implement both prompts as an accessible `role="dialog"`, `aria-modal="true"` overlay styled through existing semantic `--gr-*` tokens. Focus the first control on open, trap Tab within the prompt, restore focus to the previously active Shell element on close, and close on Escape through the broker's cancel path.

- [ ] **Step 5: Verify and commit**

```powershell
npx prettier --write apps/desktop/src/providers apps/desktop/src/App.tsx apps/desktop/src/app.css apps/desktop/src/__tests__/App.test.tsx apps/desktop/src/lib/hostApi.ts
npm run lint
npm run typecheck --workspace @git-ramus/desktop
npm run test --workspace @git-ramus/desktop -- providerPrompts App hostApi
git add -- apps/desktop/src/providers apps/desktop/src/App.tsx apps/desktop/src/app.css apps/desktop/src/__tests__/App.test.tsx apps/desktop/src/lib/hostApi.ts
git commit -m "feat: add trusted provider credential prompts"
```

### Task 13: Build the unified Provider Center plugin

**Files:**

- Create: `plugins/provider-center/package.json`
- Create: `plugins/provider-center/tsconfig.json`
- Create: `plugins/provider-center/vite.config.ts`
- Create: `plugins/provider-center/index.html`
- Create: `plugins/provider-center/plugin.json`
- Create: `plugins/provider-center/src/main.tsx`
- Create: `plugins/provider-center/src/App.tsx`
- Create: `plugins/provider-center/src/api.ts`
- Create: `plugins/provider-center/src/style.css`
- Create: `plugins/provider-center/src/components/InstancePanel.tsx`
- Create: `plugins/provider-center/src/components/AccountPanel.tsx`
- Create: `plugins/provider-center/src/components/RepositoryBrowser.tsx`
- Create: `plugins/provider-center/src/components/RemoteBindings.tsx`
- Create: `plugins/provider-center/src/__tests__/api.test.ts`
- Create: `plugins/provider-center/src/__tests__/ProviderCenter.test.tsx`
- Create: `plugins/provider-center/src/__tests__/RepositoryBrowser.test.tsx`
- Modify: `scripts/sync-builtin-plugins.mjs`
- Modify: `scripts/sync-builtin-plugins.test.mjs`
- Modify: `package-lock.json`

- [ ] **Step 1: Scaffold the package and write failing API tests**

Create `package.json` with the exact workspace metadata and pinned versions already used by Git Client:

```json
{
  "name": "@git-ramus/provider-center",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "scripts": {
    "build": "vite build",
    "typecheck": "tsc -p tsconfig.json",
    "test": "vitest run"
  },
  "dependencies": {
    "@git-ramus/contracts": "0.1.0",
    "@git-ramus/plugin-sdk": "0.1.0",
    "react": "19.2.7",
    "react-dom": "19.2.7"
  },
  "devDependencies": {
    "@testing-library/jest-dom": "6.9.1",
    "@testing-library/react": "16.3.2",
    "@testing-library/user-event": "14.6.1",
    "@types/react": "19.2.17",
    "@types/react-dom": "19.2.3",
    "@vitejs/plugin-react": "6.0.3",
    "jsdom": "29.1.1",
    "vite": "8.1.5",
    "vite-plugin-singlefile": "2.3.3",
    "vitest": "4.1.10"
  }
}
```

Create `tsconfig.json` and `vite.config.ts` exactly as follows:

```json
{
  "extends": "../../tsconfig.base.json",
  "compilerOptions": {
    "jsx": "react-jsx",
    "types": ["vite/client", "vitest/globals"]
  },
  "include": ["src", "vite.config.ts"]
}
```

```ts
import react from "@vitejs/plugin-react";
import { viteSingleFile } from "vite-plugin-singlefile";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [react(), viteSingleFile()],
  build: {
    target: "es2022",
    assetsInlineLimit: Number.MAX_SAFE_INTEGER,
    cssCodeSplit: false
  },
  test: { environment: "jsdom" }
});
```

Create `index.html` with a single `#root`, title `Providers`, and `/src/main.tsx` module script.

Then create a fake `PluginClient` and test exact RPC names and request cancellation:

```ts
it("cancels an in-flight repository page with the same operation id", async () => {
  const controller = new AbortController();
  const promise = api.listRepositories({ accountId, query, cursor: null }, controller.signal);
  const listRequest = client.requests.find(({ method }) => method === "providers.listRepositories")!;
  controller.abort();
  await waitFor(() => expect(client.requests).toContainEqual({
    method: "providers.cancelOperation",
    params: { accountId, operationId: listRequest.params.operationId }
  }));
  await expect(promise).rejects.toMatchObject({ name: "AbortError" });
});
```

Test every API method against the route names from Task 11 and assert no request contains `pat`, `secretRef`, `customCaPath`, or a local filesystem path.

- [ ] **Step 2: Run API tests and verify RED**

```powershell
npm run test --workspace @git-ramus/provider-center -- api
```

Expected: the workspace/API do not exist.

- [ ] **Step 3: Implement the typed API wrapper**

Define `ProviderCenterApi` with instance/account/discovery/match/bind methods and parse normalized errors through `errorEnvelopeSchema`. For repository discovery:

```ts
async function listRepositories(input: ListInput, signal?: AbortSignal) {
  const operationId = crypto.randomUUID();
  if (signal?.aborted) throw new DOMException("Aborted", "AbortError");
  const request = client.request("providers.listRepositories", { ...input, operationId });
  let onAbort: (() => void) | undefined;
  const aborted = new Promise<never>((_, reject) => {
    onAbort = () => {
      void client
        .request("providers.cancelOperation", { accountId: input.accountId, operationId })
        .catch(() => undefined);
      reject(new DOMException("Aborted", "AbortError"));
    };
    signal?.addEventListener("abort", onAbort, { once: true });
    if (signal?.aborted) onAbort();
  });
  try {
    const result = await (signal ? Promise.race([request, aborted]) : request);
    return providerRepositoryPageSchema.parse(result);
  } finally {
    if (onAbort) signal?.removeEventListener("abort", onAbort);
  }
}
```

Use the same pattern for local-remote matching. Do not add general HTTP or secret methods.

- [ ] **Step 4: Write failing component journeys**

Test these complete user flows with fake APIs:

- GitHub form has a fixed Base URL; GitLab exposes HTTPS Base URL and optional CA selection.
- Creating an instance refreshes the list and selects it.
- Connect/Rotate buttons call host-prompting API methods without a token field.
- The first account shows Default; switching default updates only account state.
- Search/filter changes abort the previous request and discard a late response.
- Load more appends unique repository IDs and preserves prior items on a partial/rate-limit error.
- `suggested` requires explicit Bind click; `ambiguous` never binds until a candidate is chosen.
- Manual binding can choose an explicit account or “Use instance default”.
- Account deletion impact requires reassign/inherit/unbind choice before Delete is enabled.
- Provider disabled/action-required/rate-limited states render stable recovery actions.
- Refreshing after a fake Provider changes from disabled to enabled reuses the existing selected instance/account and resumes discovery without recreating either record.

Use accessible names rather than CSS selectors in Testing Library assertions.

- [ ] **Step 5: Implement focused Provider Center components**

`App.tsx` supports `/` and `/providers` and owns selected instance/account IDs. Keep network state in the narrowest component:

- `InstancePanel`: list/create/update/validate/delete; GitHub fixed URL; GitLab HTTPS validation text and CA action.
- `AccountPanel`: list/connect/rotate/validate/default/deletion-impact resolution.
- `RepositoryBrowser`: query form, abort generation, page append/dedup, Rate Limit banner.
- `RemoteBindings`: scan suggestions, ambiguity chooser, explicit/inherited account selection, bind/unbind.

Use a request-generation integer in addition to `AbortController`; a response updates state only when its generation is current. Show `ErrorEnvelope.message`, recovery action labels, and `retryAfterMs`, never `details` wholesale.

- [ ] **Step 6: Add the manifest, token-based styles, and staging entry**

Create this manifest:

```json
{
  "schemaVersion": 1,
  "id": "git-ramus.provider-center",
  "name": "Providers",
  "version": "0.1.0",
  "publisher": "git-ramus",
  "description": "GitHub and GitLab accounts, repositories, and local remote bindings.",
  "kind": "builtin",
  "sdkVersion": "^0.1.0",
  "entrypoints": { "ui": "ui.html" },
  "contributions": {
    "navigation": [{
      "id": "providers",
      "label": "Providers",
      "route": "/providers",
      "icon": "cloud"
    }]
  },
  "permissions": [
    { "capability": "providers:read", "resources": ["providers"] },
    { "capability": "providers:manage", "resources": ["providers"] },
    { "capability": "repositories:read", "resources": ["repositories"] }
  ]
}
```

Provider Center owns no visual theme. Use only `--gr-colors-*`, `--gr-spacing-*`, `--gr-shape-*`, `--gr-typography-*`, and density tokens with safe fallbacks so theme/UI style plugins replace its appearance through the existing host token channel. Add a test that scans `style.css` and rejects hex/rgb/hsl literals outside documented safe fallbacks. Add the workspace to the sync list and assert its staged directory is exactly `plugin.json` plus `ui.html`.

- [ ] **Step 7: Verify and commit**

```powershell
npm install --package-lock-only
npx prettier --write plugins/provider-center scripts
npm run typecheck --workspace @git-ramus/provider-center
npm run test --workspace @git-ramus/provider-center
node --test scripts/sync-builtin-plugins.test.mjs
npm run build --workspace @git-ramus/provider-center
git add -- plugins/provider-center scripts/sync-builtin-plugins.mjs scripts/sync-builtin-plugins.test.mjs package-lock.json
git commit -m "feat: add unified provider center"
```

### Task 14: Add the deterministic native Provider journey and run the release gate

**Files:**

- Modify: `.github/workflows/ci.yml`
- Create: `apps/desktop/src-tauri/src/providers/e2e_adapter.rs`
- Modify: `apps/desktop/src-tauri/src/providers/mod.rs`
- Modify: `apps/desktop/src-tauri/src/app_state.rs`
- Modify: `apps/desktop/src-tauri/src/e2e.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Create: `apps/desktop/e2e/fixture-provider.ts`
- Create: `apps/desktop/e2e/provider.e2e.ts`
- Modify: `apps/desktop/e2e/wdio.conf.ts`
- Modify: `docs/development.md`

- [ ] **Step 1: Write failing e2e-boundary tests**

Add Rust tests that assert the fixture adapter and fixture command are available only under both `feature = "e2e"` and `debug_assertions`, matching the existing Git fixture boundary. Add a test that debug E2E AppState uses `MemorySecretStore`, while release-with-e2e still constructs no fixture handler.

In `provider.e2e.ts`, add the journey before implementing the fixture:

```ts
const defaultQuery = {
  search: "",
  visibility: null,
  namespace: null,
  archived: "all",
  sort: "name",
  direction: "asc",
  pageSize: 30
} as const;

it("discovers a private GitLab repository and confirms a local remote binding", async () => {
  gitFixture = await seedFixture();
  const primaryProject = gitFixture.projects[0];
  const scan = record(await invokeHost("git_project_scan", {
    request: { projectId: primaryProject.projectId }
  }));
  const localRepository = record(records(scan.repositories)[0]?.repository);

  providerFixture = await seedProviderFixture();
  await (await $("button=Providers")).click();
  const frame = await $("iframe[title='Providers plugin']");
  await frame.waitForDisplayed();
  await waitForFrameRpc(frame, "providers.listInstances");
  const activated = record(await invokeHost("activate_theme", {
    request: { themeId: "git-ramus.theme.compact" }
  }));
  expect(activated.activeThemeId).toBe("git-ramus.theme.compact");
  await expect(frame).toHaveAttribute("data-plugin-theme-id", "git-ramus.theme.compact");

  const page = record(await invokeHost("provider_repository_list", {
    request: {
      pluginId: "git-ramus.provider-center",
      accountId: providerFixture.account.id,
      query: defaultQuery,
      cursor: null,
      operationId: crypto.randomUUID()
    }
  }));
  expect(records(page.items).map((item) => item.fullName)).toContain("skills/private-skill");

  const suggestions = records(await invokeHost("provider_local_remote_match", {
    request: {
      pluginId: "git-ramus.provider-center",
      instanceId: providerFixture.instance.id,
      accountId: providerFixture.account.id,
      operationId: crypto.randomUUID()
    }
  }));
  const suggestion = suggestions.find((item) => item.status === "suggested");
  if (suggestion === undefined) throw new Error("Expected one verified Provider suggestion");

  await invokeHost("provider_binding_set", {
    request: {
      repositoryId: text(localRepository.id),
      remoteName: "origin",
      instanceId: providerFixture.instance.id,
      accountId: null,
      providerRepositoryId: text(suggestion.providerRepositoryId)
    }
  });
  const bindingPage = record(await invokeHost("provider_binding_list", {
    request: { accountId: providerFixture.account.id }
  }));
  const bindings = records(bindingPage.items);
  expect(bindings).toHaveLength(1);

  const repositoryRoot = resolve(primaryProject.rootPath, gitFixture.primaryRepository.relativePath);
  const { stdout } = await execFileAsync("git", ["-C", repositoryRoot, "remote", "get-url", "origin"]);
  expect(stdout.trim()).toBe("git@gitlab.example.test:skills/private-skill.git");
});
```

Import `resolve` from `node:path`, promisify `node:child_process.execFile`, and use `seedFixture`/`cleanupGitClientJourney` from `fixture-project.ts`. Create `fixture-provider.ts` with strict parsers for only the instance/account/repository summaries returned by `e2e_seed_provider_fixture`; its cleanup calls the production account deletion command with `resolution: { kind: "unbind" }`, deletes the now-empty instance, and ignores only `resource.not-found`. In `after`, run Provider cleanup before `cleanupGitClientJourney({ workspaceId: null, identityId: null, fixture: gitFixture })` so bindings are removed before local remotes disappear.

- [ ] **Step 2: Run boundary/unit tests and verify RED**

```powershell
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml e2e_provider_fixture_handler
npm run typecheck --workspace @git-ramus/desktop
```

Expected: fixture adapter/command and Provider E2E spec are absent.

- [ ] **Step 3: Implement the debug-only deterministic adapter and fixture**

Under `#[cfg(all(feature = "e2e", debug_assertions))]`, implement a `RepositoryDiscoveryProvider` that:

- Accepts only PAT `e2e-provider-token`.
- Returns account ID `9001`, username `e2e-provider`.
- Returns private repository ID `4242`, full name `skills/private-skill`, HTTPS URL `https://gitlab.example.test/skills/private-skill.git`, and SSH URL `git@gitlab.example.test:skills/private-skill.git`.
- Supports page 1 only and returns no rate limit.
- Detects only the configured `gitlab.example.test` instance.

In debug E2E AppState, use `MemorySecretStore` and register this adapter in place of the real GitLab adapter. Add `e2e_seed_provider_fixture` that creates/validates the test instance, connects the fixed PAT through `ProviderService`, and returns only instance/account/repository summaries. Extend `create_repository` so the primary dirty fixture, and only that fixture, runs this bounded argument-array command after `git init`:

```text
git remote add origin git@gitlab.example.test:skills/private-skill.git
```

The E2E test scans the primary project before seeding Provider state, so production remote synchronization persists the new origin. Register the Provider fixture command only in the existing debug+feature handler list.

- [ ] **Step 4: Complete native E2E and developer documentation**

Add `./provider.e2e.ts` to the serial WDIO specs list. The test may inspect only trusted Shell elements and host-side RPC method markers; do not weaken `sandbox="allow-scripts"` or read the opaque plugin DOM. In `.github/workflows/ci.yml`, add a second release-with-e2e boundary step that runs `e2e_provider_fixture_handler_matches_the_debug_feature_boundary`; retain the existing Windows/Ubuntu E2E matrix unchanged so both platforms execute the Provider spec through WDIO.

Document:

- Provider unit/integration commands.
- Why CI uses mock adapters and MemorySecretStore.
- That PATs exist transiently only in trusted prompt/IPC memory.
- Manual release-candidate smoke steps for GitHub.com, GitLab.com, and HTTPS self-managed GitLab with an extra CA.
- Confirmation that Git operations continue to use SSH/GCM and are not part of this slice.

- [ ] **Step 5: Run focused native E2E**

```powershell
npm run prepare:plugins --workspace @git-ramus/desktop
npm run build:e2e --workspace @git-ramus/desktop
npm run test:e2e --workspace @git-ramus/desktop
```

Expected: Foundation, Git Client, and Provider journeys PASS; no real network credential or persistent OS-keychain entry is used.

- [ ] **Step 6: Run the full release gate**

```powershell
npx prettier --write .
npm run check
npm audit --audit-level=high
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
cargo test --release --features e2e --manifest-path apps/desktop/src-tauri/Cargo.toml --lib e2e_provider_fixture_handler_matches_the_debug_feature_boundary -- --exact
npm run desktop:build
npm run build:e2e --workspace @git-ramus/desktop
npm run test:e2e --workspace @git-ramus/desktop
git diff --check
git status --short
```

Expected: formatting, lint, TypeScript, Rust, contracts, plugin tests, adapter tests, migration tests, audit, release-boundary test, desktop build, and native E2E all PASS. `git status --short` lists only Task 14 files and any checked-off plan state.

- [ ] **Step 7: Commit the native journey and documentation**

```powershell
git add -- .github/workflows/ci.yml apps/desktop/src-tauri/src/providers/e2e_adapter.rs apps/desktop/src-tauri/src/providers/mod.rs apps/desktop/src-tauri/src/app_state.rs apps/desktop/src-tauri/src/e2e.rs apps/desktop/src-tauri/src/lib.rs apps/desktop/e2e/fixture-provider.ts apps/desktop/e2e/provider.e2e.ts apps/desktop/e2e/wdio.conf.ts docs/development.md
git commit -m "test: cover provider discovery journey"
```

After this commit, invoke `superpowers:requesting-code-review`, address technically valid findings with `superpowers:receiving-code-review`, rerun the full release gate, then use `superpowers:finishing-a-development-branch` for the user's chosen local merge/PR/push workflow.
