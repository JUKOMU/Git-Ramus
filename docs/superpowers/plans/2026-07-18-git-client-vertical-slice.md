# Git Client Vertical Slice Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans. Steps use checkbox syntax for tracking.

**Goal:** Build a usable local Git vertical slice on top of the Foundation: recursive Project scanning, virtual Workspaces, repository snapshots and Diff, Trust-gated staging, identity profiles with signed Commit, and a global skin-plugin contract.

**Architecture:** The git-ramus.git-client UI remains a sandboxed internal plugin. Rust owns Git process execution, parsing, persistence, Trust, identity application, and theme activation. Typed plugin RPC is the only path from UI to host; all writes are serialized per repository and followed by a fresh status read.

**Tech Stack:** Tauri 2.11, Rust 1.88/edition 2024, std::process::Command, rusqlite/WAL, React 19, TypeScript 6, Zod 4, Vite single-file plugin bundles, Vitest/Testing Library, WebdriverIO Tauri E2E.

---

## Scope boundary and working rules

This plan implements only the approved Git Client Vertical Slice specification in docs/superpowers/specs/2026-07-18-git-client-vertical-slice-design.md.

It does not implement remote Fetch/Pull/Push, branches/Merge/Stash/Tag, GitHub/GitLab Provider APIs, Skills Manager, or external plugin distribution. Do not add non-functional buttons for those features.

Work in D:/Git-Ramus/.worktrees/git-client-vertical-slice on codex/git-client-vertical-slice. Keep commits small and focused. Every production behavior follows a red-green-refactor cycle: write a failing test, run it and observe the expected failure, implement the minimum, run the focused test, then run the relevant workspace checks before committing.

Baseline commands:

~~~powershell
npm ci
npx prettier --write .
git add -u
npm run check
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
~~~

On Windows, run Cargo commands from the Visual Studio Developer PowerShell with the MSVC environment initialized. The repository's core.autocrlf can make Prettier report line-ending-only changes; normalize with npx prettier --write . and git add -u without committing content changes.

## File map

### Shared contracts and SDK

- packages/contracts/src/plugin.ts — route-aware navigation and optional theme contribution schemas.
- packages/contracts/src/rpc.ts — route-aware host:init and host:theme-changed messages.
- packages/contracts/src/theme.ts — ThemeDefinition and token schemas.
- packages/contracts/src/git.ts — Project, Workspace, Repository, Snapshot, Change, Identity and operation DTOs.
- packages/contracts/src/index.ts — exports all contracts.
- packages/contracts/src/__tests__/contracts.test.ts — contract acceptance/rejection tests.
- packages/plugin-sdk/src/client.ts — route and theme state exposed to plugin code.
- packages/plugin-sdk/src/theme.ts — safe token application and theme subscription helper.
- packages/plugin-sdk/src/__tests__/client.test.ts — lifecycle and theme update tests.

### Rust host

- apps/desktop/src-tauri/migrations/0002_git_client.sql — v2 tables and indexes.
- apps/desktop/src-tauri/src/db/migrations.rs — transactional v2 migration runner.
- apps/desktop/src-tauri/src/git/mod.rs — Git module exports.
- apps/desktop/src-tauri/src/git/engine.rs — bounded argument-array Git process runner.
- apps/desktop/src-tauri/src/git/parser.rs — Porcelain v2, Diff and Config parsers.
- apps/desktop/src-tauri/src/git/repository.rs — repository detection, path normalization and per-repository locks.
- apps/desktop/src-tauri/src/git/service.rs — Project scan, snapshots, stage/unstage and Commit orchestration.
- apps/desktop/src-tauri/src/identity.rs — Identity Profile service and Global/Local application.
- apps/desktop/src-tauri/src/themes.rs — theme discovery, validation and activation.
- apps/desktop/src-tauri/src/app_state.rs — registers Git, Identity and Theme services.
- apps/desktop/src-tauri/src/commands.rs — typed Tauri command adapters.
- apps/desktop/src-tauri/src/plugins/manifest.rs — Rust-side theme contribution and route contracts.
- apps/desktop/src-tauri/src/plugins/registry.rs — loads validated theme definition assets.
- apps/desktop/src-tauri/src/error.rs — Git, Trust, signature and path error mappings.
- apps/desktop/src-tauri/Cargo.toml / Cargo.lock — dialog plugin and any required dependency locks.
- apps/desktop/src-tauri/capabilities/default.json — native dialog permission.

### Desktop shell and RPC

- apps/desktop/src/plugins/rpcRouter.ts — typed Git/Identity/Theme route dispatch and capability mapping.
- apps/desktop/src/plugins/PluginFrame.tsx — route and theme message delivery.
- apps/desktop/src/plugins/PluginHost.tsx — selected plugin/route state.
- apps/desktop/src/App.tsx — navigation route, theme state and host events.
- apps/desktop/src/shell/AppShell.tsx — semantic Shell slots and theme variables.
- apps/desktop/src/app.css — token-based host styles and density variants.
- apps/desktop/src/lib/hostApi.ts — host-side job/theme helpers used by the Shell.
- apps/desktop/src/__tests__/App.test.tsx / FoundationFlow.test.tsx — route and theme regression tests.

### Built-in plugins and tooling

- plugins/git-client/package.json — Git Client workspace package.
- plugins/git-client/plugin.json — internal Git Client Manifest and permissions.
- plugins/git-client/index.html / src/main.tsx — single-file plugin entrypoint.
- plugins/git-client/src/api.ts — typed plugin RPC client.
- plugins/git-client/src/views/OverviewView.tsx — progressive repository overview.
- plugins/git-client/src/views/ProjectsView.tsx — project open/scan settings.
- plugins/git-client/src/views/WorkspacesView.tsx — virtual workspace membership.
- plugins/git-client/src/views/RepositoryView.tsx — Changes/Diff/Trust/Commit.
- plugins/git-client/src/components/IdentityPicker.tsx / ChangeList.tsx — focused UI components.
- plugins/git-client/src/style.css — plugin-local styles consuming host tokens.
- plugins/git-client/src/__tests__/OverviewView.test.tsx, RepositoryView.test.tsx and ChangeList.test.tsx — view and RPC behavior tests.
- plugins/builtin-compact-theme/package.json / plugin.json / theme.json — demonstration global skin plugin.
- plugins/builtin-compact-theme/index.html / src/main.ts — optional theme settings/preview UI.
- scripts/sync-builtin-plugins.mjs — syncs Welcome, Git Client and Compact Theme resources.
- apps/desktop/e2e/git-client.e2e.ts — native vertical-slice journey.
- apps/desktop/e2e/wdio.conf.ts — existing native runner configuration.
- .github/workflows/ci.yml — Git Client checks and E2E gates.
- docs/development.md — local Git test prerequisites and commands.

---

### Task 1: Extend contracts for routes, Git DTOs and skin plugins

**Files:**

- Modify: packages/contracts/src/plugin.ts
- Modify: packages/contracts/src/rpc.ts
- Create: packages/contracts/src/theme.ts
- Create: packages/contracts/src/git.ts
- Modify: packages/contracts/src/index.ts
- Test: packages/contracts/src/__tests__/contracts.test.ts
- Test: packages/plugin-sdk/src/__tests__/client.test.ts

- [x] Step 1: Add failing contract tests

Add tests for these behaviors before changing schemas:

~~~typescript
it('accepts a route-aware host init and theme contribution', () => {
  const descriptor = pluginDescriptorSchema.parse({
    manifest: {
      schemaVersion: 1,
      id: 'git-ramus.theme.compact',
      name: 'Compact',
      version: '0.1.0',
      publisher: 'git-ramus',
      description: 'Compact skin',
      kind: 'builtin',
      sdkVersion: '^0.1.0',
      entrypoints: { ui: 'ui.html' },
      contributions: {
        navigation: [],
        theme: { themeId: 'git-ramus.theme.compact', definition: 'theme.json' }
      },
      permissions: []
    },
    uiUrl: 'http://git-ramus-plugin.localhost/git-ramus.theme.compact/ui.html'
  });
  expect(descriptor.manifest.contributions.theme?.themeId).toBe('git-ramus.theme.compact');
});

it('rejects theme tokens outside the declared schema', () => {
  expect(() => themeDefinitionSchema.parse({
    themeId: 'bad',
    tokens: { script: 'alert(1)' }
  })).toThrow();
});
~~~

Add RPC tests for host:init with route and host:theme-changed containing a validated token set. Run:

~~~powershell
npm run test --workspace @git-ramus/contracts -- contracts
npm run test --workspace @git-ramus/plugin-sdk -- client
~~~

Expected: FAIL because themeDefinitionSchema, the theme contribution and new messages do not exist.

- [x] Step 2: Implement the minimal shared schemas

Define ThemeDefinition with an allowlisted token object (colors, typography, spacing, shape, elevation, motion, density) and a safe themeId. Define Git DTOs for Project, Workspace, Repository, RepositorySnapshot, ChangeEntry, IdentityProfile, EffectiveIdentity, and operation responses. Keep all IDs opaque UUID strings and all paths host-returned strings.

Make hostInitSchema.route optional for backward compatibility; the SDK must use / when absent. Add themeContributionSchema to PluginContributions and themeChangedSchema to the host-to-plugin union. Export the new modules from index.ts.

- [x] Step 3: Extend the SDK lifecycle

Expose client.ready with the optional route, add client.theme and client.onThemeChanged(listener), and apply only validated CSS custom properties to the plugin document root. A theme update must not evaluate CSS or JavaScript received from the host.

- [x] Step 4: Run focused tests and commit

~~~powershell
npx prettier --write packages/contracts packages/plugin-sdk
npm run typecheck --workspace @git-ramus/contracts
npm run typecheck --workspace @git-ramus/plugin-sdk
npm run test --workspace @git-ramus/contracts
npm run test --workspace @git-ramus/plugin-sdk
git add packages/contracts packages/plugin-sdk
git commit -m "feat: add git and theme contracts"
~~~

Expected: all contract and SDK tests pass.

### Task 2: Add SQLite v2 models and repositories

**Files:**

- Create: apps/desktop/src-tauri/migrations/0002_git_client.sql
- Modify: apps/desktop/src-tauri/src/db/migrations.rs
- Create: apps/desktop/src-tauri/src/git/model.rs
- Create: apps/desktop/src-tauri/src/git/repository.rs
- Create: apps/desktop/src-tauri/src/identity.rs (persistence types only in this task)
- Modify: apps/desktop/src-tauri/src/git/mod.rs
- Test: apps/desktop/src-tauri/src/db/mod.rs and new repository tests

- [x] Step 1: Write migration and repository invariant tests first

Add Rust tests that open an in-memory database and assert:

- PRAGMA user_version = 2.
- All v2 tables exist with foreign keys enabled.
- Duplicate Project roots and Repository canonical paths fail.
- A Workspace can reference two Projects and deleting a membership does not delete either Project.
- Only one global_settings row exists.

Run:

~~~powershell
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml db::
~~~

Expected: FAIL because migration v2 and repositories are absent.

- [x] Step 2: Add the transactional v2 migration

Create 0002_git_client.sql with the tables and constraints from the specification, including the themes table. Use TEXT UUIDs, ISO-8601 timestamps, JSON text for exclusion patterns and theme definitions, ON DELETE CASCADE only for relationship rows, and indexes on canonical paths, snapshot refresh time and relationship foreign keys. End the transaction with PRAGMA user_version = 2.

Update db/migrations.rs to run migration 1 when needed, then migration 2 when current < 2; never rerun an already-applied migration.

- [x] Step 3: Implement focused repositories

Implement small repository types with methods for Project, Workspace, Repository, Snapshot, Trust, Identity Profile and Theme rows. Every query must use rusqlite parameters. Use one transaction for membership changes and Global identity pointer changes. Map missing rows to AppError::NotFound and constraint violations to stable validation errors.

- [x] Step 4: Verify and commit

~~~powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml db::
git add apps/desktop/src-tauri/migrations/0002_git_client.sql apps/desktop/src-tauri/src/db apps/desktop/src-tauri/src/git apps/desktop/src-tauri/src/identity.rs
git commit -m "feat: persist git client models"
~~~

### Task 3: Build the bounded Git Engine and parsers

**Files:**

- Create: apps/desktop/src-tauri/src/git/engine.rs
- Create: apps/desktop/src-tauri/src/git/parser.rs
- Modify: apps/desktop/src-tauri/src/git/mod.rs
- Modify: apps/desktop/src-tauri/src/error.rs
- Test: apps/desktop/src-tauri/src/git/engine.rs
- Test: apps/desktop/src-tauri/src/git/parser.rs
- Create: apps/desktop/src-tauri/tests/git_integration.rs

- [x] Step 1: Add parser fixture tests

Add fixtures for status --porcelain=v2 -z --branch containing a branch header, staged/unstaged changes, an untracked Unicode path, a rename with spaces, and a conflict. Assert exact RepositorySnapshot counts and ChangeEntry fields. Add Diff tests proving binary markers and paths after -- are retained.

Run:

~~~powershell
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml git::parser
~~~

Expected: FAIL because parser types and functions do not exist.

- [x] Step 2: Define command and output types

Implement:

~~~rust
pub struct GitCommand {
    pub repo: PathBuf,
    pub args: Vec<OsString>,
    pub stdin: Option<Vec<u8>>,
    pub timeout: Duration,
}

pub struct GitOutput {
    pub status: ExitStatus,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}

pub trait GitRunner: Send + Sync {
    fn run(&self, command: GitCommand) -> Result<GitOutput, AppError>;
}
~~~

Use std::process::Command with current_dir/-C, argument arrays, bounded output collection, timeout polling and child termination. Do not invoke cmd.exe, PowerShell, or a shell interpreter. Preserve only the minimum environment required for Git/GCM/SSH and redact credentials from errors.

- [x] Step 3: Implement parsers and repository detection

Parse NUL-separated records without converting the entire stream through a platform code page. Implement parse_status_v2, parse_diff_summary, parse_git_config, and detect_repository. Canonicalize paths, reject non-UTF8 paths with a stable error, and recognize normal, bare and worktree repositories.

- [x] Step 4: Add real temporary-repository integration tests

Create temporary repositories with git init, configure only local identity, create files with spaces and Unicode names, and exercise status, Diff and parser output. Add a test that a path containing shell metacharacters is passed as one argument and never executed.

- [x] Step 5: Verify and commit

~~~powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml git::
git add apps/desktop/src-tauri/src/git apps/desktop/src-tauri/src/error.rs apps/desktop/src-tauri/tests/git_integration.rs
git commit -m "feat: add bounded git engine"
~~~

Expected: parser, engine and integration tests pass.

### Task 4: Implement Project/Workspace scan and repository operations

**Files:**

- Create: apps/desktop/src-tauri/src/git/service.rs
- Modify: apps/desktop/src-tauri/src/app_state.rs
- Modify: apps/desktop/src-tauri/src/commands.rs
- Modify: apps/desktop/src-tauri/src/lib.rs
- Test: apps/desktop/src-tauri/tests/git_integration.rs
- Test: apps/desktop/src-tauri/src/git/service.rs

- [x] Step 1: Write failing service tests

Using a temporary root, create repo-a, nested/repo-b, an excluded node_modules/repo-c, a .git file Worktree and a Bare repository. Assert a depth-3 scan returns the correct repository kinds, deduplicates canonical paths, ignores excluded directories and preserves the old snapshot when one repository becomes unreadable. Add tests for Workspace membership across two Project roots.

Run:

~~~powershell
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml git::service
~~~

Expected: FAIL because scan/service methods do not exist.

- [x] Step 2: Implement scan and snapshot service

Implement scan_project, refresh_repository, get_overview, get_changes, and get_diff. Use a bounded semaphore for read-only work and a per-repository lock for writes. Return progressive result records rather than waiting for all repositories. Store relationships and summary snapshots through the v2 repositories.

Add explicit validation that a requested repository belongs to the Project/Workspace context supplied by the caller.

- [x] Step 3: Implement Stage/Unstage/Commit orchestration

Add service methods that:

1. Validate the repository and requested paths against the latest change set.
2. Check Trust for writes.
3. Acquire the repository write lock.
4. Run git add -- or git restore --staged -- with path arrays.
5. For Commit, require a non-empty message and a non-empty staged set, then pass the message on stdin to git commit -F -.
6. Refresh the snapshot and return the updated job/result.

On any failure, release the lock and refresh status before returning the error.

- [x] Step 4: Add Tauri command adapters

Expose typed commands for Project/Workspace CRUD, scan, overview, snapshot, Changes, Diff, Stage, Unstage, Commit and Trust. Register them in lib.rs; return CommandResult<T> with ErrorEnvelope. Do not expose a generic run-git command.

- [x] Step 5: Verify and commit

~~~powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml git::
git add apps/desktop/src-tauri/src/app_state.rs apps/desktop/src-tauri/src/commands.rs apps/desktop/src-tauri/src/lib.rs apps/desktop/src-tauri/src/git
git commit -m "feat: scan projects and commit staged changes"
~~~

### Task 5: Implement Identity Profile and signed Commit behavior

**Files:**

- Modify: apps/desktop/src-tauri/src/identity.rs
- Modify: apps/desktop/src-tauri/src/git/service.rs
- Modify: apps/desktop/src-tauri/src/app_state.rs
- Modify: apps/desktop/src-tauri/src/commands.rs
- Test: apps/desktop/src-tauri/src/identity.rs
- Test: apps/desktop/src-tauri/tests/git_integration.rs

- [x] Step 1: Write failing identity tests

Use an isolated temporary Git config file/home so tests never touch the developer's Global config. Cover:

- Importing an existing Global name/email into the first Profile.
- Creating two profiles and moving the unique Global pointer.
- Binding a repository to a non-global profile.
- Applying Global/Local config and reading it back.
- Detecting external Local drift without overwriting it.
- Restoring follow Global by removing only Git-Ramus-managed Local keys.
- Returning a user-action error when a configured signing program is unavailable.

Run:

~~~powershell
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml identity::
~~~

Expected: FAIL because IdentityService and v2 persistence are incomplete.

- [x] Step 2: Implement Profile validation and effective identity

Validate non-empty display name, valid email shape, supported gpg.format, and signing key requirements. Implement list, create, update, delete, set_global, bind_repository, unbind_repository, and effective_for_repository. Refuse deleting the current Global Profile.

- [x] Step 3: Implement safe config application and signature checks

Snapshot the exact keys Git-Ramus manages, apply with git config --global/--local, read back, and roll back on any mismatch. For a signed Commit, use the selected profile's commit.gpgSign and gpg.format; run a lightweight availability check before starting the Commit job and preserve the original Git error in a redacted envelope.

- [x] Step 4: Verify and commit

~~~powershell
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml identity::
git add apps/desktop/src-tauri/src/identity.rs apps/desktop/src-tauri/src/git apps/desktop/src-tauri/src/commands.rs apps/desktop/src-tauri/src/app_state.rs apps/desktop/src-tauri/tests/git_integration.rs
git commit -m "feat: manage git identity profiles"
~~~

### Task 6: Connect typed RPC routes and route-aware plugin hosting

**Files:**

- Modify: apps/desktop/src/plugins/rpcRouter.ts
- Modify: apps/desktop/src/plugins/PluginFrame.tsx
- Modify: apps/desktop/src/plugins/PluginHost.tsx
- Modify: apps/desktop/src/App.tsx
- Modify: apps/desktop/src/lib/hostApi.ts
- Modify: apps/desktop/src/__tests__/FoundationFlow.test.tsx
- Modify: apps/desktop/src/plugins/__tests__/PluginFrame.test.tsx

- [x] Step 1: Add failing RPC route tests

Add tests that dispatch projects.list, repositories.getChanges, repositories.stage, repositories.commit, identities.setGlobal, and repositories.trust. Assert each route checks the exact capability/resource before calling the Rust-backed HostApi. Add negative tests for an unknown Repository ID, an untrusted write, and an undeclared capability.

Run:

~~~powershell
npm run test --workspace @git-ramus/desktop -- rpcRouter PluginFrame
~~~

Expected: FAIL because the routes and route-aware frame props do not exist.

- [x] Step 2: Implement typed HostApi and RPC dispatch

Add TypeScript request/response methods matching packages/contracts/src/git.ts. Keep resource checks in a single route table. Never let a plugin supply a filesystem path to the route handler; resolve IDs through HostApi.

- [x] Step 3: Add route and theme message handling to PluginFrame

Pass the selected contribution route in host:init. Subscribe to host:theme-changed, apply the validated theme through the SDK, and keep the existing source/session checks. Add tests for route delivery, theme updates, and rejection of messages from another frame.

- [x] Step 4: Verify and commit

~~~powershell
npx prettier --write apps/desktop/src packages/contracts/src packages/plugin-sdk/src
npm run lint
npm run typecheck --workspace @git-ramus/desktop
npm run test --workspace @git-ramus/desktop
git add apps/desktop/src packages/contracts/src packages/plugin-sdk/src
git commit -m "feat: expose git client host routes"
~~~

### Task 7: Build the Git Client plugin UI and navigation

**Files:**

- Create: plugins/git-client/package.json
- Create: plugins/git-client/plugin.json
- Create: plugins/git-client/index.html
- Create: plugins/git-client/src/main.tsx
- Create: plugins/git-client/src/api.ts
- Create: plugins/git-client/src/App.tsx
- Create: plugins/git-client/src/views/OverviewView.tsx
- Create: plugins/git-client/src/views/ProjectsView.tsx
- Create: plugins/git-client/src/views/WorkspacesView.tsx
- Create: plugins/git-client/src/views/RepositoryView.tsx
- Create: plugins/git-client/src/components/ChangeList.tsx
- Create: plugins/git-client/src/components/IdentityPicker.tsx
- Create: plugins/git-client/src/style.css
- Create: plugins/git-client/vite.config.ts
- Create: plugins/git-client/tsconfig.json
- Create: plugins/git-client/src/__tests__/OverviewView.test.tsx
- Create: plugins/git-client/src/__tests__/RepositoryView.test.tsx
- Create: plugins/git-client/src/__tests__/ChangeList.test.tsx
- Modify: scripts/sync-builtin-plugins.mjs
- Modify: apps/desktop/src/shell/AppShell.tsx
- Modify: apps/desktop/src/app.css

- [x] Step 1: Write failing component tests

Add Testing Library tests for:

- Overview rendering a loading state, progressive repository rows and filter selection.
- Projects opening a root through the typed API and editing depth/exclusions.
- Workspaces adding/removing projects without changing filesystem paths.
- Repository Changes separating Staged/Unstaged/Untracked and showing Diff.
- ChangeList selecting one path versus all paths.
- IdentityPicker showing effective source, Global badge and signing status.
- Commit button disabled for an empty message, no staged changes or missing Trust.

Run the new plugin test command and observe failures because the package does not exist.

- [x] Step 2: Scaffold the single-file plugin

Follow the existing Welcome Vite configuration. Set the package name to @git-ramus/git-client, add the Manifest with navigation routes /overview, /projects, and /workspaces, and use permissions from the specification. The plugin API must only call the typed RPC client; it must not import Tauri APIs or access window.__TAURI_INTERNALS__.

- [x] Step 3: Implement the views and explicit staging flow

Implement route-based rendering. Overview requests snapshots in batches. Projects and Workspaces use optimistic relationship updates only after the host confirms success. RepositoryView refreshes after Stage/Unstage/Commit, keeps selected paths stable when possible, and displays ErrorEnvelope recovery actions.

The Commit panel sends only selected staged paths and the selected identity profile ID; it never auto-stages unstaged files. Add an explicit Stage all button.

- [x] Step 4: Integrate navigation and resource sync

Generalize sync-builtin-plugins.mjs to build/copy Welcome and Git Client into resources/plugins/<id>. Make AppShell render plugin navigation contributions rather than no-op hardcoded buttons. Keep TaskCenter visible as a host slot; Task 8 adds the Compact Theme copy step after that plugin exists.

- [x] Step 5: Verify and commit

~~~powershell
npm run build --workspace @git-ramus/builtin-welcome
npm run build --workspace @git-ramus/git-client
npm run typecheck --workspace @git-ramus/git-client
npm run test --workspace @git-ramus/git-client
npm run typecheck
npm run test
git add plugins/git-client scripts/sync-builtin-plugins.mjs apps/desktop/src/shell apps/desktop/src/app.css apps/desktop/src/App.tsx
git commit -m "feat: add git client plugin views"
~~~

### Task 8: Add global ThemeManager and Compact skin plugin

**Files:**

- Create: plugins/builtin-compact-theme/package.json
- Create: plugins/builtin-compact-theme/plugin.json
- Create: plugins/builtin-compact-theme/theme.json
- Create: plugins/builtin-compact-theme/index.html
- Create: plugins/builtin-compact-theme/src/main.ts
- Create: plugins/builtin-compact-theme/vite.config.ts
- Create: plugins/builtin-compact-theme/tsconfig.json
- Create: apps/desktop/src-tauri/src/themes.rs
- Modify: apps/desktop/src-tauri/src/app_state.rs
- Modify: apps/desktop/src-tauri/src/commands.rs
- Modify: apps/desktop/src-tauri/src/plugins/manifest.rs
- Modify: apps/desktop/src-tauri/src/plugins/registry.rs
- Modify: apps/desktop/src/App.tsx
- Modify: apps/desktop/src/shell/AppShell.tsx
- Modify: apps/desktop/src/app.css
- Modify: apps/desktop/src/plugins/PluginFrame.tsx
- Test: Rust theme tests and apps/desktop/src/__tests__/App.test.tsx

- [x] Step 1: Write failing theme tests

Test that a valid Compact theme.json is discovered, an out-of-range token is rejected, activating a theme persists active_theme_id, invalid activation falls back to the default, and a host:theme-changed message reaches an iframe without allowing cross-origin injection.

Run the focused Rust and desktop tests and observe failures.

- [x] Step 2: Implement ThemeManager and theme persistence

Load theme definitions from validated plugin roots, store only the active ID and definition metadata, expose list/activate commands, and emit a theme-changed event. Use the host default when no plugin theme is active. Never load raw CSS or executable theme content into the host.

- [x] Step 3: Apply tokens to Shell and SDK

Render CSS variables from the validated definition at the Shell root. Add density classes for host slots. Extend PluginFrame to send the current theme on init and on changes; the plugin SDK applies allowlisted variables to its own document root.

- [x] Step 4: Add the Compact plugin and switcher

Create the @git-ramus/builtin-compact-theme plugin with a visibly different density, palette and component geometry. Add a small host Settings/toolbar selector that lists themes and calls activate_theme. Verify Git Client, TaskCenter and plugin iframe all update without reload.

- [x] Step 5: Verify and commit

~~~powershell
npx prettier --write apps/desktop/src packages/plugin-sdk/src plugins/builtin-compact-theme
npm run lint
npm run typecheck
npm run test
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml
cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
git add apps/desktop/src apps/desktop/src-tauri plugins/builtin-compact-theme packages/plugin-sdk/src
git commit -m "feat: support global skin plugins"
~~~

### Task 9: Add native folder selection, end-to-end journey and CI gates

**Files:**

- Modify: apps/desktop/src-tauri/Cargo.toml / Cargo.lock
- Modify: apps/desktop/src-tauri/capabilities/default.json
- Modify: apps/desktop/src-tauri/src/lib.rs
- Modify: apps/desktop/src-tauri/src/commands.rs
- Modify: apps/desktop/src/lib/hostApi.ts
- Create: apps/desktop/e2e/fixture-project.ts
- Create: apps/desktop/e2e/git-client.e2e.ts
- Create: apps/desktop/src-tauri/src/e2e.rs
- Modify: apps/desktop/e2e/wdio.conf.ts
- Modify: .github/workflows/ci.yml
- Modify: docs/development.md

- [x] Step 1: Add the native dialog dependency and command

Add tauri-plugin-dialog 2.7.1 to Cargo.toml and @tauri-apps/plugin-dialog 2.7.1 to apps/desktop/package.json, run npm install and cargo check, enable the dialog default capability, and expose a host command that returns a selected directory or null. The command must not accept arbitrary plugin-provided paths as a substitute for the dialog.

- [x] Step 2: Write the native E2E journey

Add a WebdriverIO spec that:

1. Opens the Git Client route.
2. Calls the debug-only e2e_seed_fixture command to create two temporary repositories and returns their root plus Project ID. The test helper in fixture-project.ts invokes that command before the session and removes the temporary directory in an after hook. The command is compiled only with both e2e and debug_assertions and is never present in a release build; opening and all Git operations then use the normal plugin route.
3. Waits for a repository snapshot and verifies the depth/exclusion behavior.
4. Creates a Workspace containing two Projects.
5. Opens Repository Detail, verifies sandbox="allow-scripts" and theme handshake.
6. Trusts the repository, stages one file, confirms another remains unstaged, selects an Identity Profile, and commits.
7. Switches to the Compact skin and asserts the Shell theme marker and iframe token update.

Because the embedded driver cannot inspect the opaque cross-origin plugin DOM directly, expose host-side status attributes only for handshake/operation observability, as in the Foundation E2E. Do not weaken the production sandbox.

- [x] Step 3: Add CI commands and documentation

Keep Node 24 and Node 26 quality jobs, MSVC initialization on Windows, Rust fmt/Clippy/tests, Windows/Linux E2E, and Linux WebView dependencies. Document Git installation requirements, test fixture setup, isolated Git config, Trust behavior, and theme plugin development.

- [x] Step 4: Verify and commit

~~~powershell
npm run check
npm run build:e2e --workspace @git-ramus/desktop
npm run test:e2e --workspace @git-ramus/desktop
npm audit --audit-level=high
git diff --check
git add apps/desktop .github/workflows/ci.yml docs/development.md package-lock.json
git commit -m "test: cover git client vertical slice"
~~~

### Task 10: Complete the release gate and review

**Files:**

- Modify only files required by verification findings; do not broaden scope.
- Update: docs/superpowers/plans/2026-07-18-git-client-vertical-slice.md checkboxes.

- [x] Step 1: Run the complete clean-install gate

~~~powershell
npm ci
npx prettier --write .
git add -u
npm run check
cargo fmt --manifest-path apps/desktop/src-tauri/Cargo.toml -- --check
cargo clippy --manifest-path apps/desktop/src-tauri/Cargo.toml --all-targets -- -D warnings
cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml
npm run desktop:build
npm run build:e2e --workspace @git-ramus/desktop
npm run test:e2e --workspace @git-ramus/desktop
npm audit --audit-level=high
git diff --check
git status --short
~~~

Expected: all checks pass, native E2E reports a passing Git Client journey, audit reports zero high vulnerabilities, and only intended committed files remain.

- [x] Step 2: Perform a security and scope review

Check that no command accepts arbitrary shell strings, no plugin receives secrets, Trust gates every write, signatures never silently downgrade, theme definitions are schema-only, and no remote/provider/Skills feature slipped into this plan.

- [x] Step 3: Request code review and finalize

Run the project code-review skill against the final diff. Address concrete findings with a failing regression test first. Update all plan checkboxes only after the corresponding command has passed, then report the final commit list and verification evidence.
