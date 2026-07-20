# Git Transport and Single-Repository Network Operations Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a secure single-repository network loop—Clone, Fetch, fast-forward-only Pull, safe Push, and reusable repository-scoped SSH/HTTPS transport profiles—without mixing Provider PATs, Git credentials, or commit identities.

**Architecture:** A new Rust `GitTransportService` owns network preflight, long-running system-Git execution, profile application, Clone staging/recovery, progress, and post-operation refresh. The existing `GitService` remains responsible for local repository state, while trusted Host UI mediates directory/key selection, network confirmation, and Provider-to-Git-Client Clone navigation across sandboxed plugin boundaries.

**Tech Stack:** Tauri 2.11, Rust 1.88/edition 2024, system Git 2.40+, rusqlite, parking_lot, Tokio/Tauri async runtime, React 19, TypeScript 6, Zod 4, Vite single-file plugins, Vitest/Testing Library, WebdriverIO Tauri E2E

---

## Scope boundary and working rules

Implement only the approved [Git Transport and Single-Repository Network Operations design](../specs/2026-07-20-git-transport-network-operations-design.md).

Do not add batch sync, automatic background Fetch, Remote editing, History/Branch/Merge/Rebase/Stash/Tag/conflict UI, Force Push, arbitrary RefSpecs, recursive Submodules, Git LFS materialization, Release APIs, or Skills Manager behavior. Do not add disabled placeholders for those later slices.

At execution time, use `superpowers:using-git-worktrees` to create `D:/Git-Ramus/.worktrees/git-transport-network-operations` on `codex/git-transport-network-operations`, starting from the commit containing this plan and the approved design. Run `npm ci` in that worktree before the first test.

Use red-green-refactor for every production behavior:

1. Write one focused failing test.
2. Run the exact focused command and observe the named failure.
3. Implement only enough behavior for that test.
4. Run the focused test and its local package/module suite.
5. Commit only the task files.

The existing core `JobStatus` enum remains unchanged. A partial or interrupted network operation is a `failed` Job carrying an ErrorEnvelope with category `partialResult` or `userActionRequired`; exact Clone recovery state lives in `git_clone_operations`. This avoids rebuilding the v1 `jobs` and `job_steps` tables.

Repository-wide release commands:

```powershell
npm run check
npm audit --audit-level=high
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets -- -D warnings
cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --features e2e --all-targets -- -D warnings
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npm run desktop:build
npm run build:e2e --workspace @git-ramus/desktop
npm run test:e2e --workspace @git-ramus/desktop
```

## File map

### Shared contracts

- Create `packages/contracts/src/transport.ts`: strict transport profile, binding, Clone intent/request/result, Fetch/Pull/Push, progress, and operation schemas.
- Create `packages/contracts/src/__fixtures__/transport-contracts.json`: secret-free cross-language canonical values.
- Modify `packages/contracts/src/index.ts`: export transport contracts.
- Modify `packages/contracts/src/__tests__/contracts.test.ts`: accepted/rejected transport payloads and secret/path/RefSpec boundaries.

### Rust persistence and domain

- Create `apps/desktop/src-tauri/migrations/0004_git_transport.sql`: transport profiles, bindings, config repairs, Clone operations, and v4 indexes.
- Modify `apps/desktop/src-tauri/src/db/migrations.rs`: run migration v4.
- Modify `apps/desktop/src-tauri/src/db/mod.rs`: v3-to-v4, constraints, preservation, and idempotency tests.
- Create `apps/desktop/src-tauri/src/git/transport/mod.rs`: focused public exports.
- Create `apps/desktop/src-tauri/src/git/transport/model.rs`: internal and serializable transport models.
- Create `apps/desktop/src-tauri/src/git/transport/store.rs`: profile, binding, repair, and Clone-operation SQL.
- Create `apps/desktop/src-tauri/src/git/transport/url.rs`: production Clone/Remote validation and normalization.
- Create `apps/desktop/src-tauri/src/git/transport/config.rs`: typed SSH/HTTPS config plans, hashing, apply/restore, and Drift comparison.
- Create `apps/desktop/src-tauri/src/git/transport/progress.rs`: bounded Git progress parsing and stage mapping.
- Create `apps/desktop/src-tauri/src/git/transport/operation.rs`: operation cancellation registry and duplicate-resource exclusion.
- Create `apps/desktop/src-tauri/src/git/transport/profile_service.rs`: profile lifecycle and compensating repository binding.
- Create `apps/desktop/src-tauri/src/git/transport/service.rs`: Fetch/Pull/Push preflight, orchestration, stable results, and refresh.
- Create `apps/desktop/src-tauri/src/git/transport/clone.rs`: Clone intent registry, staging ownership, safe checkout, registration, and recovery.
- Modify `apps/desktop/src-tauri/src/git/engine.rs`: explicit execution policies, streaming progress hook, and cancellation-aware process waiting.
- Modify `apps/desktop/src-tauri/src/git/mod.rs`: export transport module and execution types.
- Modify `apps/desktop/src-tauri/src/jobs/service.rs`: create a Job with a caller-owned UUID and fail interrupted transport jobs on startup.
- Modify `apps/desktop/src-tauri/src/error.rs`: stable `TransportFailure` and redacted ErrorEnvelope mapping.
- Modify `apps/desktop/src-tauri/src/app_state.rs`: build and expose the transport service with shared repository locks.
- Modify `apps/desktop/src-tauri/src/commands.rs`: typed profile, binding, intent, Clone/Fetch/Pull/Push, and cancellation commands.
- Modify `apps/desktop/src-tauri/src/lib.rs`: register transport commands and release-boundary probes.
- Create `apps/desktop/src-tauri/tests/git_transport_integration.rs`: real Git/Bare Remote acceptance tests.

### Trusted desktop Host and RPC

- Create `apps/desktop/src/git-transport/promptPorts.ts`: network/source/config confirmation and SSH-key/destination selection ports.
- Create `apps/desktop/src/git-transport/promptBroker.ts`: serialized trusted transport prompt broker.
- Create `apps/desktop/src/git-transport/TransportConfirmationDialog.tsx`: trusted prompt outside plugin iframes.
- Create `apps/desktop/src/git-transport/cloneNavigationBroker.ts`: one-shot Provider-to-Git-Client navigation requests.
- Create `apps/desktop/src/git-transport/__tests__/transportPrompts.test.tsx`: prompt isolation, clearing, and cancellation.
- Modify `apps/desktop/src/lib/hostApi.ts`: native pickers, prompt-mediated interactive invokes, and strict transport parsing.
- Modify `apps/desktop/src/lib/__tests__/hostApi.test.ts`: exact command payloads and absence of secrets/local paths at plugin boundaries.
- Modify `apps/desktop/src/plugins/rpcRouter.ts`: transport routes and capability/resource checks.
- Modify `apps/desktop/src/plugins/__tests__/rpcRouter.test.ts`: permission order, Provider Clone intent, and RefSpec/path rejection.
- Modify `apps/desktop/src/App.tsx`: subscribe to Clone navigation and mount trusted transport dialogs.
- Modify `apps/desktop/src/shell/AppShell.tsx`: host overlay slot remains outside `PluginHost`.
- Modify `apps/desktop/src/app.css`: token-based transport dialog styling.
- Modify `apps/desktop/src/__tests__/App.test.tsx`: trusted placement and Clone route navigation tests.

### Built-in plugin UI

- Modify `plugins/git-client/plugin.json`: transport/network capabilities and Transport navigation.
- Modify `plugins/git-client/src/api.ts`: typed profile, Clone, network operation, and cancellation methods.
- Modify `plugins/git-client/src/App.tsx`: `/transport-identities`, `/clone`, `/clone/<intent-id>`, and repository Network routing.
- Create `plugins/git-client/src/views/TransportProfilesView.tsx`: reusable SSH/HTTPS profile management.
- Create `plugins/git-client/src/views/CloneView.tsx`: manual/Provider Clone wizard.
- Create `plugins/git-client/src/components/RepositoryNetworkPanel.tsx`: Remote/upstream/profile/Fetch/Pull/Push UI.
- Create `plugins/git-client/src/components/TransportProfileForm.tsx`: typed profile editor.
- Create `plugins/git-client/src/__tests__/TransportProfilesView.test.tsx`: lifecycle, delete impact, and redaction tests.
- Create `plugins/git-client/src/__tests__/CloneView.test.tsx`: manual/Provider input, project choice, cancellation, and partial recovery tests.
- Create `plugins/git-client/src/__tests__/RepositoryNetworkPanel.test.tsx`: preflight/button/upstream/Drift tests.
- Modify `plugins/git-client/src/views/RepositoryView.tsx`: compose the Network panel without adding network logic to the Changes component.
- Modify `plugins/git-client/src/__tests__/RepositoryView.test.tsx`: Network panel composition and refresh regression.
- Modify `plugins/git-client/src/style.css`: semantic-token Network/Profile/Clone layouts.
- Modify `plugins/provider-center/plugin.json`: permission to create Clone intents only.
- Modify `plugins/provider-center/src/api.ts`: typed `createCloneIntent` call.
- Modify `plugins/provider-center/src/components/RepositoryBrowser.tsx`: Clone action per repository.
- Modify `plugins/provider-center/src/__tests__/api.test.ts` and `RepositoryBrowser.test.tsx`: exact secret-free intent request and Clone action tests.

### Native journey and release gates

- Modify `apps/desktop/src-tauri/src/e2e.rs`: Debug-only Bare Remote fixture and sealed URL rewrite.
- Create `apps/desktop/e2e/fixture-transport.ts`: strict fixture/result parsing and guarded cleanup.
- Create `apps/desktop/e2e/git-transport.e2e.ts`: Provider intent → Clone → Fetch → ff-only Pull → Push journey.
- Modify `apps/desktop/e2e/wdio.conf.ts`: include the transport spec serially.
- Modify `.github/workflows/ci.yml`: release-boundary proof for transport fixtures/rewrites.
- Modify `docs/development.md`: focused transport tests and real GCM/SSH smoke procedure.

---

### Task 1: Add strict transport contracts

**Files:**

- Create: `packages/contracts/src/transport.ts`
- Create: `packages/contracts/src/__fixtures__/transport-contracts.json`
- Modify: `packages/contracts/src/index.ts`
- Test: `packages/contracts/src/__tests__/contracts.test.ts`

- [x] **Step 1: Write failing profile and network contract tests**

Add imports from `../transport` and focused cases equivalent to:

```ts
it("accepts secret-free SSH and HTTPS transport profile summaries", () => {
  expect(
    transportProfileSummarySchema.parse({
      id: "0f0df6b1-9c42-499d-a76a-e4810fa19ace",
      displayName: "Work SSH",
      kind: "ssh",
      sshKeyFileName: "id_ed25519",
      httpsUsername: null,
      available: true,
      boundRepositoryCount: 2
    }).sshKeyFileName
  ).toBe("id_ed25519");
  expect(() =>
    transportProfileSummarySchema.parse({
      id: "0f0df6b1-9c42-499d-a76a-e4810fa19ace",
      displayName: "Leaky",
      kind: "ssh",
      sshKeyFileName: "id_ed25519",
      httpsUsername: null,
      available: true,
      boundRepositoryCount: 0,
      sshKeyPath: "C:/Users/private/.ssh/id_ed25519"
    })
  ).toThrow();
});

it("keeps local paths, credentials, environment, and refspecs out of plugin requests", () => {
  const base = {
    repositoryId: "0f0df6b1-9c42-499d-a76a-e4810fa19ace",
    projectId: null,
    workspaceId: null,
    operationId: "b95c216a-dac4-45d1-8169-8dbfbc0c0315"
  };
  expect(() => repositoryFetchRequestSchema.parse({ ...base, remoteName: "origin", pat: "x" })).toThrow();
  expect(() => repositoryFetchRequestSchema.parse({ ...base, remoteName: "--upload-pack=evil" })).toThrow();
  expect(() => repositoryPushRequestSchema.parse({ ...base, refspec: "+main:main" })).toThrow();
  expect(() => cloneRequestSchema.parse({
    source: { kind: "manual", remoteUrl: "https://example.test/acme/repo.git" },
    transportKind: "https",
    profileId: null,
    folderName: "repo",
    projectTarget: { kind: "new", name: "Repo" },
    operationId: base.operationId,
    destinationParent: "C:/Users/private"
  })).toThrow();
});

it("requires exactly one safe Clone source", () => {
  expect(cloneRequestSchema.parse({
    source: { kind: "intent", intentId: "90e1e991-f93e-4e78-817e-d0ceeb06a749" },
    transportKind: "ssh",
    profileId: "0f0df6b1-9c42-499d-a76a-e4810fa19ace",
    folderName: "repository",
    projectTarget: { kind: "existing", projectId: "3b84198e-bb1a-4f0d-875f-d82f0c18c630" },
    operationId: "b95c216a-dac4-45d1-8169-8dbfbc0c0315"
  }).source.kind).toBe("intent");
});
```

- [x] **Step 2: Run the contract test and verify the missing module failure**

Run:

```powershell
npm run test --workspace @git-ramus/contracts
```

Expected: FAIL because `../transport` and the new exported schemas do not exist.

- [x] **Step 3: Implement exact Zod schemas and inferred types**

Create `transport.ts` with these named exports and strict object schemas:

```ts
import { z } from "zod";
import { persistedRepositorySnapshotSchema, projectSchema, repositoryRequestSchema, repositorySchema } from "./git";
import { jobSchema } from "./jobs";

const uuid = z.string().uuid();
const safeName = z.string().trim().min(1).max(128);
const remoteName = z.string().min(1).max(255).refine(
  (value) =>
    !value.startsWith("-") &&
    !Array.from(value).some((character) => {
      const code = character.charCodeAt(0);
      return code <= 0x20 || code === 0x7f || "~^:?*[\\".includes(character);
    }),
  "unsafe remote name"
);
const folderName = z.string().min(1).max(255).refine(
  (value) => value !== "." && value !== ".." && !/[\\/\u0000-\u001f\u007f]/u.test(value),
  "unsafe clone folder name"
);

export const transportKindSchema = z.enum(["ssh", "https"]);
export const transportProfileSummarySchema = z.object({
  id: uuid,
  displayName: safeName,
  kind: transportKindSchema,
  sshKeyFileName: z.string().min(1).max(255).nullable(),
  httpsUsername: z.string().min(1).max(256).nullable(),
  available: z.boolean(),
  boundRepositoryCount: z.number().int().nonnegative()
}).strict();

export const transportProfileCreateRequestSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("ssh"), displayName: safeName, sshKeyAction: z.literal("selectFile"), identitiesOnly: z.boolean() }).strict(),
  z.object({ kind: z.literal("https"), displayName: safeName, username: z.string().trim().min(1).max(256), useHttpPath: z.literal(true) }).strict()
]);

export const providerCloneIntentCreateRequestSchema = z.object({
  accountId: uuid,
  repositoryId: z.string().min(1).max(1024)
}).strict();

export const cloneSourceSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("intent"), intentId: uuid }).strict(),
  z.object({ kind: z.literal("manual"), remoteUrl: z.string().min(1).max(4096) }).strict()
]);

export const cloneRequestSchema = z.object({
  source: cloneSourceSchema,
  transportKind: transportKindSchema,
  profileId: uuid.nullable(),
  folderName,
  projectTarget: z.discriminatedUnion("kind", [
    z.object({ kind: z.literal("existing"), projectId: uuid }).strict(),
    z.object({ kind: z.literal("new"), name: safeName }).strict()
  ]),
  operationId: uuid
}).strict();

const networkBase = repositoryRequestSchema.extend({ operationId: uuid }).strict();
export const repositoryFetchRequestSchema = networkBase.extend({ remoteName }).strict();
export const repositoryPullRequestSchema = networkBase;
export const repositoryPushRequestSchema = networkBase.extend({
  target: z.object({ remoteName, branchName: z.string().min(1).max(1024) }).strict().nullable()
}).strict();
```

Add strict schemas for profile update/delete impact/delete resolution, bindings, effective transport, Clone intent summary, Clone result, operation progress/result, cancel request, upstream candidates, and `repositoryNetworkStateSchema`. The network state contains Branch, detached flag, upstream, sanitized Remotes, ahead/behind, conflict count, and a nullable enum for Merge/Rebase/Cherry-pick/Revert/Bisect in progress. Add a refinement to `transportProfileSummarySchema` so SSH summaries require only `sshKeyFileName` and HTTPS summaries require only `httpsUsername`. Export all inferred TypeScript types used by later tasks. Use `jobSchema`, `repositorySchema`, `projectSchema`, and `persistedRepositorySnapshotSchema` rather than duplicating them. The fixture must contain one SSH profile, one HTTPS profile, one clean binding, one Provider intent, one network state, one Clone result, and one non-fast-forward ErrorEnvelope; it must contain none of `pat`, `secret`, `password`, `privateKey`, `sshKeyPath`, or a local absolute path.

Export `./transport` from `index.ts`.

- [x] **Step 4: Run focused tests and typecheck**

Run:

```powershell
npm run test --workspace @git-ramus/contracts
npm run typecheck --workspace @git-ramus/contracts
```

Expected: all contract tests PASS and TypeScript exits 0.

- [x] **Step 5: Commit the contract slice**

```powershell
git add packages/contracts/src/transport.ts packages/contracts/src/__fixtures__/transport-contracts.json packages/contracts/src/index.ts packages/contracts/src/__tests__/contracts.test.ts
git commit -m "feat: add git transport contracts"
```

### Task 2: Add the v4 transport migration and Rust models

**Files:**

- Create: `apps/desktop/src-tauri/migrations/0004_git_transport.sql`
- Create: `apps/desktop/src-tauri/src/git/transport/mod.rs`
- Create: `apps/desktop/src-tauri/src/git/transport/model.rs`
- Modify: `apps/desktop/src-tauri/src/git/mod.rs`
- Modify: `apps/desktop/src-tauri/src/db/migrations.rs`
- Test: `apps/desktop/src-tauri/src/db/mod.rs`

- [x] **Step 1: Write failing v4 migration tests**

Add tests that open v3 explicitly, insert one Project/Repository/Job, run migrations, and assert preservation plus the new constraints:

```rust
#[test]
fn upgrading_v3_preserves_rows_and_creates_transport_tables() {
    let mut connection = rusqlite::Connection::open_in_memory().unwrap();
    connection.execute_batch("PRAGMA foreign_keys=ON;").unwrap();
    connection.execute_batch(super::migrations::MIGRATION_1).unwrap();
    connection.execute_batch(super::migrations::MIGRATION_2).unwrap();
    connection.execute_batch(super::migrations::MIGRATION_3).unwrap();
    connection.execute("INSERT INTO projects(id,root_path,name,created_at,updated_at) VALUES('p','/tmp/p','P','2026-07-20T00:00:00Z','2026-07-20T00:00:00Z')", []).unwrap();
    super::migrations::run(&mut connection).unwrap();
    let version: i64 = connection.query_row("PRAGMA user_version", [], |row| row.get(0)).unwrap();
    assert_eq!(version, 4);
    for table in ["transport_profiles", "repository_transport_bindings", "transport_config_repairs", "git_clone_operations"] {
        let count: i64 = connection.query_row("SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1", [table], |row| row.get(0)).unwrap();
        assert_eq!(count, 1, "missing {table}");
    }
    let project_count: i64 = connection.query_row("SELECT COUNT(*) FROM projects WHERE id='p'", [], |row| row.get(0)).unwrap();
    assert_eq!(project_count, 1);
}

#[test]
fn transport_profile_kind_constraints_reject_mixed_fields() {
    let database = Database::open_in_memory().unwrap();
    let result = database.with_connection(|connection| connection.execute(
        "INSERT INTO transport_profiles(id,display_name,kind,ssh_key_path,ssh_variant,ssh_identities_only,https_username,https_use_http_path,created_at,updated_at) VALUES('mixed','Mixed','ssh','/tmp/key','ssh',1,'user',1,'2026-07-20T00:00:00Z','2026-07-20T00:00:00Z')",
        []
    ));
    assert!(result.is_err());
}
```

- [x] **Step 2: Run the focused DB tests and observe the v3 failure**

Run:

```powershell
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml db::tests::upgrading_v3_preserves_rows_and_creates_transport_tables -- --exact
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml db::tests::transport_profile_kind_constraints_reject_mixed_fields -- --exact
```

Expected: FAIL because `MIGRATION_4` and the transport tables do not exist.

- [x] **Step 3: Implement migration v4**

Create the migration with these invariants:

```sql
BEGIN IMMEDIATE;

CREATE TABLE transport_profiles (
    id TEXT PRIMARY KEY NOT NULL,
    display_name TEXT NOT NULL UNIQUE,
    kind TEXT NOT NULL CHECK (kind IN ('ssh','https')),
    ssh_key_path TEXT,
    ssh_variant TEXT,
    ssh_identities_only INTEGER CHECK (ssh_identities_only IN (0,1)),
    https_username TEXT,
    https_use_http_path INTEGER CHECK (https_use_http_path IN (0,1)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    CHECK (
      (kind='ssh' AND ssh_key_path IS NOT NULL AND ssh_variant='ssh' AND ssh_identities_only IS NOT NULL AND https_username IS NULL AND https_use_http_path IS NULL)
      OR
      (kind='https' AND ssh_key_path IS NULL AND ssh_variant IS NULL AND ssh_identities_only IS NULL AND https_username IS NOT NULL AND length(trim(https_username)) > 0 AND https_use_http_path=1)
    )
);

CREATE TABLE repository_transport_bindings (
    repository_id TEXT PRIMARY KEY NOT NULL,
    transport_profile_id TEXT NOT NULL,
    before_config_json TEXT NOT NULL,
    applied_config_json TEXT NOT NULL,
    applied_config_hash TEXT NOT NULL,
    drift_status TEXT NOT NULL DEFAULT 'clean' CHECK (drift_status IN ('clean','drifted')),
    bound_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY(repository_id) REFERENCES repositories(id) ON DELETE RESTRICT,
    FOREIGN KEY(transport_profile_id) REFERENCES transport_profiles(id) ON DELETE RESTRICT
);

CREATE TABLE transport_config_repairs (
    id TEXT PRIMARY KEY NOT NULL,
    repository_id TEXT NOT NULL,
    before_config_json TEXT NOT NULL,
    attempted_config_json TEXT NOT NULL,
    error_code TEXT NOT NULL,
    created_at TEXT NOT NULL,
    resolved_at TEXT,
    FOREIGN KEY(repository_id) REFERENCES repositories(id) ON DELETE RESTRICT
);

CREATE TABLE git_clone_operations (
    operation_id TEXT PRIMARY KEY NOT NULL,
    job_id TEXT NOT NULL UNIQUE,
    source_summary TEXT NOT NULL,
    intent_id TEXT,
    staging_path TEXT NOT NULL,
    owner_marker_path TEXT NOT NULL,
    final_path TEXT NOT NULL,
    project_target_json TEXT NOT NULL,
    current_stage TEXT NOT NULL,
    filesystem_complete INTEGER NOT NULL DEFAULT 0 CHECK (filesystem_complete IN (0,1)),
    repository_id TEXT,
    project_id TEXT,
    profile_applied INTEGER NOT NULL DEFAULT 0 CHECK (profile_applied IN (0,1)),
    provider_binding_complete INTEGER NOT NULL DEFAULT 0 CHECK (provider_binding_complete IN (0,1)),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    FOREIGN KEY(job_id) REFERENCES jobs(id) ON DELETE CASCADE,
    FOREIGN KEY(repository_id) REFERENCES repositories(id) ON DELETE RESTRICT,
    FOREIGN KEY(project_id) REFERENCES projects(id) ON DELETE RESTRICT
);

CREATE INDEX idx_transport_bindings_profile ON repository_transport_bindings(transport_profile_id, repository_id);
CREATE INDEX idx_transport_repairs_repository ON transport_config_repairs(repository_id, resolved_at);
PRAGMA user_version = 4;
COMMIT;
```

Add `MIGRATION_4`, the `current < 4` runner branch, and update existing version/table-count assertions from 3 to 4.

- [x] **Step 4: Add typed Rust models and module exports**

Define `TransportKind`, `TransportProfile`, `TransportProfileSummary`, `RepositoryTransportBinding`, `TransportConfigSnapshot`, `TransportDriftStatus`, `CloneOperation`, `CloneStage`, `CloneProjectTarget`, `EffectiveTransport`, `CloneIntent`, `NetworkOperationResult`, and focused input types. Every serializable public type uses `#[serde(rename_all = "camelCase", deny_unknown_fields)]` where deserialized. Keep full key paths only on internal `TransportProfile`; summaries expose `ssh_key_file_name`.

Export the module from `git/mod.rs`:

```rust
pub mod transport;
```

- [x] **Step 5: Run DB and model tests**

Run:

```powershell
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml db::tests
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml git::transport::model::tests
```

Expected: all focused tests PASS.

- [x] **Step 6: Commit migration and models**

```powershell
git add apps/desktop/src-tauri/migrations/0004_git_transport.sql apps/desktop/src-tauri/src/db/migrations.rs apps/desktop/src-tauri/src/db/mod.rs apps/desktop/src-tauri/src/git/mod.rs apps/desktop/src-tauri/src/git/transport/mod.rs apps/desktop/src-tauri/src/git/transport/model.rs
git commit -m "feat: persist git transport state"
```

### Task 3: Implement transport persistence repositories

**Files:**

- Create: `apps/desktop/src-tauri/src/git/transport/store.rs`
- Modify: `apps/desktop/src-tauri/src/git/transport/mod.rs`
- Test: `apps/desktop/src-tauri/src/git/transport/store.rs`

- [x] **Step 1: Write failing store round-trip and invariant tests**

Create in-module tests that seed a Repository and assert:

```rust
#[test]
fn profile_binding_and_clone_recovery_round_trip_without_secrets() {
    let database = Database::open_in_memory().unwrap();
    let repository = seed_repository(&database, "/tmp/transport-store");
    let store = TransportStore::new(database.clone());
    let profile = ssh_profile("Work", "/keys/id_ed25519");
    store.insert_profile(&profile).unwrap();
    let binding = RepositoryTransportBinding::new(
        &repository.id,
        &profile.id,
        TransportConfigSnapshot::empty(),
        ssh_applied_snapshot("/keys/id_ed25519"),
    );
    store.upsert_binding(&binding).unwrap();
    assert_eq!(store.get_binding(&repository.id).unwrap().unwrap(), binding);
    assert_eq!(store.profile_deletion_impact(&profile.id).unwrap().repository_ids, vec![repository.id.clone()]);

    let clone = clone_operation("operation", "job", "/tmp/.git-ramus-clone-operation", "/tmp/repository");
    store.insert_clone_operation(&clone).unwrap();
    assert_eq!(store.get_clone_operation("operation").unwrap().unwrap().final_path, clone.final_path);

    let serialized = serde_json::to_string(&store.list_profile_summaries().unwrap()).unwrap();
    assert!(!serialized.contains("/keys/id_ed25519"));
}

#[test]
fn bound_profile_and_unresolved_repair_block_destructive_changes() {
    let database = Database::open_in_memory().unwrap();
    let repository = seed_repository(&database, "/tmp/transport-repair");
    let store = TransportStore::new(database);
    let profile = https_profile("Work", "worker");
    store.insert_profile(&profile).unwrap();
    store.upsert_binding(&RepositoryTransportBinding::new(
        &repository.id,
        &profile.id,
        TransportConfigSnapshot::empty(),
        https_applied_snapshot("worker"),
    )).unwrap();
    assert!(store.delete_profile(&profile.id).is_err());
    store.insert_repair(&repair(&repository.id)).unwrap();
    assert!(store.repository_has_unresolved_repair(&repository.id).unwrap());
}
```

- [x] **Step 2: Run tests and verify `TransportStore` is missing**

Run:

```powershell
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml git::transport::store::tests
```

Expected: FAIL because `store.rs` and repository methods do not exist.

- [x] **Step 3: Implement focused store methods**

Implement exact SQL methods:

- `insert_profile`, `update_profile`, `get_profile`, `list_profiles`, `list_profile_summaries`, `delete_profile`.
- `get_binding`, `list_bindings_for_profile`, `upsert_binding`, `mark_binding_drifted`, `delete_binding`.
- `profile_deletion_impact` returning stable sorted Repository IDs.
- `insert_repair`, `repository_has_unresolved_repair`, `resolve_repair`.
- `insert_clone_operation`, `get_clone_operation`, `update_clone_stage`, `mark_clone_filesystem_complete`, `mark_clone_repository`, `mark_clone_profile`, `mark_clone_provider_binding`, `list_incomplete_clone_operations`, `delete_clone_operation`.

All multi-row profile deletion resolutions use `Database::with_immediate_transaction`; map constraint errors through `map_constraint_error`. Parse stored JSON through concrete `TransportConfigSnapshot` and `CloneProjectTarget`, never through untyped `Value` in service code.

- [x] **Step 4: Run store and DB tests**

```powershell
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml git::transport::store::tests
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml db::tests
```

Expected: all tests PASS.

- [x] **Step 5: Commit the store**

```powershell
git add apps/desktop/src-tauri/src/git/transport/mod.rs apps/desktop/src-tauri/src/git/transport/store.rs
git commit -m "feat: add git transport repositories"
```

### Task 4: Add URL validation, config plans, and stable transport failures

**Files:**

- Create: `apps/desktop/src-tauri/src/git/transport/url.rs`
- Create: `apps/desktop/src-tauri/src/git/transport/config.rs`
- Modify: `apps/desktop/src-tauri/src/git/transport/mod.rs`
- Modify: `apps/desktop/src-tauri/src/error.rs`
- Test: `apps/desktop/src-tauri/src/git/transport/url.rs`
- Test: `apps/desktop/src-tauri/src/git/transport/config.rs`
- Test: `apps/desktop/src-tauri/src/error.rs`

- [x] **Step 1: Write failing URL and redaction tests**

```rust
#[test]
fn production_clone_urls_allow_only_https_ssh_and_scp_without_embedded_secrets() {
    assert_eq!(validate_clone_url("https://gitlab.example/group/repo.git").unwrap().kind, TransportKind::Https);
    assert_eq!(validate_clone_url("ssh://git@gitlab.example:2222/group/repo.git").unwrap().kind, TransportKind::Ssh);
    assert_eq!(validate_clone_url("git@gitlab.example:group/repo.git").unwrap().kind, TransportKind::Ssh);
    for value in [
        "file:///tmp/repo",
        "../repo",
        "git://gitlab.example/group/repo.git",
        "ext::sh -c evil",
        "https://user:secret@gitlab.example/group/repo.git",
        "https://gitlab.example/group/repo.git?token=secret",
    ] {
        assert!(validate_clone_url(value).is_err(), "accepted {value}");
    }
}

#[test]
fn transport_failure_envelopes_never_echo_remote_or_key_material() {
    let error = AppError::Transport(
        TransportFailure::authentication_required()
            .with_operation("operation")
            .with_resource("repository")
    );
    let serialized = serde_json::to_string(&ErrorEnvelope::from(error)).unwrap();
    assert!(serialized.contains("git.transport.authentication-required"));
    for secret in ["ghp_secret", "C:\\\\Users\\\\name\\\\.ssh", "user:password@"] {
        assert!(!serialized.contains(secret));
    }
}
```

- [x] **Step 2: Write failing config-plan and quoting tests**

```rust
#[test]
fn ssh_plan_is_built_only_from_typed_fields_and_https_plan_scopes_username() {
    let ssh = config_plan(&ssh_profile("Key", r"C:\keys\work key"), &ssh_remote()).unwrap();
    assert_eq!(ssh.kind, TransportKind::Ssh);
    assert_eq!(ssh.values.get("ssh.variant").map(String::as_str), Some("ssh"));
    let command = ssh.values.get("core.sshCommand").unwrap();
    assert!(command.contains("IdentitiesOnly=yes"));
    assert!(!command.contains('\n'));

    let https = config_plan(&https_profile("Web", "creator"), &https_remote()).unwrap();
    assert_eq!(https.values.get("credential.useHttpPath").map(String::as_str), Some("true"));
    assert!(https.values.iter().any(|(key, value)| key.starts_with("credential.https://gitlab.example/group/repo.git.username") && value == "creator"));
}
```

- [x] **Step 3: Run focused tests and observe missing modules/types**

```powershell
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml git::transport::url::tests
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml git::transport::config::tests
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml error::tests::transport_failure_envelopes_never_echo_remote_or_key_material -- --exact
```

Expected: FAIL because URL/config modules and `TransportFailure` do not exist.

- [x] **Step 4: Implement URL normalization and typed config construction**

`validate_clone_url` must return a `ValidatedRemoteUrl` containing only `kind`, normalized Host/Port/Path, sanitized display value, and the exact execution URL after rejecting control characters, HTTPS UserInfo/Query/Fragment, local paths, unsupported schemes, and `::` Remote Helpers. Reuse Provider URL normalization rules where semantics match, but keep the production Clone allowlist independent so Provider changes cannot widen Git execution.

`config_plan` returns:

```rust
pub struct ManagedConfigPlan {
    pub kind: TransportKind,
    pub values: std::collections::BTreeMap<String, String>,
}
```

Build `core.sshCommand` with one platform-tested quoting helper and fixed arguments only: system `ssh`, `-i <selected-path>`, and optional `-o IdentitiesOnly=yes`. Reject newline, NUL, quote-breaking control data, non-absolute key paths, and a missing/non-file key. HTTPS writes only `credential.useHttpPath=true` and normalized `credential.<url>.username` keys.

Add canonical `TransportConfigSnapshot::sha256()` over sorted UTF-8 key/value pairs. Add direct dependency `sha2 = "0.10"` to `Cargo.toml` and let Cargo update `Cargo.lock` in this task; do not rely on a transitive crate.

- [x] **Step 5: Implement `TransportFailure`**

Add an internal `AppError::Canceled` variant for the generic process layer, with a fixed redacted generic envelope. Mirror `ProviderFailure` with `TransportFailure` fixed constructors for all codes in design section 17. Store only fixed messages and typed context fields; raw stderr stays in an internal classifier input and never in `Debug`, `Display`, or `ErrorEnvelope.details`. Extend `RecoveryActionKind` only if an existing kind cannot express the specified action; prefer current `Retry`, `OpenSettings`, `Reauthorize`, and `ResolveConflict`.

- [x] **Step 6: Run focused tests and Clippy**

```powershell
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml git::transport::url::tests
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml git::transport::config::tests
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml error::tests
cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets -- -D warnings
```

Expected: all tests PASS and Clippy exits 0.

- [x] **Step 7: Commit URL, config, and error foundations**

```powershell
git add apps/desktop/src-tauri/Cargo.toml apps/desktop/src-tauri/Cargo.lock apps/desktop/src-tauri/src/error.rs apps/desktop/src-tauri/src/git/transport/mod.rs apps/desktop/src-tauri/src/git/transport/url.rs apps/desktop/src-tauri/src/git/transport/config.rs
git commit -m "feat: validate git transport inputs"
```

### Task 5: Extend GitEngine for explicit network execution, progress, and cancellation

**Files:**

- Modify: `apps/desktop/src-tauri/src/git/engine.rs`
- Create: `apps/desktop/src-tauri/src/git/transport/progress.rs`
- Create: `apps/desktop/src-tauri/src/git/transport/operation.rs`
- Modify: `apps/desktop/src-tauri/src/git/transport/mod.rs`
- Modify: `apps/desktop/src-tauri/src/git/mod.rs`
- Test: `apps/desktop/src-tauri/src/git/engine.rs`
- Test: `apps/desktop/src-tauri/src/git/transport/progress.rs`
- Test: `apps/desktop/src-tauri/src/git/transport/operation.rs`

- [x] **Step 1: Write failing execution-policy environment tests**

Extend the existing `clean_environment_from` test helpers with exact policy expectations:

```rust
#[test]
fn foreground_network_policy_allows_system_interaction_without_inheriting_attack_overrides() {
    let environment = captured_environment(
        GitExecutionPolicy::ForegroundNetworkInteractive,
        [
            ("PATH", "/safe/bin"),
            ("SSH_AUTH_SOCK", "/safe/agent"),
            ("GIT_ASKPASS", "/tmp/evil"),
            ("SSH_ASKPASS", "/tmp/evil-ssh"),
            ("GIT_SSH_COMMAND", "evil"),
            ("GCM_INTERACTIVE", "Never"),
        ],
    );
    assert_eq!(environment.get("GIT_TERMINAL_PROMPT").map(String::as_str), Some("0"));
    assert_eq!(environment.get("GCM_INTERACTIVE").map(String::as_str), Some("Auto"));
    assert_eq!(environment.get("SSH_AUTH_SOCK").map(String::as_str), Some("/safe/agent"));
    for key in ["GIT_ASKPASS", "SSH_ASKPASS", "GIT_SSH_COMMAND"] {
        assert!(!environment.contains_key(key));
    }
}

#[test]
fn background_network_policy_never_allows_interactive_credentials() {
    let environment = captured_environment(
        GitExecutionPolicy::BackgroundNetworkNonInteractive,
        [("PATH", "/safe/bin"), ("GCM_INTERACTIVE", "Auto")],
    );
    assert_eq!(environment.get("GCM_INTERACTIVE").map(String::as_str), Some("Never"));
}
```

- [x] **Step 2: Write failing cancellation and progress tests**

Use the existing child-test technique in `engine.rs` to launch a process that writes one progress line and waits. Assert `GitRunContext::cancel()` terminates the process tree before the absolute timeout and the sink receives only bounded chunks. Add parser tests:

```rust
#[test]
fn parses_receiving_and_writing_progress_without_retaining_raw_remote_text() {
    let mut parser = GitProgressParser::default();
    let events = parser.push(b"remote: secret text\nReceiving objects: 42% (42/100), 1.00 MiB | 2.00 MiB/s\r");
    assert!(events.iter().any(|event| event.stage == NetworkStage::Transferring && event.fraction == Some(0.42)));
    assert!(events.iter().all(|event| !format!("{event:?}").contains("secret text")));
}
```

For `TransportOperationRegistry`, assert duplicate Operation IDs and duplicate Repository resource keys are rejected, `cancel` flips the exact token, and `finish` releases both indexes.

- [x] **Step 3: Run focused tests and observe missing policy/context types**

```powershell
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml git::engine::tests::foreground_network_policy_allows_system_interaction_without_inheriting_attack_overrides -- --exact
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml git::transport::progress::tests
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml git::transport::operation::tests
```

Expected: FAIL because the policy, streaming context, parser, and registry do not exist.

- [x] **Step 4: Add object-safe run context support without breaking existing callers**

Keep `GitCommand` unchanged so current struct literals and `GitRunner` test doubles remain source-compatible. Add:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitExecutionPolicy {
    LocalNonInteractive,
    ForegroundNetworkInteractive,
    BackgroundNetworkNonInteractive,
}

pub trait GitProgressSink: Send + Sync {
    fn stderr_chunk(&self, chunk: &[u8]);
}

#[derive(Clone)]
pub struct GitRunContext {
    pub policy: GitExecutionPolicy,
    pub cancellation: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub progress: Option<std::sync::Arc<dyn GitProgressSink>>,
}

pub trait GitRunner: Send + Sync {
    fn run(&self, command: GitCommand) -> Result<GitOutput, AppError>;
    fn run_with_context(&self, command: GitCommand, context: GitRunContext) -> Result<GitOutput, AppError> {
        if context.cancellation.load(std::sync::atomic::Ordering::Acquire) {
            return Err(AppError::Canceled);
        }
        self.run(command)
    }
}
```

Add the internal `AppError::Canceled` variant in Task 4 and map it to a fixed generic cancellation envelope outside Transport; `GitTransportService` maps it to `TransportFailure::cancelled()` with operation/resource context. This keeps the generic process engine independent from the Transport domain.

Override `run_with_context` in `SystemGitRunner`. Refactor command construction to accept a policy, refactor stderr reading so each bounded chunk reaches `GitProgressSink`, and check cancellation in the existing process wait loop. Preserve current output caps and process-tree cleanup. `run()` delegates to the same internal path with `LocalNonInteractive`.

The environment builder must set `GCM_INTERACTIVE=Auto` only for foreground network policy. It still clears all inherited Git/AskPass/Credential overrides and sets `GIT_TERMINAL_PROMPT=0`. Do not set `StrictHostKeyChecking=no` or inherit `SSH_ASKPASS`.

- [x] **Step 5: Implement the bounded parser and operation registry**

`GitProgressParser` retains at most one 8 KiB incomplete line and emits only typed `NetworkProgress { stage, fraction, objects, bytes }`. Discard unrecognized raw lines. `TransportOperationRegistry` owns `Arc<AtomicBool>` tokens in maps by Operation ID and resource key; use an RAII registration guard so panic/error paths release indexes.

- [x] **Step 6: Run engine regressions and focused transport tests**

```powershell
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml git::engine::tests
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml git::transport::progress::tests
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml git::transport::operation::tests
```

Expected: all tests PASS, including existing Windows/Unix process-tree regressions.

- [x] **Step 7: Commit execution control**

```powershell
git add apps/desktop/src-tauri/src/git/engine.rs apps/desktop/src-tauri/src/git/mod.rs apps/desktop/src-tauri/src/git/transport/mod.rs apps/desktop/src-tauri/src/git/transport/progress.rs apps/desktop/src-tauri/src/git/transport/operation.rs
git commit -m "feat: add cancellable git network execution"
```

### Task 6: Implement Transport Profile lifecycle and compensating bindings

**Files:**

- Create: `apps/desktop/src-tauri/src/git/transport/profile_service.rs`
- Modify: `apps/desktop/src-tauri/src/git/transport/config.rs`
- Modify: `apps/desktop/src-tauri/src/git/transport/mod.rs`
- Test: `apps/desktop/src-tauri/src/git/transport/profile_service.rs`
- Test: `apps/desktop/src-tauri/tests/git_transport_integration.rs`

- [x] **Step 1: Write failing lifecycle and Trust tests**

Use a real temporary Git repository and sealed `SystemGitRunner`:

```rust
#[test]
fn binding_requires_trust_and_external_git_reads_the_applied_profile() {
    let fixture = TransportFixture::new();
    let profile = fixture.service.create_https_profile("Work HTTPS", "creator").unwrap();
    let error = fixture.service.bind_repository(&fixture.repository_id, &profile.id, false).unwrap_err();
    assert!(matches!(error, AppError::TrustRequired));

    fixture.trust();
    fixture.service.bind_repository(&fixture.repository_id, &profile.id, false).unwrap();
    assert_eq!(fixture.git_config("credential.useHttpPath").as_deref(), Some("true"));
    assert_eq!(fixture.service.effective_for_repository(&fixture.repository_id).unwrap().source, EffectiveTransportSource::Profile);
}

#[test]
fn switching_then_unbinding_restores_the_original_config_and_drift_blocks_restore() {
    let fixture = TransportFixture::with_local_config("credential.useHttpPath", "false");
    fixture.trust();
    let first = fixture.service.create_https_profile("One", "one").unwrap();
    let second = fixture.service.create_https_profile("Two", "two").unwrap();
    fixture.service.bind_repository(&fixture.repository_id, &first.id, true).unwrap();
    fixture.service.bind_repository(&fixture.repository_id, &second.id, true).unwrap();
    fixture.service.unbind_repository(&fixture.repository_id, DriftResolution::Reject).unwrap();
    assert_eq!(fixture.git_config("credential.useHttpPath").as_deref(), Some("false"));

    fixture.service.bind_repository(&fixture.repository_id, &first.id, true).unwrap();
    fixture.set_git_config("credential.useHttpPath", "external");
    let error = fixture.service.unbind_repository(&fixture.repository_id, DriftResolution::Reject).unwrap_err();
    assert!(matches!(error, AppError::Transport(failure) if failure.code() == "git.transport.config-drift"));
}
```

- [x] **Step 2: Run the focused tests and verify the missing service failure**

```powershell
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml git::transport::profile_service::tests
```

Expected: FAIL because `TransportProfileService` does not exist.

- [x] **Step 3: Implement profile CRUD and sanitized summaries**

`TransportProfileService` owns `TransportStore`, `RepositoryRepository`, `TrustRepository`, shared `RepositoryWriteLocks`, and an `Arc<dyn GitRunner>`. Implement typed constructors/update methods that canonicalize and validate SSH Key files in the Host, never accept arbitrary SSH options, and return `TransportProfileSummary` with only the filename. Prevent deletion while bindings remain unless a `ProfileDeletionResolution` covers every affected Repository exactly once.

- [x] **Step 4: Implement config read/apply/restore with compensation**

In `config.rs`, execute only these command shapes through argument arrays:

```text
git config --local --null --get-regexp <host-owned-regex>
git config --local --replace-all <host-owned-key> <host-owned-value>
git config --local --unset-all <host-owned-key>
```

Before writing, read the managed-key snapshot. If a binding does not exist and conflicting values exist, return a confirmation-required failure unless `replace_existing=true`. After each write, reread and compare the exact sorted map. On DB failure, restore the original map. On restore failure, insert `transport_config_repairs` and return `git.transport.partial`.

Switching profiles preserves the first binding's `before_config_json`. Unbind compares the live hash to `applied_config_hash`; `DriftResolution::Reject` returns Drift, `KeepExternal` deletes only the binding row, and `Reapply` writes the current Profile again.

- [x] **Step 5: Run focused and real-Git tests**

```powershell
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml git::transport::profile_service::tests
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --test git_transport_integration profile_
```

Expected: profile unit and integration tests PASS.

- [x] **Step 6: Commit profile management**

```powershell
git add apps/desktop/src-tauri/src/git/transport/config.rs apps/desktop/src-tauri/src/git/transport/mod.rs apps/desktop/src-tauri/src/git/transport/profile_service.rs apps/desktop/src-tauri/tests/git_transport_integration.rs
git commit -m "feat: manage git transport profiles"
```

### Task 7: Implement Fetch orchestration and post-operation refresh

**Files:**

- Create: `apps/desktop/src-tauri/src/git/transport/service.rs`
- Modify: `apps/desktop/src-tauri/src/git/transport/mod.rs`
- Modify: `apps/desktop/src-tauri/src/jobs/service.rs`
- Test: `apps/desktop/src-tauri/src/git/transport/service.rs`
- Test: `apps/desktop/src-tauri/tests/git_transport_integration.rs`

- [x] **Step 1: Write a failing real-Git Fetch test**

Build a fixture with a Bare Remote and a sealed test Global Config containing `url.<local-bare-path>.insteadOf=https://git.example.test/acme/repository.git`. The persisted Remote stays HTTPS.

```rust
#[test]
fn fetch_updates_remote_refs_and_persisted_ahead_behind() {
    let fixture = TransportFixture::with_https_bare_remote();
    fixture.advance_remote("remote-only.txt");
    let result = fixture.transport.fetch(FetchInput {
        repository_id: fixture.repository_id.clone(),
        context: fixture.project_context(),
        remote_name: "origin".to_owned(),
        operation_id: uuid::Uuid::new_v4().to_string(),
        interactive: true,
    }, fixture.progress()).unwrap();
    assert_eq!(result.remote_name.as_deref(), Some("origin"));
    let snapshot = fixture.git.get_snapshot(&fixture.project_id, &fixture.repository_id).unwrap();
    assert_eq!(snapshot.behind, 1);
}
```

Also add unit tests that untrusted repositories, unknown or option-looking Remotes, profile type mismatch, duplicate Operation IDs, and an already-held Repository Write Lock fail before spawning Git.

- [x] **Step 2: Run Fetch tests and observe the missing service failure**

```powershell
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --test git_transport_integration fetch_updates_remote_refs_and_persisted_ahead_behind -- --exact
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml git::transport::service::tests::fetch_rejects_untrusted_repository -- --exact
```

Expected: FAIL because `GitTransportService::fetch` is missing.

- [x] **Step 3: Add caller-owned Job IDs and interruption handling**

Add `JobService::create_with_id(id, kind, title)` that validates a UUID and refuses duplicates. Add `fail_running_by_kind_prefix("git.transport.", TransportFailure::interrupted().envelope())`, called during AppState construction in Task 10. Do not add new `JobStatus` variants.

- [x] **Step 4: Implement `GitTransportService` foundation and Fetch**

The service owns `Database`, `GitService`, `TransportProfileService`, `TransportStore`, `JobService`, `TransportOperationRegistry`, shared `RepositoryWriteLocks`, and `Arc<dyn GitRunner>`. `fetch` must:

1. Validate context and Trust.
2. Resolve the persisted Remote and classify its URL.
3. Resolve Effective Transport and reject mismatch.
4. Register Operation ID + Repository resource and acquire the shared write lock.
5. Create/start `git.transport.fetch` Job using the Operation ID.
6. Execute exactly `git fetch --progress -- <validated-remote-name>` with `ForegroundNetworkInteractive`; the argument separator is mandatory even though the strict contract also rejects option-looking Remote names.
7. Feed typed progress to the supplied reporter and JobService.
8. On every terminal path, drop the Transport-held Repository mutex guard before calling existing `GitService::refresh_repository`, which reacquires the same shared lock. Keep the Operation Registry resource guard until refresh completes so another Transport task cannot start; ordinary Git writes may run before the refresh and will then be reflected in the refreshed true state.
9. Return a stable `NetworkOperationResult`; classify raw Git failure internally and discard raw stderr after mapping.

Do not add `--prune`, retry, extra refspecs, or plugin-controlled timeout values.

- [x] **Step 5: Run Fetch tests, Job tests, and Clippy**

```powershell
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml jobs::service::tests
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml git::transport::service::tests
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --test git_transport_integration fetch_
cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets -- -D warnings
```

Expected: all tests PASS and Clippy exits 0.

- [x] **Step 6: Commit Fetch**

```powershell
git add apps/desktop/src-tauri/src/git/transport/mod.rs apps/desktop/src-tauri/src/git/transport/service.rs apps/desktop/src-tauri/src/jobs/service.rs apps/desktop/src-tauri/tests/git_transport_integration.rs
git commit -m "feat: fetch repository remotes"
```

### Task 8: Add fast-forward-only Pull and safe Push

**Files:**

- Modify: `apps/desktop/src-tauri/src/git/transport/service.rs`
- Modify: `apps/desktop/src-tauri/src/git/transport/model.rs`
- Test: `apps/desktop/src-tauri/src/git/transport/service.rs`
- Test: `apps/desktop/src-tauri/tests/git_transport_integration.rs`

- [x] **Step 1: Write failing Pull safety tests**

```rust
#[test]
fn pull_fast_forwards_but_divergence_never_creates_a_merge_or_rebase() {
    let fixture = TransportFixture::with_https_bare_remote();
    fixture.advance_remote("remote.txt");
    fixture.transport.pull(fixture.pull_input(), fixture.progress()).unwrap();
    assert!(fixture.worktree_path().join("remote.txt").is_file());

    fixture.commit_local("local.txt");
    fixture.advance_remote("other.txt");
    let before = fixture.head_oid();
    let error = fixture.transport.pull(fixture.pull_input(), fixture.progress()).unwrap_err();
    assert!(matches!(error, AppError::Transport(failure) if failure.code() == "git.transport.non-fast-forward"));
    assert_eq!(fixture.head_oid(), before);
    assert!(!fixture.git_dir().join("MERGE_HEAD").exists());
    assert!(!fixture.git_dir().join("rebase-merge").exists());
}
```

Add focused unit cases for detached HEAD, missing upstream, conflict count, and in-progress Merge/Rebase/Cherry-pick/Revert/Bisect markers.

- [x] **Step 2: Write failing Push target and non-force tests**

```rust
#[test]
fn push_sets_upstream_once_and_rejects_non_fast_forward_without_force() {
    let fixture = TransportFixture::with_untracked_local_branch();
    fixture.transport.push(PushInput {
        target: Some(PushTarget { remote_name: "origin".into(), branch_name: "feature/safe".into() }),
        ..fixture.push_input()
    }, fixture.progress()).unwrap();
    assert_eq!(fixture.upstream().as_deref(), Some("origin/feature/safe"));

    fixture.rewrite_remote_branch("feature/safe");
    let error = fixture.transport.push(PushInput { target: None, ..fixture.push_input() }, fixture.progress()).unwrap_err();
    assert!(matches!(error, AppError::Transport(failure) if failure.code() == "git.transport.non-fast-forward"));
    assert!(fixture.captured_git_args().iter().all(|arg| arg != "--force" && arg != "--force-with-lease"));
}
```

- [x] **Step 3: Run focused tests and observe missing methods**

```powershell
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --test git_transport_integration pull_fast_forwards_but_divergence_never_creates_a_merge_or_rebase -- --exact
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --test git_transport_integration push_sets_upstream_once_and_rejects_non_fast_forward_without_force -- --exact
```

Expected: FAIL because Pull/Push are not implemented.

- [x] **Step 4: Implement machine-readable preflight**

Use argument-array Git probes only:

- `symbolic-ref --quiet --short HEAD` for local Branch.
- `rev-parse --abbrev-ref --symbolic-full-name @{upstream}` for upstream.
- existing Snapshot conflict count plus Git-dir marker checks for in-progress operations.
- `check-ref-format --branch <candidate>` for a user-selected Push branch.

Expose the same probes through `GitTransportService::network_state(context, repository_id) -> RepositoryNetworkState`; UI never guesses detached/in-progress state. Never infer these states from localized human `git status` text.

- [x] **Step 5: Implement Pull and Push command construction**

Pull executes exactly `git pull --ff-only --progress` after preflight and uses the same operation/job/lock/refresh lifecycle as Fetch. Dirty worktrees are passed to Git; no Stash command is allowed.

Push with upstream resolves the stored upstream into a validated Remote plus Branch and executes `git push --progress -- <remote> HEAD:refs/heads/<validated-upstream-branch>`. Without upstream, validate a selected Remote and Branch, then execute `git push --progress --set-upstream -- <remote> HEAD:refs/heads/<validated-branch>`. The argument separator is mandatory. No public input type contains a raw RefSpec, and no code path adds Force/Delete/Mirror/Tags/All.

- [x] **Step 6: Run service and real-Git tests**

```powershell
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml git::transport::service::tests
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --test git_transport_integration pull_
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --test git_transport_integration push_
```

Expected: Pull/Push safety tests PASS.

- [x] **Step 7: Commit Pull and Push**

```powershell
git add apps/desktop/src-tauri/src/git/transport/model.rs apps/desktop/src-tauri/src/git/transport/service.rs apps/desktop/src-tauri/tests/git_transport_integration.rs
git commit -m "feat: pull and push repository branches"
```

### Task 9: Implement guarded Clone, intent resolution, and recovery

**Files:**

- Create: `apps/desktop/src-tauri/src/git/transport/clone.rs`
- Modify: `apps/desktop/src-tauri/src/git/transport/mod.rs`
- Modify: `apps/desktop/src-tauri/src/git/transport/service.rs`
- Modify: `apps/desktop/src-tauri/src/providers/service.rs`
- Test: `apps/desktop/src-tauri/src/git/transport/clone.rs`
- Test: `apps/desktop/src-tauri/tests/git_transport_integration.rs`

- [x] **Step 1: Write failing path ownership and Cleanup tests**

```rust
#[test]
fn clone_owner_marker_is_a_sidecar_and_cleanup_requires_exact_operation_ownership() {
    let parent = tempfile::tempdir().unwrap();
    let paths = ClonePaths::allocate(parent.path(), "repository", "b95c216a-dac4-45d1-8169-8dbfbc0c0315").unwrap();
    assert!(!paths.staging.exists());
    assert_eq!(paths.marker.parent(), Some(parent.path()));
    paths.write_marker().unwrap();
    std::fs::create_dir(&paths.staging).unwrap();
    assert!(paths.cleanup_owned_staging().is_ok());

    let foreign = parent.path().join(".git-ramus-clone-foreign");
    std::fs::create_dir(&foreign).unwrap();
    assert!(cleanup_staging(parent.path(), &foreign, &paths.marker, "other-operation").is_err());
    assert!(foreign.exists());
}
```

Add Windows reparse-point/Unix symlink cases and a race where Final Path appears before rename.

- [x] **Step 2: Write a failing end-to-end Clone integration test**

```rust
#[test]
fn clone_uses_staging_registers_project_and_applies_profile_without_leaking_provider_pat() {
    let fixture = CloneFixture::with_provider_source_and_https_bare_remote();
    let result = fixture.transport.clone_repository(CloneInput {
        source: CloneSource::Intent(fixture.intent_id.clone()),
        transport_kind: TransportKind::Https,
        profile_id: Some(fixture.https_profile_id.clone()),
        destination_parent: fixture.project_root.clone(),
        folder_name: "cloned-repository".into(),
        project_target: CloneProjectTarget::Existing { project_id: fixture.project_id.clone() },
        operation_id: uuid::Uuid::new_v4().to_string(),
        interactive: true,
    }, fixture.progress()).unwrap();
    assert!(fixture.project_root.join("cloned-repository/.git").is_dir());
    assert_eq!(result.project.id, fixture.project_id);
    assert_eq!(fixture.local_git_config(&result.repository, "credential.useHttpPath").as_deref(), Some("true"));
    let serialized = serde_json::to_string(&result).unwrap();
    assert!(!serialized.contains("provider-pat-fixture"));
}
```

Add failures for unsupported URL, excluded/deeper-than-scan destination, nonempty Final Path, checkout Filter/Hook attack, cancellation, registration failure after Final Rename, and Provider Binding partial failure.

- [x] **Step 3: Run Clone tests and observe missing coordinator failure**

```powershell
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml git::transport::clone::tests
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --test git_transport_integration clone_
```

Expected: FAIL because guarded Clone and intent resolution do not exist.

- [x] **Step 4: Implement an in-memory one-use Clone intent registry**

`CloneIntentRegistry` stores a `CloneIntentRecord` by UUID with caller Plugin ID, Account ID, validated `RemoteRepository`, creation time, and consumed flag. Creation calls a new `ProviderService::repository_for_clone(account_id, repository_id)` that reads the account Secret internally, asks the registered Adapter for that exact Repository, then returns only validated repository metadata to the registry. It never returns or stores the PAT.

Intent rules:

- TTL is 10 minutes using an injectable Clock.
- Only `git-ramus.git-client` can consume.
- Consume is atomic and one-use.
- Cancel/expiry removes the record.
- `https_url`/`ssh_url` are Adapter-validated and rechecked by transport URL validation before Git execution.

- [x] **Step 5: Implement guarded two-stage Clone**

Implement the exact sequence from design section 10:

1. Validate Project target and destination parent authorization.
2. Create caller-owned Job and persisted Clone Operation.
3. Compute absent Staging and sidecar Owner Marker; write Marker before Git creates Staging.
4. Build command-scoped profile config with Host-owned `-c` entries.
5. Run `git clone --no-checkout --no-recurse-submodules --progress -- <url> <staging>`; the argument separator is mandatory.
6. Validate Repository, `origin`, `.git`, HEAD, and tree.
7. Write temporary highest-priority `.git/info/attributes` rules that unset `filter`, `diff`, `merge`, and `working-tree-encoding`; run initial checkout with Host-owned empty `core.hooksPath`; remove the temporary file in a guard.
8. Atomically rename Staging to Final Path.
9. Register/scan Repository, record Trust, bind Profile, and bind Provider `origin` when applicable.
10. Remove Owner Marker only after recovery flags are durable.

After Final Rename, failures return `git.transport.partial` and never delete Final Path. Before Final Rename, Cleanup requires canonical parent, exact prefix, non-symlink/non-reparse Staging, matching sidecar Marker, and exact Operation ID.

- [x] **Step 6: Implement startup recovery classification**

For every incomplete Clone record:

- Staging exists + matching Marker + Final absent → action `cleanupStaging` or `retryClone`.
- Final exists + filesystem complete → action `retryRegistration`; never cleanup Final.
- Marker exists but neither path exists → delete stale Marker and fail the Job as interrupted.
- Any mismatch → `unsafePath`, preserve filesystem, require diagnostics.

- [x] **Step 7: Run Clone, Provider, and Git integration tests**

```powershell
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml git::transport::clone::tests
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --test git_transport_integration clone_
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml providers::
```

Expected: all tests PASS and existing Provider tests remain green.

- [x] **Step 8: Commit Clone**

```powershell
git add apps/desktop/src-tauri/src/git/transport/clone.rs apps/desktop/src-tauri/src/git/transport/mod.rs apps/desktop/src-tauri/src/git/transport/service.rs apps/desktop/src-tauri/src/providers/service.rs apps/desktop/src-tauri/tests/git_transport_integration.rs
git commit -m "feat: clone repositories safely"
```

### Task 10: Compose transport services and expose typed Tauri commands

**Files:**

- Modify: `apps/desktop/src-tauri/src/app_state.rs`
- Modify: `apps/desktop/src-tauri/src/commands.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Modify: `apps/desktop/src-tauri/src/git/transport/mod.rs`
- Test: `apps/desktop/src-tauri/src/commands.rs`
- Test: `apps/desktop/src-tauri/src/app_state.rs`
- Test: `apps/desktop/src-tauri/src/lib.rs`

- [x] **Step 1: Write failing command-boundary tests**

Add pure command-core tests that do not require a Tauri runtime:

```rust
#[test]
fn plugin_transport_requests_cannot_enable_interaction_without_host_confirmation() {
    assert!(ensure_interactive_network_allowed("git-ramus.git-client", false).is_err());
    assert!(ensure_interactive_network_allowed("external.example", false).is_err());
    assert!(ensure_interactive_network_allowed("git-ramus.git-client", true).is_ok());
}

#[test]
fn clone_native_request_requires_host_injected_absolute_parent() {
    let request = GitCloneNativeRequest {
        plugin_id: "git-ramus.git-client".into(),
        source: CloneSourceRequest::Manual { remote_url: "https://git.example.test/acme/repo.git".into() },
        transport_kind: TransportKind::Https,
        profile_id: None,
        destination_parent: "relative/path".into(),
        folder_name: "repo".into(),
        project_target: CloneProjectTarget::New { name: "Repo".into() },
        operation_id: uuid::Uuid::new_v4().to_string(),
        interactive_confirmed: true,
    };
    assert!(validate_clone_native_request(&request).is_err());
}
```

Add a `lib.rs` probe asserting the transport E2E command is enabled only under `all(feature = "e2e", debug_assertions)`; Task 15 registers the actual command.

- [x] **Step 2: Run focused tests and observe missing AppState/command fields**

```powershell
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml commands::tests::plugin_transport_requests_cannot_enable_interaction_without_host_confirmation -- --exact
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml app_state::tests::bootstrap_marks_interrupted_transport_jobs -- --exact
```

Expected: FAIL because transport composition and commands are missing.

- [x] **Step 3: Compose services with one shared repository lock set**

Add `pub transport: GitTransportService` to `AppState`. Construct one `RepositoryWriteLocks`, then pass clones to `GitService`, `IdentityService`, `TransportProfileService`, and `GitTransportService`. Production uses `SystemGitRunner::new()`; Debug E2E injection is added only in Task 15. Build `CloneIntentRegistry` and `TransportOperationRegistry` before the service. On bootstrap:

1. Fail stale running `git.transport.*` Jobs with `git.transport.interrupted`.
2. Ask Clone recovery to classify incomplete records without deleting anything automatically.
3. Continue existing Provider secret cleanup and identity import.

- [x] **Step 4: Add exact native command DTOs and commands**

Every deserialized DTO uses `#[serde(rename_all = "camelCase", deny_unknown_fields)]`. Add and register:

```text
git_transport_profile_list
git_transport_profile_create
git_transport_profile_update
git_transport_profile_deletion_impact
git_transport_profile_delete
git_transport_select_destination_parent
git_transport_select_ssh_key
git_repository_effective_transport
git_repository_network_state
git_repository_bind_transport
git_repository_unbind_transport
git_clone_intent_create
git_clone_intent_get
git_repository_clone
git_repository_fetch
git_repository_pull
git_repository_push
git_transport_operation_cancel
```

`git_transport_select_destination_parent` and `git_transport_select_ssh_key` are trusted main-window commands. Production implementations use `tauri_plugin_dialog::DialogExt`; they return paths only to the trusted HostApi, never to plugin RPC. Profile create/update native DTOs may contain the Host-injected SSH Key Path; public plugin contracts may not. Clone native DTO contains Host-injected Destination Parent and `interactive_confirmed`; no Provider PAT field exists.

Long operations are `async` Tauri commands that run blocking Git work through `tauri::async_runtime::spawn_blocking`, await completion, and emit `job://updated` at create/start/progress/terminal transitions. Commands pass a progress reporter closure into the service. Cancellation calls `TransportOperationRegistry::cancel(operation_id)` before transitioning the Job; it is idempotent for already terminal operations.

- [x] **Step 5: Enforce command caller and interaction rules**

- Profile/key/destination operations require signed built-in caller checks against `PluginRegistry`.
- Provider Center can call only `git_clone_intent_create`, after exact Provider account permission validation.
- Git Client consumes intents and executes network operations.
- External plugins with future grants still require `interactive_confirmed=true`, which only trusted Host API injects after a trusted dialog.
- Commands independently revalidate permission/resource/caller even though RPC Router also checks.

- [x] **Step 6: Run Rust unit and integration suites**

```powershell
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml commands::tests
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml app_state::tests
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --test git_transport_integration
cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets -- -D warnings
```

Expected: commands, bootstrap, transport integration, and Clippy PASS.

- [x] **Step 7: Commit native composition**

```powershell
git add apps/desktop/src-tauri/src/app_state.rs apps/desktop/src-tauri/src/commands.rs apps/desktop/src-tauri/src/lib.rs apps/desktop/src-tauri/src/git/transport/mod.rs
git commit -m "feat: expose git transport commands"
```

### Task 11: Add trusted transport prompts, native pickers, navigation, and RPC routes

**Files:**

- Create: `apps/desktop/src/git-transport/promptPorts.ts`
- Create: `apps/desktop/src/git-transport/promptBroker.ts`
- Create: `apps/desktop/src/git-transport/TransportConfirmationDialog.tsx`
- Create: `apps/desktop/src/git-transport/cloneNavigationBroker.ts`
- Create: `apps/desktop/src/git-transport/__tests__/transportPrompts.test.tsx`
- Modify: `apps/desktop/src/lib/hostApi.ts`
- Modify: `apps/desktop/src/lib/__tests__/hostApi.test.ts`
- Modify: `apps/desktop/src/plugins/rpcRouter.ts`
- Modify: `apps/desktop/src/plugins/__tests__/rpcRouter.test.ts`
- Modify: `apps/desktop/src/App.tsx`
- Modify: `apps/desktop/src/shell/AppShell.tsx`
- Modify: `apps/desktop/src/app.css`
- Modify: `apps/desktop/src/__tests__/App.test.tsx`

- [x] **Step 1: Write failing trusted-prompt tests**

```tsx
it("serializes transport confirmations and clears source details after resolution", async () => {
  render(<TransportConfirmationDialog broker={transportPromptBroker} />);
  const first = transportPromptBroker.confirm({ kind: "network", operation: "fetch", resourceLabel: "origin" });
  const second = transportPromptBroker.confirm({ kind: "sourceTrust", operation: "clone", resourceLabel: "git.example.test/acme/repo" });
  expect(await screen.findByRole("alertdialog", { name: "Confirm Git network operation" })).toBeInTheDocument();
  await userEvent.click(screen.getByRole("button", { name: "Continue" }));
  await expect(first).resolves.toBe(true);
  expect(await screen.findByText("git.example.test/acme/repo")).toBeInTheDocument();
  await userEvent.click(screen.getByRole("button", { name: "Cancel" }));
  await expect(second).resolves.toBe(false);
  expect(screen.queryByText("git.example.test/acme/repo")).not.toBeInTheDocument();
});
```

Add unmount rejection, one shared gate, stale resolution, and no secret fields in listener payloads.

- [x] **Step 2: Write failing Host API payload tests**

Mock Tauri `invoke`, trusted prompts, and navigation broker. Assert:

```ts
it("injects destination path only after trusted selection and never returns it to the plugin", async () => {
  prompts.confirm.mockResolvedValue(true);
  invokeMock.mockImplementation(async (command: string) => {
    if (command === "git_transport_select_destination_parent") return "D:/Projects";
    if (command === "git_repository_clone") return cloneResultFixture;
    throw new Error(`unexpected command: ${command}`);
  });
  const result = await hostApi.cloneRepository("git-ramus.git-client", clonePluginRequest);
  expect(invokeMock).toHaveBeenCalledWith("git_repository_clone", {
    request: expect.objectContaining({
      pluginId: "git-ramus.git-client",
      destinationParent: "D:/Projects",
      interactiveConfirmed: true
    })
  });
  expect(JSON.stringify(clonePluginRequest)).not.toContain("D:/Projects");
  expect(JSON.stringify(result)).not.toContain("D:/Projects");
});

it("publishes a Git Client clone route after a Provider intent is created", async () => {
  invokeMock.mockResolvedValue({ intentId });
  await hostApi.createCloneIntent("git-ramus.provider-center", { accountId, repositoryId: "42" });
  expect(cloneNavigationBroker.publish).toHaveBeenCalledWith(`/clone/${intentId}`);
});
```

- [x] **Step 3: Run desktop tests and observe missing brokers/routes**

```powershell
npm run test --workspace @git-ramus/desktop -- src/git-transport/__tests__/transportPrompts.test.tsx
npm run test --workspace @git-ramus/desktop -- src/lib/__tests__/hostApi.test.ts
npm run test --workspace @git-ramus/desktop -- src/plugins/__tests__/rpcRouter.test.ts
```

Expected: FAIL because trusted transport dependencies and routes do not exist.

- [x] **Step 4: Implement trusted ports and brokers**

Define:

```ts
export interface GitTransportPromptPort {
  confirm(request: {
    kind: "network" | "sourceTrust" | "replaceConfig";
    operation: "clone" | "fetch" | "pull" | "push" | "bindProfile";
    resourceLabel: string;
  }): Promise<boolean>;
}

export interface GitTransportFilePort {
  selectDestinationParent(defaultPath?: string): Promise<string | null>;
  selectSshPrivateKey(): Promise<string | null>;
}
```

Use the same atomic gate discipline as Provider prompts: acquire once, resolve/cancel/unmount clears state before settling, and never retain previous request metadata. The production `GitTransportFilePort` calls only `git_transport_select_destination_parent` and `git_transport_select_ssh_key`; test ports return deterministic values. `cloneNavigationBroker` supports one current navigation event and consumes it after `App` selects plugin `git-ramus.git-client` with the exact `/clone/<uuid>` route.

- [x] **Step 5: Extend HostApi with strict transport methods**

Import every new Schema. Public HostApi methods accept plugin-safe request types. For SSH Profile create/update, `sshKeyAction=selectFile` opens the trusted key picker and injects the path only into the native invoke. For Clone, confirm source/network, select Destination Parent, and inject path/confirmation only into native invoke. If any trusted interaction is canceled, return `null` without invoking Rust.

Provider intent creation invokes Rust with only `{ pluginId, accountId, repositoryId }`, then publishes the Clone route. Network methods confirm through the broker before invoking with `interactiveConfirmed: true`. Parse every native response before returning it.

- [x] **Step 6: Add permission-aware RPC routes**

Add `RPC_RESOURCES.transportProfiles = "transport-profiles"` and `RPC_RESOURCES.cloneIntents = "clone-intents"`. Route capabilities use the corrected Manifest form:

```text
git.transport:read
git.transport:manage
git.network:execute
```

Add exact methods matching Task 10, including `repositories.getNetworkState`. Provider intent creation requires both an authorized Provider Account (`providers:read`) and `git.network:execute` on `clone-intents`. Repository operations require `git.network:execute` plus `repositories:read`/`repositories:write` on the fixed repository resource. No route accepts `destinationParent`, `sshKeyPath`, `environment`, `args`, `refspec`, or a generic `path` field.

- [x] **Step 7: Mount dialogs outside plugin iframes and wire Clone navigation**

`App` subscribes to `cloneNavigationBroker` and selects `git-ramus.git-client` at the supplied route. Render `TransportConfirmationDialog` in `AppShell` alongside existing trusted Provider dialogs, never inside `PluginHost`. Add semantic-token CSS and an App test asserting the dialog and plugin iframe are siblings under trusted Shell ownership.

- [x] **Step 8: Run all desktop Host tests and typecheck**

```powershell
npm run test --workspace @git-ramus/desktop
npm run typecheck --workspace @git-ramus/desktop
```

Expected: desktop tests and typecheck PASS.

- [x] **Step 9: Commit trusted Host transport flow**

```powershell
git add apps/desktop/src/git-transport apps/desktop/src/lib/hostApi.ts apps/desktop/src/lib/__tests__/hostApi.test.ts apps/desktop/src/plugins/rpcRouter.ts apps/desktop/src/plugins/__tests__/rpcRouter.test.ts apps/desktop/src/App.tsx apps/desktop/src/shell/AppShell.tsx apps/desktop/src/app.css apps/desktop/src/__tests__/App.test.tsx
git commit -m "feat: broker trusted git transport actions"
```

### Task 12: Build Transport Identity management in Git Client

**Files:**

- Modify: `plugins/git-client/plugin.json`
- Modify: `plugins/git-client/src/api.ts`
- Modify: `plugins/git-client/src/App.tsx`
- Create: `plugins/git-client/src/views/TransportProfilesView.tsx`
- Create: `plugins/git-client/src/components/TransportProfileForm.tsx`
- Create: `plugins/git-client/src/__tests__/TransportProfilesView.test.tsx`
- Modify: `plugins/git-client/src/style.css`

- [x] **Step 1: Write failing API and view tests**

Add API route assertions and this user flow:

```tsx
it("creates HTTPS and SSH profiles without rendering a secret or full key path", async () => {
  const api = transportProfileApi();
  render(<TransportProfilesView api={api} />);
  await screen.findByText("System Git configuration");
  await userEvent.click(screen.getByRole("button", { name: "New HTTPS profile" }));
  await userEvent.type(screen.getByLabelText("Profile name"), "Work HTTPS");
  await userEvent.type(screen.getByLabelText("HTTPS username"), "creator");
  await userEvent.click(screen.getByRole("button", { name: "Save profile" }));
  expect(api.createTransportProfile).toHaveBeenCalledWith({
    kind: "https",
    displayName: "Work HTTPS",
    username: "creator",
    useHttpPath: true
  });

  api.listTransportProfiles.mockResolvedValue({ items: [sshProfileSummary] });
  await userEvent.click(screen.getByRole("button", { name: "Refresh profiles" }));
  expect(await screen.findByText("id_ed25519")).toBeInTheDocument();
  expect(screen.queryByText(/Users.*\.ssh/u)).not.toBeInTheDocument();
  expect(screen.queryByLabelText(/password|token|passphrase/iu)).not.toBeInTheDocument();
});
```

Add deletion-impact tests requiring every affected Repository to choose replacement or unbind before delete.

- [x] **Step 2: Run plugin tests and observe missing methods/views**

```powershell
npm run test --workspace @git-ramus/git-client -- src/__tests__/TransportProfilesView.test.tsx
```

Expected: FAIL because the API and views do not exist.

- [x] **Step 3: Extend the typed plugin API**

Add methods for profile list/create/update/deletion impact/delete, effective transport, Repository Network State, bind/unbind, Clone intent read, Clone, Fetch/Pull/Push, and cancel. Every method validates request and response with transport schemas and calls the exact RPC names from Task 11. Use a shared cancellable helper keyed by `operationId`; cancellation calls `repositories.cancelNetworkOperation` and rejects with `AbortError` without swallowing the original request rejection.

- [x] **Step 4: Add Manifest permissions and navigation**

Add navigation:

```json
{
  "id": "transport-identities",
  "label": "Transport identities",
  "route": "/transport-identities",
  "icon": "key-round"
}
```

Add `git.transport:read`, `git.transport:manage`, and `git.network:execute` permissions with resources `transport-profiles`, `repositories`, and `clone-intents` as appropriate. Do not grant Provider Account read permission to Git Client.

- [x] **Step 5: Implement focused profile components**

`TransportProfileForm` is a discriminated SSH/HTTPS form. SSH presents Name, `IdentitiesOnly`, and a “Choose private key” action; it never accepts a text path. HTTPS presents Name and Username, with `useHttpPath` fixed on and explained. `TransportProfilesView` owns list/lifecycle/delete-impact state and stale-response generations. It displays Key Filename only and uses existing `ErrorNotice` conventions.

- [x] **Step 6: Run Git Client tests, typecheck, and build**

```powershell
npm run test --workspace @git-ramus/git-client
npm run typecheck --workspace @git-ramus/git-client
npm run build --workspace @git-ramus/git-client
```

Expected: all Git Client checks PASS.

- [x] **Step 7: Commit Transport Identity UI**

```powershell
git add plugins/git-client/plugin.json plugins/git-client/src/api.ts plugins/git-client/src/App.tsx plugins/git-client/src/views/TransportProfilesView.tsx plugins/git-client/src/components/TransportProfileForm.tsx plugins/git-client/src/__tests__/TransportProfilesView.test.tsx plugins/git-client/src/style.css
git commit -m "feat: manage transport identities in git client"
```

### Task 13: Build Clone and Repository Network UI

**Files:**

- Modify: `plugins/git-client/src/App.tsx`
- Create: `plugins/git-client/src/views/CloneView.tsx`
- Create: `plugins/git-client/src/components/RepositoryNetworkPanel.tsx`
- Create: `plugins/git-client/src/__tests__/CloneView.test.tsx`
- Create: `plugins/git-client/src/__tests__/RepositoryNetworkPanel.test.tsx`
- Modify: `plugins/git-client/src/views/RepositoryView.tsx`
- Modify: `plugins/git-client/src/__tests__/RepositoryView.test.tsx`
- Modify: `plugins/git-client/src/style.css`

- [x] **Step 1: Write failing Clone wizard tests**

```tsx
it("consumes a Provider intent and submits a path-free Clone request", async () => {
  const api = cloneApi({ intent: providerCloneIntent, projects: [existingProject] });
  render(<CloneView api={api} intentId={providerCloneIntent.id} />);
  expect(await screen.findByText("skills/private-skill")).toBeInTheDocument();
  await userEvent.click(screen.getByLabelText("SSH"));
  await userEvent.selectOptions(screen.getByLabelText("Transport profile"), sshProfile.id);
  await userEvent.selectOptions(screen.getByLabelText("Project"), existingProject.id);
  await userEvent.click(screen.getByRole("button", { name: "Clone repository" }));
  expect(api.cloneRepository).toHaveBeenCalledWith(expect.objectContaining({
    source: { kind: "intent", intentId: providerCloneIntent.id },
    transportKind: "ssh",
    profileId: sshProfile.id,
    projectTarget: { kind: "existing", projectId: existingProject.id }
  }), expect.any(AbortSignal));
  expect(JSON.stringify(api.cloneRepository.mock.calls[0])).not.toMatch(/[A-Za-z]:[\\/]|\/home\//u);
});
```

Add manual URL, new Project, unsafe folder, user cancel, operation cancel, stale intent, Partial registration recovery, and success navigation cases.

- [x] **Step 2: Write failing Repository Network panel tests**

```tsx
it("disables unsafe Pull and asks for a Push target only when upstream is absent", async () => {
  const api = networkApi({ branch: "main", upstream: null, conflictedCount: 0 });
  render(<RepositoryNetworkPanel api={api} repository={repository} context={{ projectId, workspaceId: null }} trusted />);
  expect(screen.getByRole("button", { name: "Pull" })).toBeDisabled();
  await userEvent.click(screen.getByRole("button", { name: "Push" }));
  await userEvent.selectOptions(screen.getByLabelText("Remote"), "origin");
  await userEvent.type(screen.getByLabelText("Remote branch"), "main");
  await userEvent.click(screen.getByRole("button", { name: "Set upstream and push" }));
  expect(api.pushRepository).toHaveBeenCalledWith(expect.objectContaining({
    target: { remoteName: "origin", branchName: "main" }
  }), expect.any(AbortSignal));
});
```

Add Fetch Remote choice, ff-only divergence message, Drift actions, operation cancellation, duplicate-click exclusion, and post-terminal refresh tests.

- [x] **Step 3: Run focused tests and observe missing components**

```powershell
npm run test --workspace @git-ramus/git-client -- src/__tests__/CloneView.test.tsx
npm run test --workspace @git-ramus/git-client -- src/__tests__/RepositoryNetworkPanel.test.tsx
```

Expected: FAIL because Clone/Network components are missing.

- [x] **Step 4: Implement Clone routing and wizard**

`App` recognizes exact `/clone` and `/clone/<uuid>` routes before the regular navigation switch. Validate the UUID using the contract before requesting an intent. `CloneView` owns a single operation generation and `AbortController`, lists Projects/Profiles, derives allowed transport types from the intent, never handles local paths, and displays operation stages from local request state plus Task summaries. A success callback opens the returned Repository in its Project context; Partial errors expose only server-provided recovery actions.

- [x] **Step 5: Implement a focused Network panel**

`RepositoryNetworkPanel` loads Effective Transport and the existing Snapshot/Remote summary. It owns Fetch/Pull/Push and cancellation state; `RepositoryView` passes the Repository, context, trust state, and a post-operation `refresh` callback. Keep Changes/Diff/Commit state in `RepositoryView`; do not copy it into the Network component.

Pull is disabled for detached HEAD, no upstream, conflict/in-progress flags, untrusted Repository, or busy state. Push without upstream opens the target form. No Force/Delete/Tags controls are rendered. The panel displays sanitized URLs only.

- [x] **Step 6: Run Git Client tests and build**

```powershell
npm run test --workspace @git-ramus/git-client
npm run typecheck --workspace @git-ramus/git-client
npm run build --workspace @git-ramus/git-client
```

Expected: all Git Client tests and build PASS.

- [x] **Step 7: Commit Clone and Network UI**

```powershell
git add plugins/git-client/src/App.tsx plugins/git-client/src/views/CloneView.tsx plugins/git-client/src/components/RepositoryNetworkPanel.tsx plugins/git-client/src/__tests__/CloneView.test.tsx plugins/git-client/src/__tests__/RepositoryNetworkPanel.test.tsx plugins/git-client/src/views/RepositoryView.tsx plugins/git-client/src/__tests__/RepositoryView.test.tsx plugins/git-client/src/style.css
git commit -m "feat: add clone and repository network views"
```

### Task 14: Connect Provider Center repository results to Clone intents

**Files:**

- Modify: `plugins/provider-center/plugin.json`
- Modify: `plugins/provider-center/src/api.ts`
- Modify: `plugins/provider-center/src/components/RepositoryBrowser.tsx`
- Test: `plugins/provider-center/src/__tests__/api.test.ts`
- Test: `plugins/provider-center/src/__tests__/RepositoryBrowser.test.tsx`
- Modify: `plugins/provider-center/src/style.css`

- [x] **Step 1: Write failing API boundary tests**

```ts
it("creates a Clone intent with repository identity only", async () => {
  const client = pluginClient({ intentId: "90e1e991-f93e-4e78-817e-d0ceeb06a749" });
  const api = createProviderCenterApi(client);
  await api.createCloneIntent(accountId, "4242");
  expect(client.request).toHaveBeenCalledWith("repositories.createCloneIntent", {
    accountId,
    repositoryId: "4242"
  });
  const payload = JSON.stringify(client.request.mock.calls[0]);
  for (const forbidden of ["pat", "secret", "sshKeyPath", "destination", "C:\\\\", "/home/"]) {
    expect(payload).not.toContain(forbidden);
  }
});
```

- [x] **Step 2: Write a failing Repository Browser Clone-action test**

Render a writable Provider repository, click `Clone skills/private-skill`, and assert `api.createCloneIntent(account.id, repository.repositoryId)` is called exactly once. Add disabled states for archived/no-read-access entries, duplicate clicks, stale account changes, and API failure surfaced through `ErrorNotice`.

- [x] **Step 3: Run Provider Center tests and observe missing behavior**

```powershell
npm run test --workspace @git-ramus/provider-center -- src/__tests__/api.test.ts src/__tests__/RepositoryBrowser.test.tsx
```

Expected: FAIL because `createCloneIntent` and the Clone button do not exist.

- [x] **Step 4: Add the exact capability and API method**

Add this Manifest permission only:

```json
{
  "capability": "git.network:execute",
  "resources": ["clone-intents"]
}
```

Do not add transport-profile management, repository write, or local filesystem permissions to Provider Center. Implement `createCloneIntent(accountId, repositoryId)` using `providerCloneIntentCreateRequestSchema` and `cloneIntentReferenceSchema`.

- [x] **Step 5: Add the Clone action with stale-request protection**

Pass the selected Account into each action. Keep a generation/ref keyed by Account ID; if the account changes while intent creation is in flight, discard the result/error for the previous account. Disable only the clicked repository while creating its intent. Host navigation happens through Task 11 after successful RPC; Provider Center must not parse or construct the Git Client route.

- [x] **Step 6: Run Provider tests, typecheck, and build**

```powershell
npm run test --workspace @git-ramus/provider-center
npm run typecheck --workspace @git-ramus/provider-center
npm run build --workspace @git-ramus/provider-center
```

Expected: all Provider Center checks PASS.

- [x] **Step 7: Commit Provider Clone handoff**

```powershell
git add plugins/provider-center/plugin.json plugins/provider-center/src/api.ts plugins/provider-center/src/components/RepositoryBrowser.tsx plugins/provider-center/src/__tests__/api.test.ts plugins/provider-center/src/__tests__/RepositoryBrowser.test.tsx plugins/provider-center/src/style.css
git commit -m "feat: hand provider repositories to clone"
```

### Task 15: Add deterministic native Transport E2E and prove the release boundary

**Files:**

- Modify: `apps/desktop/src-tauri/src/e2e.rs`
- Modify: `apps/desktop/src-tauri/src/app_state.rs`
- Modify: `apps/desktop/src-tauri/src/git/transport/service.rs`
- Modify: `apps/desktop/src-tauri/src/lib.rs`
- Create: `apps/desktop/e2e/fixture-transport.ts`
- Create: `apps/desktop/e2e/git-transport.e2e.ts`
- Modify: `apps/desktop/e2e/wdio.conf.ts`
- Modify: `apps/desktop/package.json`
- Modify: `.github/workflows/ci.yml`
- Test: `apps/desktop/src-tauri/src/e2e.rs`
- Test: `apps/desktop/src-tauri/src/lib.rs`

- [x] **Step 1: Write failing Rust fixture-boundary tests**

```rust
#[test]
fn e2e_transport_fixture_handler_matches_the_debug_feature_boundary() {
    assert_eq!(
        super::e2e_transport_fixture_handler_enabled(),
        cfg!(all(feature = "e2e", debug_assertions))
    );
}

#[cfg(all(feature = "e2e", debug_assertions))]
#[test]
fn transport_fixture_uses_guarded_temp_paths_and_a_sealed_git_rewrite() {
    let fixture = seed_transport_fixture().unwrap();
    assert!(fixture.root_path.file_name().unwrap().to_string_lossy().starts_with("git-ramus-e2e-transport-"));
    assert!(fixture.bare_remote.join("HEAD").is_file());
    assert_eq!(fixture.public_url, "https://gitlab.example.test/skills/private-skill.git");
    assert!(!std::fs::read_to_string(&fixture.sealed_global_config).unwrap().contains("provider-pat"));
    cleanup_transport_fixture(&fixture).unwrap();
}
```

- [x] **Step 2: Add a Debug-only rewrite and destination-selection queue**

Under `cfg(all(feature = "e2e", debug_assertions))`, add an AppState-owned test registry populated by `e2e_seed_transport_fixture`:

- Maps only the existing deterministic Provider fixture URL `https://gitlab.example.test/skills/private-skill.git` to one guarded local Bare path.
- Supplies one queued Destination Parent to the trusted Host destination-selection command.
- Uses a sealed HOME/XDG/Global Git config and never touches user Global Config.
- Defines the fixed sentinel `GIT_RAMUS_E2E_TRANSPORT_REWRITE` next to the rewrite implementation so Release scanning proves that implementation was removed, rather than scanning a descriptive wildcard that might never exist in the binary.
- Is unavailable in non-e2e or Release builds.

Production Transport Service receives an empty registry and cannot add `url.*.insteadOf`. Debug execution injects the one fixed rewrite as Host-owned config only after exact source/Operation validation.

The trusted destination-selection command from Task 10 consumes the queued fixture parent in Debug E2E; its production branch continues to use `tauri_plugin_dialog::DialogExt`. HostApi holds the returned path and never returns it to a plugin.

- [x] **Step 3: Write the failing native journey**

`fixture-transport.ts` strictly parses only opaque IDs, repository names, expected branch names, and cleanup tokens; it must not return arbitrary deletion paths to TypeScript. `git-transport.e2e.ts` performs:

1. Seed Provider and Transport fixtures.
2. Open Providers and select the deterministic account.
3. Click Clone on `skills/private-skill`.
4. Assert Host navigates to Git Client `/clone/<intent-id>` while the Provider frame never receives a path.
5. Select HTTPS/System Git profile and existing fixture Project.
6. Clone and assert Repository registration plus `origin` sanitized URL.
7. Advance the Bare Remote through a guarded Debug-only native command, Fetch, and assert behind becomes 1.
8. Pull and assert ff-only result.
9. Create a local fixture Commit through a guarded command, Push with upstream, and verify the Bare ref.
10. Cancel a deliberately blocked Fetch and assert the Job becomes canceled and no child process remains.
11. Cleanup through production Project/Binding/Profile APIs, then guarded fixture cleanup.

- [x] **Step 4: Run the E2E test and observe missing fixture/UI behavior**

```powershell
npm run build:e2e --workspace @git-ramus/desktop
npm run test:e2e --workspace @git-ramus/desktop -- --spec ./e2e/git-transport.e2e.ts
```

Expected before completion: FAIL at the first missing fixture/Clone/network step.

- [x] **Step 5: Implement the fixture and make the journey pass**

Reuse the existing guarded temp-root checks: fixed prefix, direct child of system Temp, non-symlink/reparse, and exact cleanup token. All fixture Git commands use argument arrays, sealed config, explicit author config, and `commit.gpgSign=false`. Add the new spec to `wdio.conf.ts`; raise Mocha timeout only for this file through suite configuration rather than globally masking hangs.

- [x] **Step 6: Add exact release-boundary tests and CI commands**

Register `e2e_seed_transport_fixture`, remote-advance, local-commit, and cleanup handlers only under `all(feature="e2e", debug_assertions)`. Add a Release test matching existing Provider/Foundation probes. In CI run:

```powershell
cargo test --release --features e2e --manifest-path apps/desktop/src-tauri/Cargo.toml --lib tests::e2e_transport_fixture_handler_matches_the_debug_feature_boundary -- --exact
```

After `npm run desktop:build`, scan the Release binary for the fixed test URL, temp prefix, handler names, and rewrite key; fail if any are found.

- [x] **Step 7: Run Rust E2E-feature tests and full native E2E**

```powershell
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --features e2e e2e::tests
cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --features e2e --all-targets -- -D warnings
npm run build:e2e --workspace @git-ramus/desktop
npm run test:e2e --workspace @git-ramus/desktop
```

Expected: Foundation, Git Client, Provider, and Git Transport specs all PASS.

- [x] **Step 8: Commit native Transport E2E**

```powershell
git add apps/desktop/src-tauri/src/e2e.rs apps/desktop/src-tauri/src/app_state.rs apps/desktop/src-tauri/src/git/transport/service.rs apps/desktop/src-tauri/src/lib.rs apps/desktop/e2e/fixture-transport.ts apps/desktop/e2e/git-transport.e2e.ts apps/desktop/e2e/wdio.conf.ts apps/desktop/package.json .github/workflows/ci.yml
git commit -m "test: cover git transport journey"
```

### Task 16: Complete documentation, release gate, and final review

**Files:**

- Modify: `docs/development.md`
- Modify only if findings require fixes: files named by Tasks 1–15
- Update checkboxes: `docs/superpowers/plans/2026-07-20-git-transport-network-operations.md`

- [x] **Step 1: Document focused tests and real-account smoke checks**

Add commands for Contract, profile, transport integration, and native E2E tests. Document that:

- Provider PATs are not Git credentials.
- HTTPS uses system GCM and can show UI only for user-confirmed foreground operations.
- SSH uses an Agent; unknown Host Keys are never auto-accepted.
- Pull is ff-only and Force Push does not exist.
- Debug fixtures never use real accounts and Release binaries contain no rewrite.

Add manual smoke steps for GitHub HTTPS/GCM, GitHub or GitLab SSH Agent, GitLab.com, and one self-managed GitLab Remote on Windows/macOS/Linux.

- [x] **Step 2: Run the complete JavaScript/TypeScript gate**

```powershell
npm run check
npm audit --audit-level=high
```

Expected: format, lint, all workspace typechecks/tests PASS; audit reports 0 High/Critical vulnerabilities.

- [x] **Step 3: Run the complete Rust gate**

```powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets -- -D warnings
cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --features e2e --all-targets -- -D warnings
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
```

Expected: formatting, both Clippy configurations, unit tests, and all integration tests PASS.

- [x] **Step 4: Prove Release boundaries and build the desktop app**

```powershell
cargo test --release --features e2e --manifest-path apps/desktop/src-tauri/Cargo.toml --lib tests::e2e_seed_fixture_handler_matches_the_debug_feature_boundary -- --exact
cargo test --release --features e2e --manifest-path apps/desktop/src-tauri/Cargo.toml --lib tests::e2e_provider_fixture_handler_matches_the_debug_feature_boundary -- --exact
cargo test --release --features e2e --manifest-path apps/desktop/src-tauri/Cargo.toml --lib tests::e2e_transport_fixture_handler_matches_the_debug_feature_boundary -- --exact
npm run desktop:build
$extension = if ($IsWindows) { ".exe" } else { "" }
$binary = Resolve-Path "apps/desktop/src-tauri/target/release/git-ramus-desktop$extension"
$markers = @(
  "e2e_seed_transport_fixture",
  "git-ramus-e2e-transport-",
  "https://gitlab.example.test/skills/private-skill.git",
  "GIT_RAMUS_E2E_TRANSPORT_REWRITE",
  "e2e_block_transport_fetch",
  "e2e-provider-token"
)
foreach ($marker in $markers) {
  rg -a -F -- $marker $binary
  if ($LASTEXITCODE -eq 0) { throw "release binary contains transport fixture marker: $marker" }
  if ($LASTEXITCODE -ne 1) { throw "release marker scan failed for: $marker" }
}
```

Expected: all boundary probes and Release desktop build PASS; every binary marker scan returns the expected no-match exit code 1.

- [x] **Step 5: Run full Windows/Linux-equivalent native E2E locally where supported**

```powershell
npm run build:e2e --workspace @git-ramus/desktop
npm run test:e2e --workspace @git-ramus/desktop
```

Expected: all four specs PASS serially. CI supplies the other operating-system run.

- [x] **Step 6: Review the implementation against every acceptance criterion**

Use `superpowers:requesting-code-review`. Reviewer checks:

- No plugin payload contains Destination Parent, full Key Path, credential, environment, args, or RefSpec.
- No Provider PAT reaches Transport code.
- GCM UI is Host-confirmed and background mode is noninteractive.
- Pull is always ff-only; Push has no Force path.
- Config Drift and Clone Final-Path partial failure never overwrite/delete user data.
- Shared Repository Write Locks cover Commit/Profile/Pull/Push races.
- E2E rewrite/fixture code is Debug-only and absent from Release.

Address each concrete finding with a failing regression test first, rerun its focused suite, and commit fixes separately.

- [x] **Step 7: Check plan completion and commit documentation**

Only after every corresponding command has passed, mark this plan's checkboxes complete. Then run:

```powershell
git diff --check
git status --short
git add docs/development.md docs/superpowers/plans/2026-07-20-git-transport-network-operations.md
git commit -m "docs: complete git transport plan"
```

Expected: commit succeeds; only intentional review-fix commits remain in history and the worktree is clean.

---

## Final acceptance checklist

- [x] Manual and Provider Clone both use the trusted Git Client wizard.
- [x] Clone to existing/new Project registers and opens the Repository.
- [x] Foreground HTTPS can use GCM; no background/plugin path can silently show credentials UI.
- [x] SSH and HTTPS Profiles are reusable, repository-scoped, externally readable, and secret-free.
- [x] Config switch/unbind restores original values; Drift is never overwritten silently.
- [x] Fetch refreshes Remote refs and ahead/behind without implicit prune.
- [x] Pull is strictly ff-only and never auto-stashes, merges, or rebases.
- [x] Push uses upstream or a validated Host-built target and has no Force/RefSpec path.
- [x] Cancellation terminates complete process trees and refreshes real Repository state.
- [x] Clone staging cleanup requires exact sidecar ownership; successful Final Paths are never deleted after registration failure.
- [x] Provider PAT, Git credential, and commit identity remain isolated across DB, logs, IPC, and plugin RPC.
- [x] Contracts, migrations, Rust, React, real Git integration, Release boundary, desktop build, and Windows/Linux E2E gates pass.
